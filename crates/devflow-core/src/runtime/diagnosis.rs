//! Bounded Agent-driven diagnosis. HTTP and MCP only adapt these operations.
use crate::{DevFlowEngine, workspace::{DetectedProject, WorkspaceError}};
use super::{diagnostics::redact, java::java_identity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, io::{Read, Write}, net::{SocketAddr, TcpStream}, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

type Result<T> = std::result::Result<T, WorkspaceError>;
fn error(code: &'static str, message: impl Into<String>) -> WorkspaceError { WorkspaceError { code, message: message.into() } }
fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub command: String,
    pub output: String,
    pub truncated: bool,
    pub timed_out: bool,
    pub recorded_at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub id: String,
    pub revision: u64,
    pub project_path: String,
    pub pid: u32,
    pub process_started: u64,
    pub run_id: Option<String>,
    pub problem: String,
    pub status: String,
    pub progress: String,
    pub conclusion: String,
    pub evidence: Vec<Evidence>,
    pub created_at: u64,
    pub updated_at: u64,
    pub agent_prompt: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Store { workspace: String, sessions: Vec<Diagnosis> }
#[derive(Default)]
pub struct DiagnosisService { jobs: Mutex<HashMap<String, Arc<AtomicBool>>> }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosisQuery { pub project_path: String, pub expected_revision: u64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosisRequest {
    pub project_path: String,
    pub expected_revision: u64,
    pub operation: DiagnosisOperation,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum DiagnosisOperation {
    Create { pid: u32, problem: String, run_id: Option<String> },
    Attach { id: String, revision: u64 },
    Observe { id: String, revision: u64, observation: Observation, seconds: u64 },
    Progress { id: String, revision: u64, text: String, waiting: bool },
    Finish { id: String, revision: u64, conclusion: String },
    Cancel { id: String, revision: u64 },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Observation {
    Jvm,
    Threads,
    Class { class_name: String },
    Methods { class_name: String, method_name: String },
    Trace { class_name: String, method_name: String },
    Stack { class_name: String, method_name: String },
    Watch { class_name: String, method_name: String },
}
fn identifier(value: &str, class: bool) -> Result<()> {
    let valid = !value.is_empty() && value.len() <= 240 && value.split('.').all(|part| {
        !part.is_empty() && part.chars().enumerate().all(|(i, c)| c == '_' || c == '$' || c.is_alphabetic() || (i > 0 && c.is_ascii_digit()))
    });
    if !valid || (!class && value.contains('.')) { return Err(error("invalid_request", "需提供精确 Java 类名和方法名，不支持通配符或表达式")); }
    if class && ["java.", "javax.", "jdk.", "sun.", "com.sun.", "com.taobao.arthas."].iter().any(|prefix| value.starts_with(prefix)) {
        return Err(error("invalid_request", "诊断仅允许应用类"));
    }
    Ok(())
}
impl Observation {
    fn command(&self) -> Result<String> {
        let (kind, class, method) = match self {
            Self::Jvm => return Ok("jvm".into()),
            Self::Threads => return Ok("thread -n 5 -i 100".into()),
            Self::Class { class_name } => { identifier(class_name, true)?; return Ok(format!("sc -d {class_name}")); },
            Self::Methods { class_name, method_name } => ("sm", class_name, method_name),
            Self::Trace { class_name, method_name } => ("trace", class_name, method_name),
            Self::Stack { class_name, method_name } => ("stack", class_name, method_name),
            Self::Watch { class_name, method_name } => ("watch", class_name, method_name),
        };
        identifier(class, true)?; identifier(method, false)?;
        Ok(match kind {
            "sm" => format!("sm -d {class} {method}"),
            // A fixed expression only; no arbitrary OGNL, shell, or class mutation.
            "watch" => format!("watch {class} {method} '{{params,returnObj,throwExp}}' -x 1 -n 3 -m 1"),
            _ => format!("{kind} {class} {method} -n 3 -m 1"),
        })
    }
}
fn text(value: &str, max: usize) -> Result<String> {
    if value.trim().is_empty() || value.chars().count() > max { return Err(error("invalid_request", format!("内容不能为空且不能超过 {max} 字符"))); }
    Ok(redact(value.trim()))
}
fn terminal(status: &str) -> bool { matches!(status, "completed" | "cancelled" | "failed") }
fn load(path: &Path, key: &str) -> Result<Store> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store { workspace: key.into(), sessions: Vec::new() }),
        Err(e) => return Err(e.into()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 16 * 1024 * 1024 => return Err(error("permission_denied", "诊断存储文件类型或大小无效")),
        _ => (),
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 { return Err(error("io_error", "诊断存储超过上限")); }
    let store: Store = serde_json::from_slice(&bytes).map_err(|e| error("io_error", format!("诊断记录损坏，原文件已保留：{e}")))?;
    if store.workspace != key { return Err(error("conflict", "诊断工作区不匹配")); }
    Ok(store)
}
fn save(path: &Path, store: &Store) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let outcome = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(&serde_json::to_vec(store).map_err(|e| error("io_error", e.to_string()))?)?;
        file.sync_all()?; drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if outcome.is_err() { let _ = fs::remove_file(temp); }
    outcome
}
fn recover(store: &mut Store, jobs: &HashMap<String, Arc<AtomicBool>>) {
    for session in &mut store.sessions {
        if matches!(session.status.as_str(), "observing" | "cancelling") && !jobs.contains_key(&session.id) {
            session.status = "failed".into();
            session.progress = "DevFlow 已重启，诊断采样已中断；原观测有时间上限，请新建诊断。".into();
            session.revision += 1; session.updated_at = now();
        }
    }
}

impl DevFlowEngine {
    fn diagnosis_scope(&self, project_path: &str, revision: u64, authenticated: bool) -> Result<(DetectedProject, String, PathBuf)> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let project = self.workspace.runtime_project(project_path, Some(revision))?;
        if matches!(project.kind, crate::workspace::ProjectKind::Node) { return Err(error("invalid_request", "Arthas 诊断仅支持 Java 服务")); }
        let snapshot = self.workspace.snapshot()?;
        let key = snapshot.workspace_file.or(snapshot.workspace_root).ok_or_else(|| error("outside_workspace", "请先打开工作区"))?;
        let path = self.workspace.diagnosis_location(&key, revision)?;
        Ok((project, key, path))
    }
    pub fn diagnosis_get(&self, query: DiagnosisQuery, authenticated: bool) -> Result<Vec<Diagnosis>> {
        let (project, key, path) = self.diagnosis_scope(&query.project_path, query.expected_revision, authenticated)?;
        let jobs = self.diagnosis.jobs.lock().map_err(|_| error("unavailable", "诊断暂不可用"))?;
        let mut store = load(&path, &key)?;
        let previous: Vec<_> = store.sessions.iter().map(|s| s.revision).collect();
        recover(&mut store, &jobs);
        if previous != store.sessions.iter().map(|s| s.revision).collect::<Vec<_>>() { save(&path, &store)?; }
        Ok(store.sessions.into_iter().filter(|s| s.project_path == project.path).collect())
    }
    pub fn diagnosis_execute(&self, request: DiagnosisRequest, authenticated: bool) -> Result<Diagnosis> {
        let _operation = self.runtime_operations.lock().map_err(|_| error("unavailable", "运行服务暂不可用"))?;
        let (project, key, path) = self.diagnosis_scope(&request.project_path, request.expected_revision, authenticated)?;
        let mut jobs = self.diagnosis.jobs.lock().map_err(|_| error("unavailable", "诊断暂不可用"))?;
        let mut store = load(&path, &key)?;
        recover(&mut store, &jobs);
        use DiagnosisOperation::*;
        if let Create { pid, problem, run_id } = &request.operation {
            let problem = text(problem, 2000)?;
            let target = self.diagnosis_target(&project, *pid)?;
            let output = self.java.output(&project, u64::MAX, None)?;
            if output.state != "external" && run_id.as_deref() != Some(output.run_id.as_str()) { return Err(error("conflict", "运行已切换，请刷新后创建诊断")); }
            if store.sessions.iter().any(|s| s.project_path == project.path && !terminal(&s.status)) { return Err(error("conflict", "该服务已有诊断，请继续或取消后再创建")); }
            while store.sessions.len() >= 20 {
                let index = store.sessions.iter().position(|s| terminal(&s.status)).ok_or_else(|| error("conflict", "工作区进行中的诊断已达 20 条"))?;
                store.sessions.remove(index);
            }
            let id = uuid::Uuid::new_v4().to_string();
            let session = Diagnosis {
                agent_prompt: format!("请通过 DevFlow MCP 接手诊断 {id}。\n项目：{}\n问题：{problem}\n按需挂载 Arthas、读取源码与日志，持续排查并回写进展、证据和结论；未经要求不要编译、重启或停止服务。", project.path),
                id, revision: 0, project_path: project.path, pid: *pid, process_started: target.1,
                run_id: (output.state != "external").then_some(output.run_id), problem,
                status: "awaiting_agent".into(), progress: "等待 Agent 接手".into(), conclusion: String::new(), evidence: Vec::new(), created_at: now(), updated_at: now(),
            };
            store.sessions.push(session.clone()); save(&path, &store)?;
            return Ok(session);
        }
        let (id, revision) = match &request.operation {
            Attach { id, revision } | Observe { id, revision, .. } | Progress { id, revision, .. } | Finish { id, revision, .. } | Cancel { id, revision } => (id, *revision),
            Create { .. } => unreachable!(),
        };
        let session = store.sessions.iter_mut().find(|s| &s.id == id && s.project_path == project.path).ok_or_else(|| error("not_found", "诊断不存在"))?;
        if session.revision != revision { return Err(error("conflict", "诊断已更新，请重新读取")); }
        if terminal(&session.status) { return Err(error("conflict", "诊断已结束，请新建诊断")); }
        let busy = jobs.contains_key(id);
        if busy && !matches!(&request.operation, Cancel { .. }) { return Err(error("conflict", "采样仍在执行，请读取结果或取消")); }
        let mut worker = None;
        match &request.operation {
            Attach { .. } | Observe { .. } => {
                let target = self.diagnosis_target(&project, session.pid)?;
                if target.1 != session.process_started { return Err(error("conflict", "目标 JVM 已变化，请新建诊断")); }
                let output = self.java.output(&project, u64::MAX, None)?;
                if session.run_id.as_ref().is_some_and(|id| id != &output.run_id) { return Err(error("conflict", "运行已切换，请新建诊断")); }
                match &request.operation {
                    Attach { .. } => {
                        if self.java.diagnosis_endpoint(&project, target).is_err() {
                            if output.attaching { return Err(error("conflict", "Arthas 正在挂载，请轮询 java_output")); }
                            self.java.attach(&project, target)?;
                        }
                        session.status = "diagnosing".into(); session.progress = "Agent 已接手，等待 Arthas 就绪".into();
                    }
                    Observe { observation, seconds, .. } => {
                        if !(1..=20).contains(seconds) { return Err(error("invalid_request", "采样时长须为 1–20 秒")); }
                        if session.evidence.len() >= 20 { return Err(error("conflict", "单次诊断最多保留 20 次采样，请整理结论或新建诊断")); }
                        let command = observation.command()?;
                        let endpoint = self.java.diagnosis_endpoint(&project, target)?;
                        session.status = "observing".into(); session.progress = format!("正在采样：{command}；需要时请触发问题复现");
                        worker = Some((endpoint, target, command, *seconds));
                    }
                    _ => unreachable!(),
                }
            }
            Progress { text: message, waiting, .. } => {
                session.progress = text(message, 2000)?;
                session.status = if *waiting { "waiting_reproduction" } else { "diagnosing" }.into();
            }
            Finish { conclusion, .. } => {
                session.conclusion = text(conclusion, 8000)?;
                session.status = "completed".into(); session.progress = "诊断完成".into();
            }
            Cancel { .. } => {
                session.status = if busy { "cancelling" } else { "cancelled" }.into();
                session.progress = if busy { "正在取消，采样将在本次时间上限内结束并清理" } else { "诊断已取消" }.into();
            }
            Create { .. } => unreachable!(),
        }
        session.revision += 1; session.updated_at = now();
        let result = session.clone(); save(&path, &store)?;
        if matches!(&request.operation, Cancel { .. }) {
            if let Some(cancel) = jobs.get(id) { cancel.store(true, Ordering::SeqCst); }
        }
        if let Some((endpoint, target, command, seconds)) = worker {
            let cancel = Arc::new(AtomicBool::new(false));
            jobs.insert(id.clone(), cancel.clone());
            let service = self.diagnosis.clone(); let id = id.clone();
            thread::spawn(move || service.collect(path, key, id, endpoint, target, command, seconds, cancel));
        }
        Ok(result)
    }
    fn diagnosis_target(&self, project: &DetectedProject, pid: u32) -> Result<(u32, u64)> {
        self.runtime.invalidate();
        if !self.runtime_snapshot()?.projects.iter().any(|p| p.project_path == project.path && p.java_pids.contains(&pid)) { return Err(error("conflict", "目标 Java 进程已退出或不属于该服务")); }
        java_identity(pid)
    }
}

impl DiagnosisService {
    fn collect(self: Arc<Self>, path: PathBuf, key: String, id: String, endpoint: String, target: (u32, u64), command: String, seconds: u64, cancel: Arc<AtomicBool>) {
        let result = collect(&endpoint, target, &command, seconds, &cancel);
        if let Ok(mut jobs) = self.jobs.lock() {
            let saved = (|| -> Result<()> {
                let mut store = load(&path, &key)?;
                let session = store.sessions.iter_mut().find(|s| s.id == id).ok_or_else(|| error("not_found", "诊断记录不存在"))?;
                let cancelled = cancel.load(Ordering::SeqCst);
                match result {
                    Ok((output, timed_out)) => {
                        let sanitized = redact(&output);
                        session.evidence.push(Evidence { command, truncated: sanitized.chars().count() > 24000, output: sanitized.chars().take(24000).collect(), timed_out, recorded_at: now() });
                        session.status = if cancelled { "cancelled" } else { "diagnosing" }.into();
                        session.progress = if cancelled { "诊断已取消，本次观测会话已清理" } else { "采样结束，等待 Agent 分析并选择下一步" }.into();
                    }
                    Err(err) => {
                        let output = redact(&err.message);
                        session.evidence.push(Evidence { command, truncated: output.chars().count() > 24000, output: output.chars().take(24000).collect(), timed_out: err.code == "timeout", recorded_at: now() });
                        session.status = if matches!(err.code, "cleanup_failed" | "conflict") { "failed" } else if cancelled { "cancelled" } else { "diagnosing" }.into();
                        session.progress = redact(&format!("本次采样未成功，Agent 可查看证据调整下一步：{}", err.message)).chars().take(2000).collect();
                    }
                }
                session.revision += 1; session.updated_at = now(); save(&path, &store)
            })();
            if let Err(err) = saved { eprintln!("诊断结果保存失败：{}", err.message); }
            jobs.remove(&id);
        }
    }
}

// Every observation owns a separate Arthas session; close only that session.
fn collect(endpoint: &str, target: (u32, u64), command: &str, seconds: u64, cancel: &AtomicBool) -> Result<(String, bool)> {
    let address = endpoint.strip_prefix("http://127.0.0.1:").and_then(|p| p.parse::<u16>().ok()).filter(|p| *p != 0).map(|p| SocketAddr::from(([127, 0, 0, 1], p))).ok_or_else(|| error("permission_denied", "Arthas 地址无效"))?;
    collect_with(command, seconds, cancel, |body, timeout| api(address, body, timeout), || {
        if java_identity(target.0).ok() != Some(target) { return Err(error("conflict", "目标 JVM 已变化")); } Ok(())
    })
}

fn collect_with(command: &str, seconds: u64, cancel: &AtomicBool, mut send: impl FnMut(Value, u64) -> Result<Value>, check: impl Fn() -> Result<()>) -> Result<(String, bool)> {
    check()?;
    let init = send(json!({"action":"init_session"}), 3)?;
    let session = init["sessionId"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| error("io_error", "Arthas 未返回会话标识"))?;
    let outcome = (|| -> Result<(String, bool)> {
        check()?;
        if cancel.load(Ordering::SeqCst) { return Ok(("采样开始前已取消".into(), false)); }
        let response = send(json!({"action":"exec", "sessionId":session, "command":command, "execTimeout":seconds * 1000}), seconds + 3)?;
        let output = response.get("body").unwrap_or(&response);
        if output["results"].as_array().is_some_and(|r| r.iter().any(|v| v["statusCode"].as_i64().is_some_and(|c| c != 0))) {
            return Err(error("io_error", format!("Arthas 命令失败：{output}")));
        }
        Ok((serde_json::to_string_pretty(output).unwrap_or_default(), output["timeExpired"].as_bool().unwrap_or(false)))
    })();
    // Do not send cleanup to a reused PID. The old JVM/session is already gone.
    if check().is_ok() {
        send(json!({"action":"close_session", "sessionId":session}), 3).map_err(|e| error("cleanup_failed", format!("观测会话清理未确认：{}", e.message)))?;
    }
    outcome
}

/// Tiny bounded HTTP client for an internally generated loopback address only.
/// HTTP/1.0 + Connection: close avoids persistent connections. Tolerate chunked replies.
fn api(address: SocketAddr, body: Value, seconds: u64) -> Result<Value> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let body = body.to_string();
    write!(stream, "POST /api HTTP/1.0\r\nHost: {address}\r\nContent-Type: application/json; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len())?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut data = Vec::new(); let mut buffer = [0u8; 8192];
    loop {
        if Instant::now() >= deadline { return Err(error("timeout", "Arthas 请求超时")); }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => { data.extend_from_slice(&buffer[..n]); if data.len() > 1024 * 1024 { return Err(error("io_error", "Arthas 响应超过 1 MiB 上限")); } },
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    parse_response(&data)
}
fn parse_response(data: &[u8]) -> Result<Value> {
    let split = data.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| error("io_error", "Arthas HTTP 响应无效"))?;
    let header = String::from_utf8_lossy(&data[..split]);
    if !header.lines().next().is_some_and(|line| line.split_whitespace().nth(1) == Some("200")) { return Err(error("io_error", "Arthas HTTP 请求失败")); }
    let raw = &data[split+4..];
    let mut decoded = Vec::new();
    let chunked = header.lines().any(|line| line.split_once(':').is_some_and(|(name, value)| name.eq_ignore_ascii_case("transfer-encoding") && value.trim().eq_ignore_ascii_case("chunked")));
    let body = if chunked {
        let mut remaining = raw;
        loop {
            let end = remaining.windows(2).position(|w| w == b"\r\n").ok_or_else(|| error("io_error", "Arthas 分块响应不完整"))?;
            let length = std::str::from_utf8(&remaining[..end]).ok().and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or(""), 16).ok()).filter(|n| *n <= 1024 * 1024).ok_or_else(|| error("io_error", "Arthas 分块长度无效"))?;
            remaining = &remaining[end+2..];
            if length == 0 { if !remaining.starts_with(b"\r\n") { return Err(error("io_error", "Arthas 分块结尾无效")); } break; }
            if remaining.len() < length + 2 || &remaining[length..length+2] != b"\r\n" { return Err(error("io_error", "Arthas 分块响应不完整")); }
            decoded.extend_from_slice(&remaining[..length]); remaining = &remaining[length+2..];
        }
        decoded.as_slice()
    } else {
        if let Some(length) = header.lines().find_map(|line| line.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("content-length")).map(|(_, value)| value.trim())) {
            if length.parse::<usize>().ok() != Some(raw.len()) { return Err(error("io_error", "Arthas 响应长度不匹配")); }
        }
        raw
    };
    let result: Value = serde_json::from_slice(body).map_err(|e| error("io_error", format!("Arthas JSON 响应无效：{e}")))?;
    if result["state"] != "SUCCEEDED" { return Err(error("io_error", format!("Arthas 拒绝请求：{}", redact(&result.to_string())))); }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn sample() -> Diagnosis {
        Diagnosis { id: "session-one".into(), revision: 0, project_path: "D:/中文 空格/服务".into(), pid: 123, process_started: 100,
            run_id: Some("run-one".into()), problem: "接口偶发超时".into(), status: "diagnosing".into(), progress: "读取源码".into(), conclusion: String::new(),
            evidence: Vec::new(), created_at: 1, updated_at: 1, agent_prompt: "接手诊断".into() }
    }
    #[test]
    fn observation_rejects_injection_wildcards_and_system_classes() {
        for class in ["*", "a.Foo;stop", "a.Foo | cat", "a.Foo\nstop", "java.lang.Runtime", "a..Foo", "1Foo"] {
            assert!(Observation::Trace { class_name: class.into(), method_name: "run".into() }.command().is_err());
        }
        for method in ["*", "run;stop", "run -n 9999", "@java.lang.Runtime@getRuntime()", "a.b"] {
            assert!(Observation::Watch { class_name: "com.example.中文服务$Inner".into(), method_name: method.into() }.command().is_err());
        }
        let command = Observation::Trace { class_name: "com.example.中文服务$Inner".into(), method_name: "query".into() }.command().unwrap();
        assert_eq!(command, "trace com.example.中文服务$Inner query -n 3 -m 1");
        assert!(serde_json::from_value::<Observation>(json!({"kind":"jvm","command":"stop"})).is_err());
    }
    #[test]
    fn closes_only_owned_session_on_timeout_failure_and_cancellation() {
        for mode in ["timeout", "failure", "cancel"] {
            let mut requests = Vec::new();
            let cancel = AtomicBool::new(mode == "cancel");
            let result = collect_with("trace a.Foo run -n 3 -m 1", 2, &cancel, |request, _| {
                requests.push(request.clone());
                match request["action"].as_str().unwrap() {
                    "init_session" => Ok(json!({"sessionId":"owned"})),
                    "exec" if mode == "failure" => Err(error("io_error", "模拟连接失败")),
                    "exec" => { assert_eq!(request["execTimeout"], 2000); Ok(json!({"body":{"timeExpired":true,"results":[]}})) },
                    "close_session" => Ok(json!({})),
                    _ => unreachable!(),
                }
            }, || Ok(()));
            assert_eq!(requests.last().unwrap(), &json!({"action":"close_session","sessionId":"owned"}));
            if mode == "failure" { assert!(result.is_err()); } else { assert!(result.is_ok()); }
            assert_eq!(requests.len(), if mode == "cancel" { 2 } else { 3 });
        }
    }
    #[test]
    fn cleanup_failure_is_visible_and_pid_reuse_blocks_execution() {
        let cancelled = AtomicBool::new(false);
        let result = collect_with("jvm", 1, &cancelled, |request, _| {
            if request["action"] == "init_session" { Ok(json!({"sessionId":"owned"})) }
            else if request["action"] == "close_session" { Err(error("io_error", "清理失败")) }
            else { Ok(json!({"body":{"results":[]}})) }
        }, || Ok(()));
        assert_eq!(result.unwrap_err().code, "cleanup_failed");
        let calls = Cell::new(0);
        let result = collect_with("jvm", 1, &cancelled, |_, _| { calls.set(calls.get()+1); Ok(json!({})) }, || Err(error("conflict", "PID 已复用")));
        assert!(result.is_err()); assert_eq!(calls.get(), 0);
    }
    #[test]
    fn persisted_chinese_windows_identity_recovers_interrupted_sampling() {
        let directory = std::env::temp_dir().join(format!("devflow-诊断-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap(); let path = directory.join("diagnosis.json");
        let mut session = sample(); session.status = "observing".into();
        let store = Store { workspace: "D:/中文 空格/工作区.code-workspace".into(), sessions: vec![session] };
        save(&path, &store).unwrap(); save(&path, &store).unwrap();
        assert!(load(&path, "D:/其他工作区").is_err());
        let mut restored = load(&path, &store.workspace).unwrap();
        let mut jobs = HashMap::new(); jobs.insert("session-one".into(), Arc::new(AtomicBool::new(false)));
        recover(&mut restored, &jobs); assert_eq!(restored.sessions[0].status, "observing");
        recover(&mut restored, &HashMap::new());
        assert_eq!(restored.sessions[0].status, "failed"); assert_eq!(restored.sessions[0].revision, 1);
        assert_eq!(restored.sessions[0].project_path, "D:/中文 空格/服务");
        fs::remove_file(&path).unwrap(); fs::remove_dir(&directory).unwrap();
    }
    #[test]
    fn core_enforces_auth_workspace_and_session_revisions() {
        let root = std::env::temp_dir().join(format!("devflow-diagnosis-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pom.xml"), "<project><modelVersion>4.0.0</modelVersion><groupId>example</groupId><artifactId>demo</artifactId><version>1</version></project>").unwrap();
        let engine = DevFlowEngine::new(vec![root.clone()], Some(root.clone())).unwrap();
        engine.workspace.configure_recent(root.join(".devflow/recent.json"), None, false).unwrap();
        let snapshot = engine.workspace.snapshot().unwrap();
        let project = snapshot.projects.first().unwrap();
        let query = || DiagnosisQuery { project_path: project.path.clone(), expected_revision: snapshot.revision };
        assert_eq!(engine.diagnosis_get(query(), false).unwrap_err().code, "unauthorized");
        assert!(engine.diagnosis_get(DiagnosisQuery { expected_revision: snapshot.revision+1, ..query() }, true).is_err());
        assert!(engine.diagnosis_get(DiagnosisQuery { project_path: root.join("outside").to_string_lossy().into(), ..query() }, true).is_err());
        let (_, key, path) = engine.diagnosis_scope(&project.path, snapshot.revision, true).unwrap();
        let mut session = sample(); session.project_path = project.path.clone();
        save(&path, &Store { workspace: key, sessions: vec![session] }).unwrap();
        let progress = || DiagnosisRequest { project_path: project.path.clone(), expected_revision: snapshot.revision, operation: DiagnosisOperation::Progress { id: "session-one".into(), revision: 0, text: "等待复现".into(), waiting: true } };
        assert_eq!(engine.diagnosis_execute(progress(), true).unwrap().revision, 1);
        assert_eq!(engine.diagnosis_execute(progress(), true).unwrap_err().code, "conflict");
        let invalid_pid = DiagnosisRequest { project_path: project.path.clone(), expected_revision: snapshot.revision, operation: DiagnosisOperation::Create { pid: std::process::id(), problem: "定位问题".into(), run_id: None } };
        assert!(engine.diagnosis_execute(invalid_pid, true).is_err());
        drop(engine);
        fs::remove_file(&path).unwrap(); fs::remove_dir(root.join(".devflow/boards")).unwrap(); fs::remove_dir(root.join(".devflow")).unwrap();
        fs::remove_file(root.join("pom.xml")).unwrap(); fs::remove_dir(&root).unwrap();
    }
    #[test]
    fn http_handles_bounded_json_chunks_and_truncated_responses() {
        let body = r#"{"state":"SUCCEEDED","body":{"message":"中文"}}"#;
        let response = format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        assert_eq!(parse_response(response.as_bytes()).unwrap()["body"]["message"], "中文");
        let response = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len());
        assert!(parse_response(response.as_bytes()).is_ok());
        assert!(parse_response(b"HTTP/1.0 200 OK\r\nContent-Length: 100\r\n\r\n{}").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nfffffff\r\n{}").is_err());
    }
}
