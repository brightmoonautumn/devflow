//! Workspace-scoped, bounded HTTP scenarios shared by Web and MCP.
use crate::{DevFlowEngine, workspace::WorkspaceError};
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, WorkspaceError>;
const LIMIT: u64 = 1024 * 1024;
mod sse;
fn error(code: &'static str, text: impl Into<String>) -> WorkspaceError {
    WorkspaceError {
        code,
        message: text.into(),
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Suite {
    pub name: String,
    pub base_url: String,
    pub site_url: String,
    pub public_key: String,
    pub actors: BTreeMap<String, Actor>,
    pub variables: BTreeMap<String, Value>,
    pub scenarios: Vec<Scenario>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environments: Vec<Environment>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub base_url: String,
}
impl Suite {
    fn environment(&self, id: Option<&str>) -> Result<Environment> {
        if self.environments.is_empty() && id.is_none_or(|id| id == "default") {
            return Ok(Environment { id: "default".into(), name: self.name.clone(), base_url: self.base_url.clone() });
        }
        let environment = match id {
            Some(id) => self.environments.iter().find(|item| item.id == id),
            None => self.environments.first(),
        };
        environment.cloned().ok_or_else(|| error("invalid_request", "QA 环境不存在"))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub label: String,
    pub token: Option<String>,
    #[serde(default)]
    pub prefix: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub module: String,
    pub title: String,
    pub purpose: String,
    #[serde(default)]
    pub uses: Vec<String>,
    pub actions: Vec<Action>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub description: String,
    pub calls: Vec<Call>,
    #[serde(default)]
    pub cleanup: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub method: String,
    pub path: String,
    pub actor: String,
    pub body: Value,
    #[serde(default)]
    pub extracts: BTreeMap<String, String>,
    #[serde(default)]
    pub assertions: Vec<Assertion>,
    #[serde(default)]
    pub encrypt: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sse: Option<SseOptions>,
    #[serde(skip)]
    cleanup: bool,
    #[serde(skip)]
    required_variables: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SseOptions {
    pub end_event: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub path: String,
    pub op: String,
    #[serde(default)]
    pub value: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub call_id: String,
    pub scenario_id: String,
    pub name: String,
    pub actor: String,
    pub status: String,
    pub http_status: Option<u16>,
    pub elapsed_ms: u64,
    pub request: Value,
    pub response: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_raw: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stream_events: Vec<Value>,
    pub checks: Vec<Check>,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub scenario_id: String,
    pub suite_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub account_name: String,
    pub current_call: Option<String>,
    pub results: Vec<StepResult>,
    pub variables: BTreeMap<String, Value>,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub workspace_key: String,
    pub revision: u64,
    pub suite: Option<Suite>,
    pub runs: Vec<Run>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub workspace_key: String,
    pub expected_revision: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub workspace_key: String,
    pub expected_revision: u64,
    pub qa_revision: u64,
    pub operation: Operation,
}
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Operation {
    Seed,
    Save {
        suite: Suite,
    },
    Run {
        scenario_id: String,
        #[serde(default)]
        environment_id: Option<String>,
        #[serde(default)]
        inputs: BTreeMap<String, Value>,
    },
    Cancel {
        run_id: String,
    },
    ClearHistory {
        run_ids: Vec<String>,
    },
    DeleteScenario { scenario_id: String },
    DeleteModule { module: String },
}
pub struct QaService {
    jobs: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
    changes: tokio::sync::broadcast::Sender<String>,
    live_requests: Mutex<BTreeMap<String, (String, BTreeMap<String, (Value, Option<String>)>)>>,
}
impl Default for QaService {
    fn default() -> Self {
        let (changes, _) = tokio::sync::broadcast::channel(128);
        Self { jobs: Mutex::new(BTreeMap::new()), changes, live_requests: Mutex::new(BTreeMap::new()) }
    }
}
impl DevFlowEngine {
    pub fn qa_subscribe(&self, query: Query, authenticated: bool) -> Result<tokio::sync::broadcast::Receiver<String>> {
        if !authenticated { return Err(error("unauthorized", "需要本机管理令牌")); }
        self.workspace.qa_location(&query.workspace_key, query.expected_revision)?;
        Ok(self.qa.changes.subscribe())
    }
    pub fn qa_get(&self, query: Query, authenticated: bool) -> Result<Snapshot> {
        if !authenticated {
            return Err(error("unauthorized", "需要本机管理令牌"));
        }
        let path = self
            .workspace
            .qa_location(&query.workspace_key, query.expected_revision)?;
        let jobs = self
            .qa
            .jobs
            .lock()
            .map_err(|_| error("unavailable", "QA 服务不可用"))?;
        let mut store = load(&path, &query.workspace_key)?;
        if store
            .runs
            .iter()
            .any(|r| matches!(r.status.as_str(), "running" | "cancelling"))
            && !jobs.contains_key(&query.workspace_key)
        {
            if let Ok(_lock) = storage_lock(&path) {
                for run in &mut store.runs {
                    if matches!(run.status.as_str(), "running" | "cancelling") {
                        run.status = "interrupted".into();
                        for step in &mut run.results {
                            if step.status == "streaming" {
                                step.status = "interrupted".into();
                            }
                        }
                        run.current_call = None;
                        run.error = Some("DevFlow 重启，执行已中断；不会重放请求".into());
                        run.updated_at = now();
                    }
                }
                save(&path, &store)?;
            }
        }
        if store.runs.len() > 10 && !jobs.contains_key(&query.workspace_key) {
            if let Ok(_lock) = storage_lock(&path) {
                store = load(&path, &query.workspace_key)?;
                if store.runs.len() > 10 {
                    store.runs.truncate(10);
                    save(&path, &store)?;
                }
            }
        }
        if let Ok(requests) = self.qa.live_requests.lock() {
            if let Some((run_id, calls)) = requests.get(&query.workspace_key) {
                if let Some(run) = store.runs.iter_mut().find(|run| &run.id == run_id) {
                    for result in &mut run.results {
                        if let Some((request, response)) = calls.get(&result.call_id) {
                            result.request = request.clone();
                            result.response_raw = response.clone();
                        }
                    }
                }
            }
        }
        Ok(store)
    }
    pub fn qa_execute(&self, request: Request, authenticated: bool) -> Result<Snapshot> {
        if !authenticated {
            return Err(error("unauthorized", "需要本机管理令牌"));
        }
        let path = self
            .workspace
            .qa_location(&request.workspace_key, request.expected_revision)?;
        let mut jobs = self
            .qa
            .jobs
            .lock()
            .map_err(|_| error("unavailable", "QA 服务不可用"))?;
        let mut store = load(&path, &request.workspace_key)?;
        if store.revision != request.qa_revision {
            return Err(error("conflict", "QA 配置已变化，请重新读取"));
        }
        if let Operation::Cancel { run_id } = &request.operation {
            let cancel = jobs
                .get(&request.workspace_key)
                .ok_or_else(|| error("conflict", "没有正在执行的场景"))?;
            let run = store
                .runs
                .iter_mut()
                .find(|r| r.id == *run_id && matches!(r.status.as_str(), "running" | "cancelling"))
                .ok_or_else(|| error("conflict", "执行已结束或标识不匹配"))?;
            cancel.store(true, Ordering::SeqCst);
            run.status = "cancelling".into();
            save(&path, &store)?;
            let _ = self.qa.changes.send(request.workspace_key.clone());
            return Ok(store);
        }
        if jobs.contains_key(&request.workspace_key) {
            return Err(error("conflict", "请等待当前场景完成或取消"));
        }
        let lock = storage_lock(&path)?;
        match request.operation {
            Operation::Seed => {
                if store.suite.is_some() {
                    return Err(error("conflict", "已有场景，不会覆盖"));
                }
                store.suite = Some(
                    serde_json::from_str(include_str!("extreme.json"))
                        .map_err(|_| error("invalid_request", "内置场景无效"))?,
                );
                validate(store.suite.as_ref().unwrap())?;
                store.revision += 1;
                save(&path, &store)?;
            }
            Operation::Save { suite } => {
                validate(&suite)?;
                store.suite = Some(suite);
                store.revision += 1;
                save(&path, &store)?;
            }
            Operation::Run {
                scenario_id,
                environment_id,
                inputs,
            } => {
                let suite = store
                    .suite
                    .clone()
                    .ok_or_else(|| error("not_found", "请先初始化场景"))?;
                validate(&suite)?;
                let environment = suite.environment(environment_id.as_deref())?;
                let plan = execution_plan(&suite, &scenario_id)?;
                if inputs.len() > 32
                    || inputs
                        .values()
                        .any(|v| !v.is_string() || v.as_str().unwrap().len() > 4096)
                {
                    return Err(error("invalid_request", "输入需为不超过 32 项的短字符串"));
                }
                let id = uuid::Uuid::new_v4().simple().to_string();
                let account_name = format!("qa_df_{}", &id[..16]);
                let mut vars = suite.variables.clone();
                vars.extend(inputs);
                vars.insert("runId".into(), json!(&id[..16]));
                vars.insert("accountName".into(), json!(account_name));
                if plan.iter().any(|(_, call)| call.id == "admin-login-call") && !vars.contains_key("adminPassword") {
                    return Err(error("invalid_request", "请提供管理员密码，仅用于本轮执行"));
                }
                let run = Run {
                    id: id.clone(),
                    scenario_id,
                    suite_revision: store.revision,
                    environment: Some(environment.clone()),
                    status: "running".into(),
                    created_at: now(),
                    updated_at: now(),
                    account_name,
                    current_call: None,
                    results: vec![],
                    variables: BTreeMap::new(),
                    error: None,
                };
                store.runs.insert(0, run);
                store.runs.truncate(10);
                save(&path, &store)?;
                let _ = self.qa.changes.send(request.workspace_key.clone());
                if let Ok(mut requests) = self.qa.live_requests.lock() {
                    if requests.len() >= 4 && !requests.contains_key(&request.workspace_key) {
                        if let Some(key) = requests.keys().next().cloned() { requests.remove(&key); }
                    }
                    requests.insert(request.workspace_key.clone(), (id.clone(), BTreeMap::new()));
                }
                let initial = store.clone();
                let cancel = Arc::new(AtomicBool::new(false));
                jobs.insert(request.workspace_key.clone(), cancel.clone());
                let service = self.qa.clone();
                std::thread::spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        execute(
                            &service, &path, &mut store, &suite, &environment.base_url, plan, &mut vars, &cancel,
                        )
                    }));
                    match result {
                        Err(_) => {
                            store.runs[0].status = "interrupted".into();
                            store.runs[0].error = Some("执行器异常，未重放请求".into());
                        }
                        Ok(Err(err)) => {
                            store.runs[0].status = "failed".into();
                            store.runs[0].error = Some(err.message);
                        }
                        _ => (),
                    }
                    if let Ok(mut jobs) = service.jobs.lock() {
                        store.runs[0].current_call = None;
                        store.runs[0].updated_at = now();
                        if save(&path, &store).is_ok() { let _ = service.changes.send(store.workspace_key.clone()); }
                        jobs.remove(&store.workspace_key);
                    }
                    drop(lock);
                });
                return Ok(initial);
            }
            Operation::DeleteScenario { scenario_id } => {
                delete_scenarios(&mut store, Some(&scenario_id), None)?;
                save(&path, &store)?;
                if let Ok(mut requests) = self.qa.live_requests.lock() { requests.remove(&request.workspace_key); }
            }
            Operation::DeleteModule { module } => {
                delete_scenarios(&mut store, None, Some(&module))?;
                save(&path, &store)?;
                if let Ok(mut requests) = self.qa.live_requests.lock() { requests.remove(&request.workspace_key); }
            }
            Operation::ClearHistory { run_ids } => {
                let count = run_ids.len();
                let ids: HashSet<String> = run_ids.into_iter().collect();
                if ids.is_empty() || ids.len() != count || ids.len() > 20
                    || ids.iter().any(|id| !store.runs.iter().any(|run| &run.id == id)) {
                    return Err(error("conflict", "执行记录已变化，请刷新后重新选择"));
                }
                if store.runs.iter().any(|run| ids.contains(&run.id) && matches!(run.status.as_str(), "running" | "cancelling")) {
                    return Err(error("conflict", "无法清除正在执行的记录"));
                }
                store.runs.retain(|run| !ids.contains(&run.id));
                save(&path, &store)?;
                if let Ok(mut requests) = self.qa.live_requests.lock() {
                    if requests.get(&request.workspace_key).is_some_and(|(run_id, _)| ids.contains(run_id)) {
                        requests.remove(&request.workspace_key);
                    }
                }
            }
            Operation::Cancel { .. } => unreachable!(),
        }
        let _ = self.qa.changes.send(request.workspace_key.clone());
        Ok(store)
    }
}
fn delete_scenarios(store: &mut Snapshot, scenario_id: Option<&str>, module: Option<&str>) -> Result<()> {
    let suite = store.suite.as_mut().ok_or_else(|| error("not_found", "场景配置不存在"))?;
    let mut removed: HashSet<String> = suite.scenarios.iter()
        .filter(|scenario| scenario_id.is_some_and(|id| scenario.id == id) || module.is_some_and(|name| scenario.module == name))
        .map(|scenario| scenario.id.clone()).collect();
    if removed.is_empty() { return Err(error("not_found", "场景或文件夹不存在")); }
    loop {
        let previous = removed.len();
        for scenario in &suite.scenarios {
            if scenario.uses.iter().any(|id| removed.contains(id)) { removed.insert(scenario.id.clone()); }
        }
        if removed.len() == previous { break; }
    }
    suite.scenarios.retain(|scenario| !removed.contains(&scenario.id));
    store.runs.retain(|run| !removed.contains(&run.scenario_id)
        && !run.results.iter().any(|result| removed.contains(&result.scenario_id)));
    store.revision = store.revision.checked_add(1).ok_or_else(|| error("conflict", "QA 版本已达上限"))?;
    Ok(())
}
fn storage_lock(path: &Path) -> Result<fs::File> {
    let path = path.with_extension("qa.lock");
    valid_file(&path)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| error("io_error", "无法打开 QA 存储锁"))?;
    file.try_lock()
        .map_err(|_| error("conflict", "另一执行器正在使用 QA 数据"))?;
    Ok(file)
}
fn valid_file(path: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16 * LIMIT {
            return Err(error("permission_denied", "QA 存储类型或大小无效"));
        }
    }
    Ok(())
}
fn load(path: &Path, key: &str) -> Result<Snapshot> {
    valid_file(path)?;
    if !path.exists() {
        return Ok(Snapshot {
            workspace_key: key.into(),
            revision: 0,
            suite: None,
            runs: vec![],
        });
    }
    let bytes = fs::read(path).map_err(|_| error("io_error", "无法读取 QA 存储"))?;
    let store: Snapshot = serde_json::from_slice(&bytes)
        .map_err(|_| error("io_error", "QA 存储损坏，原文件已保留"))?;
    if store.workspace_key != key {
        return Err(error("conflict", "QA 工作区标识不匹配"));
    }
    Ok(store)
}
fn save(path: &Path, store: &Snapshot) -> Result<()> {
    valid_file(path)?;
    let bytes =
        serde_json::to_vec_pretty(store).map_err(|_| error("io_error", "无法编码 QA 数据"))?;
    if bytes.len() as u64 > 16 * LIMIT {
        return Err(error("invalid_request", "QA 历史超过 16 MiB"));
    }
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result.map_err(|_| error("io_error", "QA 数据保存失败"))
}
fn endpoint(base: &str, path: &str) -> Result<url::Url> {
    let base = url::Url::parse(base).map_err(|_| error("invalid_request", "环境地址无效"))?;
    let origin = base.origin().ascii_serialization();
    let allowed = match std::env::var("DEVFLOW_QA_ORIGINS") {
        Ok(origins) => origins.split(',').any(|item| item.trim() == origin),
        Err(std::env::VarError::NotPresent) => origin == "http://192.168.1.200" ||
            matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        Err(_) => false,
    };
    if !matches!(base.scheme(), "http" | "https")
        || !base.username().is_empty()
        || base.password().is_some()
        || !allowed
    {
        return Err(error(
            "permission_denied",
            "QA 目标不在 DEVFLOW_QA_ORIGINS 允许列表中",
        ));
    }
    if !base.path().ends_with('/')
        || base.query().is_some()
        || base.fragment().is_some()
        || path.starts_with('/')
        || path.contains(['\\', '#'])
        || path
            .split('?')
            .next()
            .unwrap_or("")
            .split('/')
            .any(|s| s == ".." || s == ".")
    {
        return Err(error("invalid_request", "请求必须使用环境路径下的相对地址"));
    }
    let url = base
        .join(path)
        .map_err(|_| error("invalid_request", "请求路径无效"))?;
    if url.origin() != base.origin()
        || !url.path().starts_with(base.path())
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(error("permission_denied", "请求越过环境边界"));
    }
    Ok(url)
}
pub fn execution_plan(suite: &Suite, id: &str) -> Result<Vec<(String, Call)>> {
    fn visit(
        suite: &Suite,
        id: &str,
        active: &mut HashSet<String>,
        seen: &mut HashSet<String>,
        result: &mut Vec<(String, Call)>,
    ) -> Result<()> {
        if active.contains(id) {
            return Err(error("invalid_request", "场景引用存在循环"));
        }
        if seen.contains(id) {
            return Ok(());
        }
        let item = suite
            .scenarios
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| error("not_found", "引用场景不存在"))?;
        active.insert(id.into());
        for dependency in &item.uses {
            visit(suite, dependency, active, seen, result)?;
        }
        active.remove(id);
        seen.insert(id.into());
        for action in &item.actions {
            for call in &action.calls {
                let mut call = call.clone();
                call.cleanup = action.cleanup;
                call.required_variables = action.requires.clone();
                result.push((id.into(), call));
            }
        }
        if result.len() > 100 {
            return Err(error("invalid_request", "单轮最多 100 个请求"));
        }
        Ok(())
    }
    let mut plan = vec![];
    visit(
        suite,
        id,
        &mut HashSet::new(),
        &mut HashSet::new(),
        &mut plan,
    )?;
    // Run normal calls in dependency order, then release each scenario's resources
    // in reverse dependency order. Calls within one cleanup keep their order.
    let (mut normal, cleanup): (Vec<_>, Vec<_>) = plan
        .into_iter()
        .partition(|(_, call)| !call.cleanup);
    let mut groups: Vec<Vec<(String, Call)>> = Vec::new();
    for item in cleanup {
        if groups.last().is_none_or(|group| group[0].0 != item.0) {
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().push(item);
    }
    normal.extend(groups.into_iter().rev().flatten());
    Ok(normal)
}
fn validate(suite: &Suite) -> Result<()> {
    let environments = if suite.environments.is_empty() {
        vec![suite.environment(None)?]
    } else {
        suite.environments.clone()
    };
    let mut environment_ids = HashSet::new();
    if environments.len() > 20 {
        return Err(error("invalid_request", "QA 环境最多 20 项"));
    }
    for environment in &environments {
        if environment.name.trim().is_empty() || environment.name.len() > 120 ||
            environment.id.is_empty() || environment.id.len() > 64 ||
            !environment_ids.insert(environment.id.as_str()) || environment.base_url.len() > 1024 {
            return Err(error("invalid_request", "QA 环境标识、名称或地址无效"));
        }
        endpoint(&environment.base_url, "info")?;
    }
    if serde_json::to_vec(suite).unwrap_or_default().len() > 256 * 1024
        || suite.scenarios.len() > 50
    {
        return Err(error("invalid_request", "场景最多 50 项且不超过 256 KiB"));
    }
    if suite.variables.keys().any(|key| sensitive(key)) {
        return Err(error(
            "invalid_request",
            "密码与 token 必须通过本轮 inputs 或响应提取，不得存入场景变量",
        ));
    }
    fn check_body(value: &Value) -> bool {
        match value {
            Value::Object(o) => o.iter().all(|(key, v)| {
                if sensitive(key) {
                    v.as_str()
                        .is_some_and(|s| s.starts_with("{{") && s.ends_with("}}"))
                } else {
                    check_body(v)
                }
            }),
            Value::Array(a) => a.iter().all(check_body),
            _ => true,
        }
    }
    let mut ids = HashSet::new();
    for scenario in &suite.scenarios {
        if !ids.insert(format!("scenario:{}", scenario.id)) || scenario.actions.is_empty() {
            return Err(error("invalid_request", "场景标识重复或行为为空"));
        }
        for action in &scenario.actions {
            if !ids.insert(format!("action:{}", action.id)) || action.calls.is_empty() {
                return Err(error("invalid_request", "行为标识重复或请求为空"));
            }
            if (!action.cleanup && !action.requires.is_empty())
                || action.requires.len() > 32
                || action.requires.iter().any(|key| {
                    key.is_empty()
                        || key.len() > 128
                        || !key.bytes().enumerate().all(|(index, ch)| {
                            ch.is_ascii_alphabetic()
                                || (index > 0 && (ch.is_ascii_digit() || ch == b'_'))
                        })
                })
            {
                return Err(error("invalid_request", "清理前置变量配置无效"));
            }
            for call in &action.calls {
                if !ids.insert(format!("call:{}", call.id))
                    || !suite.actors.contains_key(&call.actor)
                    || !matches!(
                        call.method.as_str(),
                        "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
                    )
                {
                    return Err(error("invalid_request", "请求标识、身份或方法无效"));
                }
                for environment in &environments {
                    endpoint(&environment.base_url, &call.path)?;
                }
                if let Some(sse) = &call.sse {
                    if sse.end_event.is_empty() || sse.end_event.len() > 128 ||
                        !sse.end_event.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)) {
                        return Err(error("invalid_request", "SSE 结束事件名无效"));
                    }
                }
                if !check_body(&call.body) {
                    return Err(error(
                        "invalid_request",
                        "请求中的密码与 token 必须引用临时变量，不得保存字面凭证",
                    ));
                }
                if call.assertions.len() > 32
                    || call.extracts.len() > 32
                    || call.assertions.iter().any(|a| {
                        !matches!(
                            a.op.as_str(),
                            "equals" | "not_equals" | "nonempty" | "contains"
                        )
                    })
                {
                    return Err(error("invalid_request", "断言或提取配置无效"));
                }
            }
        }
        execution_plan(suite, &scenario.id)?;
    }
    Ok(())
}
fn render(value: &Value, vars: &BTreeMap<String, Value>, url_encode: bool) -> Result<Value> {
    Ok(match value {
        Value::String(s) => {
            let re = regex::Regex::new(r"\{\{([a-zA-Z][a-zA-Z0-9_]*)\}\}").unwrap();
            if !url_encode {
                if let Some(c) = re.captures(s) {
                    if c.get(0).unwrap().as_str() == s {
                        return vars.get(&c[1]).cloned().ok_or_else(|| {
                            error("invalid_request", format!("缺少变量 {}", &c[1]))
                        });
                    }
                }
            }
            let mut text = s.clone();
            for c in re.captures_iter(s) {
                let name = &c[1];
                let v = vars
                    .get(name)
                    .ok_or_else(|| error("invalid_request", format!("缺少变量 {name}")))?;
                let v = v
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string());
                let v = if url_encode {
                    url::form_urlencoded::byte_serialize(v.as_bytes()).collect()
                } else {
                    v
                };
                text = text.replace(&c[0], &v);
            }
            Value::String(text)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| render(v, vars, url_encode))
                .collect::<Result<_>>()?,
        ),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(k, v)| Ok((k.clone(), render(v, vars, url_encode)?)))
                .collect::<Result<_>>()?,
        ),
        v => v.clone(),
    })
}
fn sensitive(key: &str) -> bool {
    let key = key.to_lowercase();
    ["password", "token", "secret", "authorization", "cookie"]
        .iter()
        .any(|s| key.contains(s))
}
fn redact(value: &Value, secrets: &[String]) -> Value {
    match value {
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        if sensitive(k) {
                            json!("[REDACTED]")
                        } else {
                            redact(v, secrets)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| redact(v, secrets)).collect()),
        Value::String(s) => {
            let mut s = s.clone();
            for secret in secrets {
                if !secret.is_empty() {
                    s = s.replace(secret, "[REDACTED]");
                }
            }
            json!(s)
        }
        Value::Number(n) if n.as_u64().is_some_and(|v| v > 9_007_199_254_740_991) => {
            json!(n.to_string())
        }
        v => v.clone(),
    }
}
fn assertion(value: &Value, check: &Assertion, vars: &BTreeMap<String, Value>) -> Result<bool> {
    let actual = value.pointer(&check.path);
    let expected = render(&check.value, vars, false)?;
    Ok(match check.op.as_str() {
        "equals" => actual == Some(&expected),
        "not_equals" => actual.is_some() && actual != Some(&expected),
        "nonempty" => actual.is_some_and(|v| {
            !v.is_null()
                && v.as_str().is_none_or(|s| !s.is_empty())
                && v.as_array().is_none_or(|a| !a.is_empty())
        }),
        "contains" => actual.is_some_and(|v| {
            v.as_array().is_some_and(|a| a.contains(&expected))
                || v.as_str()
                    .zip(expected.as_str())
                    .is_some_and(|(a, b)| a.contains(b))
        }),
        _ => false,
    })
}
fn publish(
    service: &QaService,
    path: &Path,
    store: &mut Snapshot,
    vars: &BTreeMap<String, Value>,
) -> Result<()> {
    let jobs = service
        .jobs
        .lock()
        .map_err(|_| error("unavailable", "QA 服务不可用"))?;
    if jobs
        .get(&store.workspace_key)
        .is_some_and(|flag| flag.load(Ordering::SeqCst))
        && store.runs[0].status == "running"
    {
        store.runs[0].status = "cancelling".into();
    }
    let secrets: Vec<_> = vars
        .iter()
        .filter(|(k, _)| sensitive(k))
        .filter_map(|(_, v)| v.as_str().map(str::to_owned))
        .collect();
    store.runs[0].variables = vars
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                if sensitive(k) {
                    json!("[REDACTED]")
                } else {
                    redact(v, &secrets)
                },
            )
        })
        .collect();
    store.runs[0].updated_at = now();
    save(path, store)?;
    let _ = service.changes.send(store.workspace_key.clone());
    Ok(())
}
fn collect_secrets(value: &Value, list: &mut Vec<String>) {
    match value {
        Value::Object(o) => {
            for (key, v) in o {
                if sensitive(key) {
                    if let Some(s) = v.as_str() {
                        list.push(s.into());
                    }
                }
                collect_secrets(v, list);
            }
        }
        Value::Array(a) => {
            for v in a {
                collect_secrets(v, list);
            }
        }
        _ => (),
    }
}
// Validate a browser-supplied cached administrator session before any scenario
// writes. Only an explicit 401 permits a fresh login; other failures stay visible.
fn check_cached_admin(
    client: &reqwest::blocking::Client,
    base_url: &str,
    info_path: &str,
    token: &str,
    prefix: &str,
) -> Result<(u16, Value, String, Value)> {
    let url = endpoint(base_url, info_path)?;
    let authorization = format!("{prefix}{token}");
    let request = json!({"method":"GET", "url":url.as_str(),
        "headers":{"accept":"application/json", "authorization":authorization},
        "body":null, "live":true});
    let response = client.get(url)
        .header("Accept", "application/json")
        .header("Authorization", authorization)
        .send()
        .map_err(|_| error("unavailable", "缓存登录态校验失败或超时"))?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response.take(LIMIT + 1).read_to_end(&mut bytes)
        .map_err(|_| error("unavailable", "读取登录态校验响应失败"))?;
    if bytes.len() as u64 > LIMIT {
        return Err(error("invalid_request", "登录态校验响应超过 1 MiB 上限"));
    }
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    // HTTP 401 can have an empty/non-JSON body; it still means the token expired.
    let response = if status == 401 {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|_| error("invalid_request", "登录态校验未返回有效 JSON"))?
    };
    Ok((status, request, raw, response))
}

