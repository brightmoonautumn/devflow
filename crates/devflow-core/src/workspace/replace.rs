//! Bounded search replacement plans. Preview captures exact disk snapshots;
//! apply reuses the same authorized, conflict-checked writer as the editor.
use super::*;
use super::search::SearchMode;
use regex::{Captures, Regex, RegexBuilder};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplacePreviewRequest {
    pub search: WorkspaceSearchRequest,
    pub replacement: String,
    #[serde(default)]
    pub preserve_case: bool,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub column: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceApplyRequest { pub plan_id: String, pub expected_revision: u64 }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacePreview {
    pub plan_id: String,
    pub revision: u64,
    pub replacements: usize,
    pub files: Vec<ReplaceFilePreview>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceFilePreview { pub path: String, pub replacements: usize, pub before: String, pub after: String }

pub(super) struct ReplacementPlan {
    id: String, revision: u64, created: Instant, files: Vec<PlannedFile>,
}
struct PlannedFile { path: String, before: String, after: String, replacements: usize }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceOutcome {
    pub files: Vec<String>,
    pub replacements: usize,
    pub failure: Option<WorkspaceError>,
}

impl WorkspaceService {
    pub fn replace_preview(&self, mut request: ReplacePreviewRequest, authenticated: bool) -> Result<ReplacePreview> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        if request.replacement.len() > 16 * 1024 || request.replacement.chars().any(|c| c < ' ' && !matches!(c, '\t' | '\n' | '\r' | '\u{000c}')) {
            return Err(error("limit_exceeded", "替换文本最多 16 KiB，不能包含二进制控制字符"));
        }
        if (request.line.is_some() && request.path.is_none()) || (request.column.is_some() && request.line.is_none()) {
            return Err(error("invalid_query", "单处替换需要文件路径、行号和列号"));
        }
        let revision = request.search.expected_revision;
        let expression = replacement_regex(&request.search)?;
        request.search.mode = SearchMode::Content;
        let mut paths = Vec::new();
        if let Some(path) = &request.path {
            // A selected file is checked in full below, even when the workspace
            // result list was truncated. Its explicit path is still authorized.
            paths.push(path.clone());
        } else {
            let result = self.search(request.search.clone(), authenticated)?;
            if result.truncated || !result.warnings.is_empty() {
                return Err(error("limit_exceeded", "搜索结果不完整，无法全部替换。请缩小包含文件范围后重新搜索。"));
            }
            for found in result.matches { if !paths.contains(&found.path) { paths.push(found.path); } }
        }
        if paths.is_empty() { return Err(error("no_matches", "没有可替换的内容匹配，请重新搜索")); }
        let mut files = Vec::new();
        let mut previews = Vec::new();
        let mut bytes = 0;
        let mut total = 0;
        for path in paths {
            let file = self.read_file(WorkspaceFileReadRequest { path: path.clone(), expected_revision: revision }, true)?;
            let path = file.path.clone();
            if file.readonly { return Err(error("permission_denied", format!("{path}：{}", file.readonly_reason.unwrap_or_default()))); }
            let (after, count, before_preview, after_preview) = replace_text(&file.content, &expression, &request)?;
            if count == 0 || after == file.content { continue; }
            bytes += file.content.len() + after.len();
            total += count;
            if bytes > 8 * 1024 * 1024 || total > 20000 {
                return Err(error("limit_exceeded", "替换计划超过 8 MiB 或 20000 处，请缩小范围"));
            }
            previews.push(ReplaceFilePreview { path: path.clone(), replacements: count, before: before_preview, after: after_preview });
            files.push(PlannedFile { path, before: file.content, after, replacements: count });
        }
        if files.is_empty() { return Err(error("no_matches", "没有需要修改的内容，或单处匹配已变化，请重新搜索")); }
        if self.lock()?.revision != revision { return Err(error("conflict", "工作区已改变，请重新搜索")); }
        let id = uuid::Uuid::new_v4().to_string();
        *self.replacement.lock().map_err(|_| error("unavailable", "替换服务暂不可用"))? = Some(ReplacementPlan {
            id: id.clone(), revision, created: Instant::now(), files,
        });
        Ok(ReplacePreview { plan_id: id, revision, replacements: total, files: previews })
    }

    pub fn replace_apply(&self, request: ReplaceApplyRequest, authenticated: bool) -> Result<ReplaceOutcome> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let mut stored = self.replacement.lock().map_err(|_| error("unavailable", "替换服务暂不可用"))?;
        let plan = stored.as_ref().ok_or_else(|| error("conflict", "替换预览已失效，请重新预览"))?;
        if plan.id != request.plan_id || plan.revision != request.expected_revision || plan.created.elapsed() > Duration::from_secs(300) {
            return Err(error("conflict", "替换预览已改变或过期，请重新预览"));
        }
        // Check every file before the first write. A late I/O failure is returned
        // with the exact successful paths; a batch is not a filesystem transaction.
        for file in &plan.files {
            let current = self.read_file(WorkspaceFileReadRequest { path: file.path.clone(), expected_revision: plan.revision }, true)?;
            if current.readonly { return Err(error("permission_denied", format!("{} 为只读", file.path))); }
            if current.content != file.before { return Err(error("file_conflict", format!("{} 已被修改，未执行替换。请重新预览。", file.path))); }
        }
        let plan = stored.take().ok_or_else(|| error("conflict", "替换预览已失效"))?;
        let mut outcome = ReplaceOutcome { files: Vec::new(), replacements: 0, failure: None };
        for file in plan.files {
            match self.write_file(WorkspaceFileWriteRequest {
                path: file.path.clone(), expected_revision: plan.revision, expected_content: file.before, content: file.after,
            }, true) {
                Ok(_) => { outcome.files.push(file.path); outcome.replacements += file.replacements; }
                Err(err) => { outcome.failure = Some(err); break; }
            }
        }
        Ok(outcome)
    }
}

