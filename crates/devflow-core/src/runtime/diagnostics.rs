//! Owner diagnostics export shared by HTTP and MCP. Launch configuration is omitted; log redaction is best effort.
use super::process::{LogLine, ProcessOutput};
use crate::{DevFlowEngine, workspace::{DetectedProject, WorkspaceError}};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsQuery {
    pub project_path: String,
    pub kind: DiagnosticsKind,
    pub script: Option<String>,
    pub run_id: Option<String>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticsKind { Java, Node }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsContext {
    pub run_id: String,
    pub state: String,
    pub truncated: bool,
    pub redacted: bool,
    pub text: String,
    pub excerpt: String,
}

fn conflict() -> WorkspaceError {
    WorkspaceError { code: "conflict", message: "运行记录已切换，请等待面板刷新后重新复制排查上下文".into() }
}
fn check_run(output: &ProcessOutput, expected: Option<&str>) -> Result<(), WorkspaceError> {
    if expected.is_some_and(|id| id != output.run_id) { return Err(conflict()); }
    Ok(())
}

impl DevFlowEngine {
    pub fn runtime_diagnostics(&self, query: DiagnosticsQuery) -> Result<DiagnosticsContext, WorkspaceError> {
        let project = match query.kind {
            DiagnosticsKind::Java => {
                if query.script.is_some() { return Err(WorkspaceError { code: "invalid_request", message: "Java 排查上下文不接受 Node 脚本".into() }); }
                self.workspace.runtime_project(&query.project_path, None)?
            },
            DiagnosticsKind::Node => self.workspace.node_runtime_project(&query.project_path, query.script.as_deref(), None)?,
        };
        let read = |after, run_id: Option<&str>| match query.kind {
            DiagnosticsKind::Java => self.java.output(&project, after, run_id),
            DiagnosticsKind::Node => self.node.output(&project, after, run_id),
        };
        let first = read(0, None)?;
        if first.state == "external" {
            if query.run_id.as_deref().is_some_and(|id| !id.is_empty()) { return Err(conflict()); }
            return Ok(build_context(&project, &first, &[], false, query.kind));
        }
        check_run(&first, query.run_id.as_deref())?;
        // Pin a run and an upper cursor. Read at most the 2,000 retained lines,
        // even if the process continues producing output during export.
        let snapshot = read(u64::MAX, Some(&first.run_id))?;
        check_run(&snapshot, Some(&first.run_id))?;
        let mut cursor = snapshot.next_cursor.saturating_sub(2000);
        let mut truncated = cursor > 0;
        let mut lines = Vec::new();
        for _ in 0..4 {
            let page = read(cursor, Some(&snapshot.run_id))?;
            check_run(&page, Some(&snapshot.run_id))?;
            truncated |= page.truncated;
            lines.extend(page.lines.into_iter().filter(|line| line.sequence <= snapshot.next_cursor));
            if page.next_cursor <= cursor || page.next_cursor >= snapshot.next_cursor { break; }
            cursor = page.next_cursor;
        }
        Ok(build_context(&project, &snapshot, &lines, truncated, query.kind))
    }
}

pub(crate) fn redact(value: &str) -> String {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let rules = RULES.get_or_init(|| vec![
        (Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----").unwrap(), "[REDACTED PRIVATE KEY]"),
        (Regex::new(r#"(?im)(\b(?:authorization|proxy-authorization|cookie|set-cookie)["']?\s*[:=]\s*)[^\r\n]+"#).unwrap(), "${1}[REDACTED]"),
        (Regex::new(r"(?i)\b(Bearer|Basic)\s+[A-Za-z0-9_+/=.\-]+").unwrap(), "$1 [REDACTED]"),
        (Regex::new(r"(?i)([a-z][a-z0-9+.-]*://)[^\s/@]+(?::[^\s/@]*)?@").unwrap(), "${1}[REDACTED]@"),
        (Regex::new(r#"(?i)((?:[\"']?)(?:password|passwd|pwd|secret|client[_-]?secret|access[_-]?token|refresh[_-]?token|api[_-]?key|token)(?:[\"']?)\s*[:=]\s*)(?:\"(?:\\.|[^\"\\])*\"|'[^']*'|[^\s,;&}\]]+)"#).unwrap(), "${1}[REDACTED]"),
    ]);
    rules.iter().fold(value.to_owned(), |text, (pattern, replacement)| pattern.replace_all(&text, *replacement).into_owned())
}

fn build_context(project: &DetectedProject, output: &ProcessOutput, lines: &[LogLine], mut truncated: bool, kind: DiagnosticsKind) -> DiagnosticsContext {
    let node = matches!(kind, DiagnosticsKind::Node);
    let kind = if node { "Node.js" } else { "Java" };
    let mut selected = std::collections::BTreeSet::new();
    // Keep the first explicit failure and its nearby stack trace, plus the tail.
    if let Some(index) = lines.iter().position(|line| {
        let text = line.text.to_lowercase();
        text.contains("error") || text.contains("exception") || text.contains("failed") || text.contains("失败") || text.contains("错误")
    }) {
        selected.extend(index.saturating_sub(5)..(index + 25).min(lines.len()));
    }
    selected.extend(lines.len().saturating_sub(100)..lines.len());
    truncated |= selected.len() < lines.len();
    let mut raw = String::new();
    let mut previous = None;
    for index in selected {
        if previous.is_some_and(|last| index > last + 1) { raw.push_str("… 中间日志省略 …\n"); }
        let line = &lines[index];
        raw.push_str(&format!("[{}][{}] {}\n", line.sequence, line.stream, line.text));
        previous = Some(index);
    }
    // Redact before clipping so a truncated credential cannot escape matching.
    let sanitized = redact(&raw);
    let redacted = sanitized != raw;
    let excerpt = if sanitized.chars().count() > 24_000 {
        truncated = true;
        format!("{}\n… 日志超出复制长度上限 …", sanitized.chars().take(24_000).collect::<String>())
    } else { sanitized };
    let external = output.state == "external";
    let script = project.node_script.as_ref().map(|script| format!("\n脚本：{script}")).unwrap_or_default();
    let text = format!(
        "请通过 DevFlow MCP 排查此服务，按需读取日志和源码。\n项目：{}\n类型：{kind}{script}\n运行：{}\n先定位原因，未经要求不要编译、重启或停止服务。",
        project.path, if external { "外部进程" } else { &output.run_id },
    );
    DiagnosticsContext { run_id: output.run_id.clone(), state: output.state.clone(), truncated, redacted, text, excerpt }

}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_redacts_credentials_and_preserves_failure_evidence() {
        let raw = "Authorization: Bearer abc.def\nCookie: session=secret\n{\"password\":\"has spaces\",\"token\":\"hidden\"}\njdbc:mysql://user:pass@localhost/db?password=hello&x=1\napi_key=abcd\njava.lang.Exception: 中文失败";
        let result = redact(raw);
        for secret in ["abc.def", "session=secret", "has spaces", "hidden", "user:pass", "hello", "abcd"] { assert!(!result.contains(secret), "{secret}"); }
        assert!(result.contains("java.lang.Exception: 中文失败"));
        assert!(result.contains("x=1"));
        assert_eq!(redact("文件名或扩展名太长。 (os error 206)"), "文件名或扩展名太长。 (os error 206)");
    }
    #[test]
    fn redact_json_headers_and_multiline_private_keys() {
        let input = "{\"Cookie\":\"session=do-not-copy\"}\n-----BEGIN PRIVATE KEY-----\nprivate-material\n-----END PRIVATE KEY-----\nclient_secret='secret with spaces'";
        let result = redact(input);
        assert!(!result.contains("do-not-copy")); assert!(!result.contains("private-material"));
        assert!(!result.contains("secret with spaces")); assert!(result.contains("REDACTED"));
    }
    #[test]
    fn export_rejects_replaced_runs() {
        let output = super::super::process::Run::new("failed").output(0);
        assert!(check_run(&output, Some("previous-run")).is_err());
        assert!(check_run(&output, Some(&output.run_id)).is_ok());
    }
    #[test]
    fn excerpt_keeps_early_failure_and_tail_with_unicode_safe_limit() {
        let project = DetectedProject { name: "文件服务".into(), kind: crate::workspace::ProjectKind::Node, path: "D:/项目 空格".into(), markers: vec![], main_classes: vec![], launch_scripts: vec![], node_script: Some("file".into()), node_selector: None };
        let output = super::super::process::Run::new("failed").output(0);
        let mut lines: Vec<LogLine> = (0..300).map(|i| LogLine { sequence: i, stream: "stderr".into(), text: format!("普通日志 {i}") }).collect();
        lines[10].text = "首次 ERROR: 启动失败 password=secret".into();
        let context = build_context(&project, &output, &lines, false, DiagnosticsKind::Node);
        assert!(context.truncated && context.redacted);
        assert!(context.excerpt.contains("首次 ERROR")); assert!(context.excerpt.contains("普通日志 299"));
        assert!(!context.excerpt.contains("password=secret")); assert!(context.text.contains("脚本：file")); assert!(context.text.chars().count() < 400);
        lines[299].text = "中".repeat(30_000);
        let context = build_context(&project, &output, &lines, false, DiagnosticsKind::Node);
        assert!(context.truncated); assert!(context.text.chars().count() < 400);
        assert!(context.excerpt.chars().count() < 24_100);
    }
}
