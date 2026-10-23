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
        inputs: BTreeMap<String, Value>,
    },
    Cancel {
        run_id: String,
    },
}
#[derive(Default)]
pub struct QaService {
    jobs: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
}
impl DevFlowEngine {
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
                        run.current_call = None;
                        run.error = Some("DevFlow 重启，执行已中断；不会重放请求".into());
                        run.updated_at = now();
                    }
                }
                save(&path, &store)?;
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
                inputs,
            } => {
                let suite = store
                    .suite
                    .clone()
                    .ok_or_else(|| error("not_found", "请先初始化场景"))?;
                validate(&suite)?;
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
                if !vars.contains_key("adminPassword") {
                    return Err(error("invalid_request", "请提供管理员密码，仅用于本轮执行"));
                }
                let run = Run {
                    id: id.clone(),
                    scenario_id,
                    suite_revision: store.revision,
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
                store.runs.truncate(20);
                save(&path, &store)?;
                let initial = store.clone();
                let cancel = Arc::new(AtomicBool::new(false));
                jobs.insert(request.workspace_key.clone(), cancel.clone());
                let service = self.qa.clone();
                std::thread::spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        execute(
                            &service, &path, &mut store, &suite, plan, &mut vars, &cancel,
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
                        let _ = save(&path, &store);
                        jobs.remove(&store.workspace_key);
                    }
                    drop(lock);
                });
                return Ok(initial);
            }
            Operation::Cancel { .. } => unreachable!(),
        }
        Ok(store)
    }
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
    let allowed =
        std::env::var("DEVFLOW_QA_ORIGINS").unwrap_or_else(|_| "http://192.168.1.200".into());
    if !matches!(base.scheme(), "http" | "https")
        || !base.username().is_empty()
        || base.password().is_some()
        || !allowed
            .split(',')
            .any(|x| x.trim() == base.origin().ascii_serialization())
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
        for call in item.actions.iter().flat_map(|a| &a.calls) {
            result.push((id.into(), call.clone()));
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
    Ok(plan)
}
fn validate(suite: &Suite) -> Result<()> {
    endpoint(&suite.base_url, "info")?;
    if serde_json::to_vec(suite).unwrap_or_default().len() > 256 * 1024
        || suite.scenarios.is_empty()
        || suite.scenarios.len() > 50
    {
        return Err(error("invalid_request", "场景需为 1–50 项且不超过 256 KiB"));
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
                endpoint(&suite.base_url, &call.path)?;
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
    save(path, store)
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
fn execute(
    service: &QaService,
    path: &Path,
    store: &mut Snapshot,
    suite: &Suite,
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
    for (scenario_id, call) in plan {
        if cancel.load(Ordering::SeqCst) {
            store.runs[0].status = "cancelled".into();
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(300) {
            return Err(error("unavailable", "场景执行达到 5 分钟上限"));
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
            checks: vec![],
            error: None,
        };
        let result = (|| -> Result<()> {
            let url = endpoint(
                &suite.base_url,
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
                .header("Accept", "application/json");
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
            step.request = json!({"method":call.method,"url":url.as_str(),"actor":call.actor,"authorization":if actor.token.is_some() { "[REDACTED]" } else { "无" },"body":body});
            let response = request
                .send()
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
            step.response = response.clone();
            step.checks.push(Check {
                name: "HTTP 200".into(),
                passed: step.http_status == Some(200),
            });
            step.checks.push(Check {
                name: "业务 code = 200".into(),
                passed: response["code"] == 200,
            });
            step.checks.push(Check {
                name: "业务 success = true".into(),
                passed: response["success"] == true,
            });
            for check in &call.assertions {
                step.checks.push(Check {
                    name: format!("{} {}", check.path, check.op),
                    passed: assertion(&response, check, vars)?,
                });
            }
            if step.checks.iter().any(|c| !c.passed) {
                return Err(error(
                    "assertion_failed",
                    "接口返回或业务断言不满足预期，已停止后续请求",
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
        step.request = redact(&step.request, &secrets);
        step.response = redact(&step.response, &secrets);
        for value in [&mut step.request, &mut step.response] {
            let text = value.to_string();
            if text.len() > 48 * 1024 {
                *value = json!({"truncated":true,"preview":text.chars().take(24000).collect::<String>()});
            }
        }
        step.elapsed_ms = tick.elapsed().as_millis() as u64;
        if let Err(err) = result {
            step.error = Some(err.message);
            store.runs[0].results.push(step);
            store.runs[0].status = "failed".into();
            publish(service, path, store, vars)?;
            return Ok(());
        }
        store.runs[0].results.push(step);
        publish(service, path, store, vars)?;
    }
    store.runs[0].status = if cancel.load(Ordering::SeqCst) {
        "cancelled"
    } else {
        "passed"
    }
    .into();
    Ok(())
}
#[cfg(test)]
mod tests;