fn replacement_regex(request: &WorkspaceSearchRequest) -> Result<Regex> {
    if request.query.trim().is_empty() || request.query.len() > 1024 || request.query.contains(['\0', '\n', '\r']) {
        return Err(error("invalid_query", "请输入有效的单行搜索词"));
    }
    let mut pattern = if request.regex { request.query.clone() } else { regex::escape(&request.query) };
    if request.whole_word { pattern = format!(r"\b(?:{pattern})\b"); }
    RegexBuilder::new(&pattern).case_insensitive(!request.case_sensitive).size_limit(4 * 1024 * 1024)
        .build().map_err(|err| error("invalid_query", format!("正则表达式无效：{err}")))
}

fn expanded(captures: &Captures<'_>, replacement: &str, regex: bool, preserve: bool) -> Result<String> {
    let mut output = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(ch) = chars.next() {
        if regex && ch == '$' {
            match chars.peek().copied() {
                Some('$') => { chars.next(); output.push('$'); }
                Some('&') => { chars.next(); output.push_str(captures.get(0).map_or("", |m| m.as_str())); }
                Some(digit) if digit.is_ascii_digit() => {
                    let mut index = 0;
                    let mut digits = String::new();
                    for _ in 0..2 {
                        let Some(digit) = chars.peek().copied().filter(|c| c.is_ascii_digit()) else { break; };
                        chars.next(); digits.push(digit); index = index * 10 + digit.to_digit(10).unwrap_or(0) as usize;
                    }
                    if index < captures.len() { output.push_str(captures.get(index).map_or("", |m| m.as_str())); }
                    else { output.push('$'); output.push_str(&digits); }
                }
                _ => output.push('$'),
            }
        } else if regex && ch == '\\' {
            match chars.next() {
                Some('n') => output.push('\n'), Some('t') => output.push('\t'), Some('\\') => output.push('\\'),
                Some(ch) => { output.push('\\'); output.push(ch); }, None => output.push('\\'),
            }
        } else { output.push(ch); }
        if output.len() > 1024 * 1024 { return Err(error("limit_exceeded", "替换后文本超过 1 MiB")); }
    }
    if !preserve { return Ok(output); }
    let matched = captures.get(0).map_or("", |m| m.as_str());
    let letters: String = matched.chars().filter(|c| c.is_lowercase() || c.is_uppercase()).collect();
    let output = if letters.is_empty() { output }
    else if letters == letters.to_uppercase() { output.to_uppercase() }
    else if letters == letters.to_lowercase() { output.to_lowercase() }
    else if letters.chars().next().is_some_and(|c| c.is_uppercase()) && letters.chars().skip(1).all(|c| c.is_lowercase()) {
        let mut chars = output.chars();
        chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>()) + &chars.as_str().to_lowercase()
    } else { output };
    if output.len() > 1024 * 1024 { return Err(error("limit_exceeded", "替换后文本超过 1 MiB")); }
    Ok(output)
}