fn execute(
    service: &QaService,
    path: &Path,
    store: &mut Snapshot,
    suite: &Suite,
    base_url: &str,
    plan: Vec<(String, Call)>,
    vars: &mut BTreeMap<String, Value>,
    cancel: &AtomicBool,
) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| error("unavailable", "无法创建 HTTP 执行器"))?;
    let started = Instant::now();
    let mut failed = false;
    let mut cancelled = false;
    let mut cleanup_started: Option<Instant> = None;
    for (scenario_id, call) in plan {
        cancelled |= cancel.load(Ordering::SeqCst);
        if call.cleanup {
            let cleanup_start = cleanup_started.get_or_insert_with(Instant::now);
            if cleanup_start.elapsed() >= Duration::from_secs(60) {
                let message = "清理达到 60 秒上限，现场可能未清理完成";
                let previous = store.runs[0].error.take();
                store.runs[0].error = Some(match previous {
                    Some(previous) => format!("{previous}；{message}"),
                    None => message.into(),
                });
                failed = true;
                break;
            }
        } else {
            if !failed && !cancelled && started.elapsed() > Duration::from_secs(300) {
                store.runs[0].error = Some("场景执行达到 5 分钟上限".into());
                failed = true;
            }
            if failed || cancelled {
                continue;
            }
        }
        store.runs[0].current_call = Some(call.id.clone());
        publish(service, path, store, vars)?;
        let tick = Instant::now();
        let mut step = StepResult {
            call_id: call.id.clone(),
            scenario_id,
            name: call.name.clone(),
            actor: call.actor.clone(),
            status: "failed".into(),
            http_status: None,
            elapsed_ms: 0,
            request: Value::Null,
            response: Value::Null,
            response_raw: None,
            stream_events: vec![],
            checks: vec![],
            error: None,
        };
        if call.cleanup
            && call.required_variables.iter().any(|key| {
                vars.get(key).is_none_or(|value| {
                    value.is_null() || value.as_str().is_some_and(str::is_empty)
                })
            })
        {
            step.status = "skipped".into();
            step.error = Some("未取得清理所需变量，已跳过".into());
            store.runs[0].results.push(step);
            publish(service, path, store, vars)?;
            continue;
        }
        let login_prefix = call.path.strip_suffix("login").filter(|prefix| prefix.is_empty() || prefix.ends_with('/'));
        if call.id == "admin-login-call"
            && call.method == "POST"
            && call.sse.is_none()
            && login_prefix.is_some()
            && call.extracts.get("adminToken").is_some_and(|pointer| pointer == "/data")
        {
            if let Some(token) = vars.get("adminToken").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned) {
                let actor = suite.actors.iter().find(|(_, actor)| actor.token.as_deref() == Some("adminToken"));
                if let Some((actor_name, actor)) = actor {
                    match check_cached_admin(&client, base_url, &format!("{}info", login_prefix.unwrap()), &token, &actor.prefix) {
                        Ok((status, request, raw, response)) => {
                            if status == 401 || response["code"] == 401 || response["code"] == "401" {
                                vars.remove("adminToken");
                                // Fall through to the original RSA login exactly once.
                            } else {
                                step.name = "复用管理员登录".into();
                                step.actor = actor_name.clone();
                                step.http_status = Some(status);
                                step.checks = vec![
                                    Check { name: "HTTP 200".into(), passed: status == 200 },
                                    Check { name: "业务 code = 200".into(), passed: response["code"] == 200 },
                                    Check { name: "业务 success = true".into(), passed: response["success"] == true },
                                ];
                                let mut secrets: Vec<_> = vars.iter()
                                    .filter(|(key, _)| sensitive(key))
                                    .filter_map(|(_, value)| value.as_str().map(str::to_owned))
                                    .collect();
                                collect_secrets(&response, &mut secrets);
                                step.request = redact(&request, &secrets);
                                step.request["live"] = json!(false);
                                step.response = redact(&response, &secrets);
                                for value in [&mut step.request, &mut step.response] {
                                    let text = value.to_string();
                                    if text.len() > 48 * 1024 {
                                        *value = json!({"truncated":true,"preview":text.chars().take(24000).collect::<String>()});
                                    }
                                }
                                if let Ok(mut requests) = service.live_requests.lock() {
                                    if let Some((run_id, calls)) = requests.get_mut(&store.workspace_key) {
                                        if run_id == &store.runs[0].id {
                                            calls.insert(call.id.clone(), (request, Some(raw)));
                                        }
                                    }
                                }
                                if step.checks.iter().all(|check| check.passed) {
                                    step.status = "passed".into();
                                } else {
                                    step.error = Some("缓存登录态校验未通过，未重新登录".into());
                                    store.runs[0].error = step.error.clone();
                                    failed = true;
                                }
                                step.elapsed_ms = tick.elapsed().as_millis() as u64;
                                store.runs[0].results.push(step);
                                publish(service, path, store, vars)?;
                                continue;
                            }
                        }
                        Err(err) => {
                            step.error = Some(err.message);
                            store.runs[0].error = step.error.clone();
                            step.elapsed_ms = tick.elapsed().as_millis() as u64;
                            store.runs[0].results.push(step);
                            failed = true;
                            publish(service, path, store, vars)?;
                            continue;
                        }
                    }
                }
            }
        }
        let result = (|| -> Result<()> {
            let url = endpoint(
                base_url,
                render(&json!(call.path), vars, true)?.as_str().unwrap(),
            )?;
            let mut body = render(&call.body, vars, false)?;
            for pointer in &call.encrypt {
                let raw = body
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .ok_or_else(|| error("invalid_request", "RSA 字段不存在或不是字符串"))?;
                let der = STANDARD
                    .decode(&suite.public_key)
                    .map_err(|_| error("invalid_request", "RSA 公钥无效"))?;
                let key = RsaPublicKey::from_public_key_der(&der)
                    .map_err(|_| error("invalid_request", "RSA 公钥格式无效"))?;
                let encrypted = key
                    .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, raw.as_bytes())
                    .map_err(|_| error("invalid_request", "RSA 加密失败"))?;
                *body.pointer_mut(pointer).unwrap() = json!(STANDARD.encode(encrypted));
            }
            let actor = &suite.actors[&call.actor];
            let mut request = client
                .request(
                    reqwest::Method::from_bytes(call.method.as_bytes()).unwrap(),
                    url.clone(),
                )
                .header("Accept", if call.sse.is_some() { "text/event-stream" } else { "application/json" });
            if let Some(token) = &actor.token {
                let value = vars
                    .get(token)
                    .and_then(Value::as_str)
                    .ok_or_else(|| error("invalid_request", format!("缺少身份 token：{token}")))?;
                request = request.header("Authorization", format!("{}{value}", actor.prefix));
            }
            if !body.is_null() {
                request = request.json(&body);
            }
            if let Some(started) = cleanup_started.as_ref() {
                let remaining = Duration::from_secs(60).saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    return Err(error("unavailable", "清理达到 60 秒上限"));
                }
                request = request.timeout(remaining.min(Duration::from_secs(20)));
            }
            let built = request.build().map_err(|_| error("invalid_request", "无法构建目标请求"))?;
            let headers: BTreeMap<String, String> = built.headers().iter()
                .map(|(name, value)| (name.to_string(), value.to_str().unwrap_or("").to_owned()))
                .collect();
            let body_text = built.body().and_then(|body| body.as_bytes())
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            let actual = json!({"method":built.method().as_str(), "url":built.url().as_str(), "headers":headers, "body":body_text, "live":true});
            step.request = actual.clone();
            step.request["live"] = json!(false);
            if let Ok(mut requests) = service.live_requests.lock() {
                if let Some((run_id, calls)) = requests.get_mut(&store.workspace_key) {
                    if run_id == &store.runs[0].id { calls.insert(call.id.clone(), (actual, None)); }
                }
            }
            let response = if let Some(options) = &call.sse {
                let budget = match cleanup_started {
                    Some(cleanup) => Duration::from_secs(60).saturating_sub(cleanup.elapsed()),
                    None => Duration::from_secs(300).saturating_sub(started.elapsed()),
                };
                let cancellation = if call.cleanup { None } else { Some(cancel) };
                let secrets: Vec<_> = vars.iter().filter(|(key, _)| sensitive(key))
                    .filter_map(|(_, value)| value.as_str().map(str::to_owned)).collect();
                let mut last_update: Option<Instant> = None;
                let mut event_bytes = 0;
                let terminal = sse::read(built, options, budget.min(Duration::from_secs(180)), cancellation, |status, event| {
                    step.http_status = Some(status);
                    let event = redact(&event, &secrets);
                    let size = event.to_string().len();
                    if size <= 12288 {
                        while event_bytes + size > 12288 || step.stream_events.len() >= 64 {
                            event_bytes -= step.stream_events.remove(0).to_string().len();
                        }
                        event_bytes += size;
                        step.stream_events.push(event);
                    }
                    if last_update.is_none_or(|last| last.elapsed() >= Duration::from_millis(200)) {
                        let mut progress = step.clone();
                        progress.status = "streaming".into();
                        progress.request = redact(&progress.request, &secrets);
                        if !body.is_null() {
                            progress.request["body"] = json!(redact(&body, &secrets).to_string());
                        }
                        store.runs[0].results.push(progress);
                        let published = publish(service, path, store, vars);
                        store.runs[0].results.pop();
                        published?;
                        last_update = Some(Instant::now());
                    }
                    Ok(())
                })?;
                step.http_status = Some(terminal.0);
                terminal.1
            } else {
                let response = client.execute(built)
                    .map_err(|_| error("unavailable", "目标请求失败或超时（未自动重试）"))?;
                step.http_status = Some(response.status().as_u16());
                let mut bytes = vec![];
                response
                    .take(LIMIT + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| error("unavailable", "读取响应失败"))?;
                if bytes.len() as u64 > LIMIT {
                    return Err(error("invalid_request", "响应超过 1 MiB 上限"));
                }
                let response: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| error("invalid_request", "目标没有返回有效 JSON"))?;
                if let Ok(mut requests) = service.live_requests.lock() {
                    if let Some((run_id, calls)) = requests.get_mut(&store.workspace_key) {
                        if run_id == &store.runs[0].id {
                            if let Some((_, raw)) = calls.get_mut(&call.id) {
                                *raw = Some(String::from_utf8_lossy(&bytes).into_owned());
                            }
                        }
                    }
                }
                response
            };
            step.response = response.clone();
            step.checks.push(Check {
                name: "HTTP 200".into(),
                passed: step.http_status == Some(200),
            });
            if call.sse.is_none() {
                step.checks.push(Check {
                    name: "业务 code = 200".into(),
                    passed: response["code"] == 200,
                });
                step.checks.push(Check {
                    name: "业务 success = true".into(),
                    passed: response["success"] == true,
                });
            }
            for check in &call.assertions {
                step.checks.push(Check {
                    name: format!("{} {}", check.path, check.op),
                    passed: assertion(&response, check, vars)?,
                });
            }
            if step.checks.iter().any(|c| !c.passed) {
                return Err(error(
                    "assertion_failed",
                    "接口返回或业务断言不满足预期",
                ));
            }
            for (name, pointer) in &call.extracts {
                let value = response
                    .pointer(pointer)
                    .filter(|v| !v.is_null())
                    .ok_or_else(|| error("invalid_request", format!("提取变量 {name} 失败")))?;
                vars.insert(name.clone(), value.clone());
            }
            step.status = "passed".into();
            Ok(())
        })();
        let mut secrets: Vec<_> = vars
            .iter()
            .filter(|(k, _)| sensitive(k))
            .filter_map(|(_, v)| v.as_str().map(str::to_owned))
            .collect();
        collect_secrets(&step.response, &mut secrets);
        for (name, pointer) in &call.extracts {
            if sensitive(name) {
                if let Some(s) = step.response.pointer(pointer).and_then(Value::as_str) {
                    secrets.push(s.into());
                }
            }
        }
        // Parse the serialized body before redaction so sensitive JSON keys remain protected on disk.
        if let Some(body) = step.request.get("body").and_then(Value::as_str) {
            if let Ok(value) = serde_json::from_str::<Value>(body) {
                step.request["body"] = json!(redact(&value, &secrets).to_string());
            }
        }
        step.request = redact(&step.request, &secrets);
        step.response = redact(&step.response, &secrets);
        for event in &mut step.stream_events {
            *event = redact(event, &secrets);
        }
        for value in [&mut step.request, &mut step.response] {
            let text = value.to_string();
            if text.len() > 48 * 1024 {
                *value = json!({"truncated":true,"preview":text.chars().take(24000).collect::<String>()});
            }
        }
        step.elapsed_ms = tick.elapsed().as_millis() as u64;
        if let Err(err) = result {
            if err.code == "cancelled" && !call.cleanup {
                cancelled = true;
                step.status = "cancelled".into();
                store.runs[0].results.push(step);
                publish(service, path, store, vars)?;
                continue;
            }
            store.runs[0]
                .error
                .get_or_insert_with(|| format!("{}：{}", call.name, err.message));
            step.error = Some(err.message);
            store.runs[0].results.push(step);
            failed = true;
            publish(service, path, store, vars)?;
            continue;
        }
        store.runs[0].results.push(step);
        publish(service, path, store, vars)?;
    }
    store.runs[0].status = if failed {
        "failed"
    } else if cancelled || cancel.load(Ordering::SeqCst) {
        "cancelled"
    } else {
        "passed"
    }
    .into();
    Ok(())
}
#[cfg(test)]
mod tests;