fn replace_text(content: &str, expression: &Regex, request: &ReplacePreviewRequest) -> Result<(String, usize, String, String)> {
    if content.replace("\r\n", "").contains('\r') {
        return Err(error("unsupported_file", "搜索替换仅支持 LF/CRLF 换行，请先转换独立 CR 换行的文件"));
    }
    let normalized = content.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut output = String::new();
    let mut count = 0;
    let mut preview = (String::new(), String::new());
    for (index, segment) in normalized.split_inclusive('\n').enumerate() {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let mut updated = String::new();
        let mut cursor = 0;
        for captures in expression.captures_iter(line) {
            let Some(matched) = captures.get(0) else { continue; };
            if request.line.is_some_and(|number| number != index as u64 + 1) || request.column.is_some_and(|column| column != line[..matched.start()].encode_utf16().count() + 1) { continue; }
            // A row action replaces the first occurrence on that row; column
            // disambiguates repeats and zero-width matches.
            if request.line.is_some() && count > 0 { continue; }
            updated.push_str(&line[cursor..matched.start()]);
            updated.push_str(&expanded(&captures, &request.replacement, request.search.regex, request.preserve_case)?);
            if updated.len() > 1024 * 1024 { return Err(error("limit_exceeded", "替换后文件超过 1 MiB")); }
            cursor = matched.end(); count += 1;
            if count > 20000 { return Err(error("limit_exceeded", "替换超过 20000 处，请缩小范围")); }
        }
        updated.push_str(&line[cursor..]);
        if preview.0.is_empty() && updated != line {
            let common = line.chars().zip(updated.chars()).take_while(|(before, after)| before == after).count();
            let start = common.saturating_sub(60);
            let prefix = if start > 0 { "…" } else { "" };
            preview = (format!("{prefix}{}", line.chars().skip(start).take(240).collect::<String>()), format!("{prefix}{}", updated.chars().skip(start).take(240).collect::<String>()));
        }
        output.push_str(&updated);
        if segment.ends_with('\n') { output.push('\n'); }
        if output.len() > 1024 * 1024 { return Err(error("limit_exceeded", "替换后文件超过 1 MiB")); }
    }
    let ending = if content.contains("\r\n") { "\r\n" } else if content.contains('\r') { "\r" } else { "\n" };
    if ending != "\n" { output = output.replace('\n', ending); }
    if content.starts_with('\u{feff}') { output.insert(0, '\u{feff}'); }
    if output.len() > 1024 * 1024 { return Err(error("limit_exceeded", "替换后文件超过 1 MiB")); }
    Ok((output, count, preview.0, preview.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(query: &str, replacement: &str, regex: bool) -> ReplacePreviewRequest {
        ReplacePreviewRequest {
            search: WorkspaceSearchRequest { query: query.into(), mode: SearchMode::Both, expected_revision: 0, case_sensitive: false, whole_word: false, regex, include_glob: String::new(), exclude_glob: String::new() },
            replacement: replacement.into(), preserve_case: false, path: None, line: None, column: None,
        }
    }
    fn transform(content: &str, request: &ReplacePreviewRequest) -> (String, usize, String, String) {
        replace_text(content, &replacement_regex(&request.search).unwrap(), request).unwrap()
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("devflow-replace-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap(); Self(path.canonicalize().unwrap())
        }
        fn service(&self) -> WorkspaceService { WorkspaceService::new(vec![self.0.clone()], Some(self.0.clone())).unwrap() }
        fn plan(&self, service: &WorkspaceService, names: &[&str]) -> ReplaceApplyRequest {
            let files = names.iter().map(|name| PlannedFile { path: display_path(&self.0.join(name)), before: "before".into(), after: "after".into(), replacements: 1 }).collect();
            *service.replacement.lock().unwrap() = Some(ReplacementPlan { id: "fixture-plan".into(), revision: 0, created: Instant::now(), files });
            ReplaceApplyRequest { plan_id: "fixture-plan".into(), expected_revision: 0 }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.0.file_name().is_some_and(|name| name.to_string_lossy().starts_with("devflow-replace-test-")) { let _ = fs::remove_dir_all(&self.0); }
        }
    }

    #[test]
    fn literal_deletion_regex_groups_escaping_and_preserve_case() {
        assert_eq!(transform("中.$ 中.$", &request("中.$", "$1", false)).0, "$1 $1");
        assert_eq!(transform("a b a", &request("a", "", false)).0, " b ");
        let transformed = transform("user=张三", &request(r"user=(\S+)", r"$1/$$/$&/\n", true));
        assert_eq!(transformed.0, "张三/$/user=张三/\n");
        assert_eq!(transform("a", &request("(a)", "$99", true)).0, "$99");
        let mut preserved = request("hello", "world", false); preserved.preserve_case = true;
        assert_eq!(transform("HELLO Hello hello", &preserved).0, "WORLD World world");
        let mut chinese = request("中文", "World", false); chinese.preserve_case = true;
        assert_eq!(transform("中文", &chinese).0, "World");
    }

    #[test]
    fn single_match_uses_utf16_column_and_preserves_bom_crlf_and_zero_width() {
        let mut one = request("中", "文", false);
        one.path = Some("source.txt".into()); one.line = Some(1); one.column = Some(5);
        let result = transform("\u{feff}😀中中\r\n中\r\n", &one);
        // Emoji occupies two UTF-16 units; the second 中 begins at column 4.
        assert_eq!(result.1, 0);
        one.column = Some(4);
        let result = transform("\u{feff}😀中中\r\n中\r\n", &one);
        assert_eq!(result.0, "\u{feff}😀中文\r\n中\r\n"); assert_eq!(result.1, 1);
        assert_eq!(transform("a\nb", &request("^", ">", true)).0, ">a\n>b");
        assert_eq!(transform("a\n", &request("^", ">", true)).0, ">a\n");
        let mut word = request("cat", "dog", false); word.search.whole_word = true;
        assert_eq!(transform("cat scatter", &word).0, "dog scatter");
        assert!(replace_text("a\rb", &replacement_regex(&word.search).unwrap(), &word).is_err());
    }

    #[test]
    fn preflight_checks_entire_batch_before_writes_and_binds_auth_plan_and_revision() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("first.txt"), "before").unwrap();
        fs::write(fixture.0.join("second.txt"), "external").unwrap();
        let service = fixture.service();
        let apply = fixture.plan(&service, &["first.txt", "second.txt"]);
        assert_eq!(service.replace_apply(apply, true).unwrap_err().code, "file_conflict");
        assert_eq!(fs::read_to_string(fixture.0.join("first.txt")).unwrap(), "before");
        let apply = fixture.plan(&service, &["first.txt"]);
        assert_eq!(service.replace_apply(apply, false).unwrap_err().code, "unauthorized");
        assert_eq!(service.replace_apply(ReplaceApplyRequest { plan_id: "wrong".into(), expected_revision: 0 }, true).unwrap_err().code, "conflict");
        service.replacement.lock().unwrap().as_mut().unwrap().created = Instant::now() - Duration::from_secs(301);
        assert_eq!(service.replace_apply(ReplaceApplyRequest { plan_id: "fixture-plan".into(), expected_revision: 0 }, true).unwrap_err().code, "conflict");
        let apply = fixture.plan(&service, &["first.txt"]);
        let result = service.replace_apply(apply, true).unwrap();
        assert!(result.failure.is_none()); assert_eq!(result.replacements, 1);
        assert_eq!(fs::read_to_string(fixture.0.join("first.txt")).unwrap(), "after");
        assert!(service.replace_apply(ReplaceApplyRequest { plan_id: "fixture-plan".into(), expected_revision: 0 }, true).is_err());
    }

    #[test]
    fn apply_rejects_paths_outside_active_workspace_and_after_workspace_close() {
        let fixture = Fixture::new(); let outside = Fixture::new();
        fs::write(fixture.0.join("first.txt"), "before").unwrap();
        fs::write(outside.0.join("outside.txt"), "before").unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone(), outside.0.clone()], Some(fixture.0.clone())).unwrap();
        let apply = fixture.plan(&service, &["first.txt"]);
        service.replacement.lock().unwrap().as_mut().unwrap().files[0].path = display_path(&outside.0.join("outside.txt"));
        assert_eq!(service.replace_apply(apply, true).unwrap_err().code, "outside_workspace");
        let apply = fixture.plan(&service, &["first.txt"]);
        service.execute(WorkspaceRequest { expected_revision: 0, discard_changes: false, operation: WorkspaceOperation::Close }, true).unwrap();
        assert_eq!(service.replace_apply(apply, true).unwrap_err().code, "conflict");
        assert_eq!(fs::read_to_string(fixture.0.join("first.txt")).unwrap(), "before");
    }

    #[test]
    #[ignore = "Requires bundled rg or PATH; run explicitly for search replacement acceptance"]
    fn preview_is_readonly_and_apply_replaces_only_content_and_keeps_file_name() {
        let fixture = Fixture::new();
        let path = fixture.0.join("needle-中文.txt");
        fs::write(&path, "\u{feff}needle needle\r\n").unwrap();
        let service = fixture.service();
        let plan = service.replace_preview(request("needle", "changed", false), true).unwrap();
        assert_eq!(plan.replacements, 2); assert_eq!(plan.files.len(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), "\u{feff}needle needle\r\n");
        let outcome = service.replace_apply(ReplaceApplyRequest { plan_id: plan.plan_id, expected_revision: plan.revision }, true).unwrap();
        assert!(outcome.failure.is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), "\u{feff}changed changed\r\n");
        assert!(!fixture.0.join("changed-中文.txt").exists());
    }
}
