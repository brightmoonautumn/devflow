//! Progressive, read-only projections shared by HTTP and MCP. Existing full DTOs
//! remain the storage/UI contract; projections never participate in mutations.
use crate::{DevFlowEngine, workspace::WorkspaceError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, WorkspaceError>;
fn error(code: &'static str, message: impl Into<String>) -> WorkspaceError {
    WorkspaceError { code, message: message.into() }
}
fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| error("invalid_request", e.to_string()))
}
fn encode(value: impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| error("invalid_request", e.to_string()))
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum View { #[default] Summary, Detail, History, Full, Status }

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadOptions {
    pub view: View,
    pub id: Option<String>,
    pub item_id: Option<String>,
    pub index: Option<usize>,
    pub section: Option<Section>,
    pub feature: Option<String>,
    pub status: Option<String>,
    pub query: Option<String>,
    pub offset: usize,
    pub char_offset: usize,
    pub limit: Option<usize>,
    pub snapshot_id: Option<String>,
    pub if_snapshot_id: Option<String>,
    pub max_chars: Option<usize>,
    pub raw: bool,
}
#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Section { Feature, Run, Evidence }
impl ReadOptions {
    pub fn extract(arguments: &mut Value) -> Result<Self> {
        let value = arguments.as_object_mut().ok_or_else(|| error("invalid_request", "参数必须为对象"))?.remove("read");
        let options: Self = match value { Some(value) => decode(value)?, None => Self::default() };
        if options.limit.is_some_and(|n| n == 0 || n > 200)
            || options.max_chars.is_some_and(|n| !(256..=24000).contains(&n)) {
            return Err(error("invalid_request", "read.limit 为 1–200，maxChars 为 256–24000"));
        }
        Ok(options)
    }
    pub fn extract_write(arguments: &mut Value) -> Result<Self> {
        let options = Self::extract(arguments)?;
        // Reject read selectors before doing any side effect. A projection error
        // after a successful mutation must not encourage a duplicate retry.
        if !matches!(options.view, View::Summary | View::Status | View::Full)
            || options.id.is_some() || options.item_id.is_some() || options.index.is_some()
            || options.section.is_some() || options.feature.is_some() || options.status.is_some()
            || options.query.is_some() || options.offset != 0 || options.char_offset != 0
            || options.snapshot_id.is_some() || options.if_snapshot_id.is_some() || options.raw {
            return Err(error("invalid_request", "写入回执仅支持 summary/status/full 视图及 limit/maxChars"));
        }
        Ok(options)
    }
    fn limit(&self) -> usize { self.limit.unwrap_or(30) }
    pub fn budget(&self) -> usize { self.max_chars.unwrap_or(6000) }
    fn selected<'a>(&self, values: &'a [Value]) -> Result<&'a Value> {
        let id = self.id.as_deref().ok_or_else(|| error("invalid_request", "详情/历史读取需要 read.id"))?;
        values.iter().find(|v| v["id"].as_str() == Some(id)).ok_or_else(|| error("not_found", "指定记录不存在"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRead { pub tool: String, pub arguments: Value }

pub fn supports(name: &str) -> bool {
    matches!(name, "workspace_get" | "workspace_list" | "board_get" | "qa_get" | "diagnosis_get"
        | "workspace_children" | "workspace_search" | "workspace_file_read" | "runtime_list"
        | "java_output" | "node_output" | "runtime_diagnostics")
}

fn fields(value: &Value, names: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for name in names {
        if let Some(value) = value.get(*name) { result.insert((*name).into(), value.clone()); }
    }
    Value::Object(result)
}
fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn fingerprint(value: &Value) -> Result<String> {
    // A consistency token, not a credential or security hash.
    let bytes = serde_json::to_vec(value).map_err(|e| error("invalid_request", e.to_string()))?;
    let hash = bytes.iter().fold(14695981039346656037u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(1099511628211));
    Ok(format!("{hash:016x}"))
}
fn page(values: Vec<Value>, options: &ReadOptions) -> Value {
    let total = values.len();
    let mut items = Vec::new();
    let mut chars = 0;
    for item in values.into_iter().skip(options.offset).take(options.limit()) {
        let size = item.to_string().chars().count();
        // Never split a record or return an empty page that cannot advance.
        if !items.is_empty() && chars + size > options.budget() { break; }
        chars += size;
        items.push(item);
    }
    let end = options.offset.saturating_add(items.len());
    json!({"items":items,"total":total,"offset":options.offset,"nextOffset":if end < total { Some(end) } else { None },"hasMore":end < total})
}
fn text_page(text: &str, options: &ReadOptions) -> Value {
    let total = text.chars().count();
    let text: String = text.chars().skip(options.offset).take(options.budget()).collect();
    let end = options.offset.saturating_add(text.chars().count());
    json!({"text":text,"totalChars":total,"offset":options.offset,"nextOffset":if end < total { Some(end) } else { None },"hasMore":end < total})
}
fn preview(value: &Value, name: &str) -> String {
    value[name].as_str().unwrap_or("").chars().take(160).collect()
}

impl DevFlowEngine {
    pub fn context_read(&self, request: ContextRead, authenticated: bool) -> Result<Value> {
        // Even formerly public reads stay behind the owner boundary on this API.
        if !authenticated { return Err(error("unauthorized", "需要本机管理令牌")); }
        let ContextRead { tool, mut arguments } = request;
        let options = ReadOptions::extract(&mut arguments)?;
        let snapshot = match tool.as_str() {
            "board_get" => encode(self.workspace.board_read(decode(arguments)?, authenticated)?)?,
            "qa_get" => encode(self.qa_get(decode(arguments)?, authenticated)?)?,
            "diagnosis_get" => encode(self.diagnosis_get(decode(arguments)?, authenticated)?)?,
            "workspace_get" | "workspace_list" | "runtime_list" => {
                if arguments != json!({}) { return Err(error("invalid_request", "仅接受 read 参数")); }
                if tool == "runtime_list" { encode(self.runtime_snapshot()?)? } else { encode(self.workspace.snapshot()?)? }
            },
            "workspace_children" => encode(self.workspace.children(decode(arguments)?)?)?,
            "workspace_search" => encode(self.workspace.search(decode(arguments)?, authenticated)?)?,
            "workspace_file_read" => encode(self.workspace.read_file(decode(arguments)?, authenticated)?)?,
            "runtime_diagnostics" => encode(self.runtime_diagnostics(decode(arguments)?)?)?,
            "java_output" => {
                let query: crate::runtime::java::JavaQuery = decode(arguments)?;
                let project = self.workspace.runtime_project(&query.project_path, None)?;
                encode(self.java.output(&project, query.after, query.run_id.as_deref())?)?
            },
            "node_output" => {
                let query: crate::runtime::node::NodeQuery = decode(arguments)?;
                let project = self.workspace.node_runtime_project(&query.project_path, query.script.as_deref(), None)?;
                encode(self.node.output(&project, query.after, query.run_id.as_deref())?)?
            },
            _ => return Err(error("invalid_request", "不支持的只读上下文工具")),
        };
        project(&tool, snapshot, &options)
    }
}

pub fn project(tool: &str, snapshot: Value, options: &ReadOptions) -> Result<Value> {
    // Full mode is deliberately exact for legacy clients and edit preconditions.
    if options.view == View::Full { return Ok(if tool == "workspace_list" { snapshot["projects"].clone() } else { snapshot }); }
    let token = fingerprint(&snapshot)?;
    if options.snapshot_id.as_ref().is_some_and(|id| id != &token) {
        return Err(error("conflict", "读取期间数据已变化，请从第一页重新读取"));
    }
    if options.if_snapshot_id.as_ref() == Some(&token) {
        return Ok(json!({"snapshotId":token,"unchanged":true}));
    }
    let mut result = match tool {
        "board_get" => board(&snapshot, options)?,
        "qa_get" => qa(&snapshot, options)?,
        "diagnosis_get" => diagnosis(&snapshot, options)?,
        "workspace_get" => {
            let mut value = fields(&snapshot, &["workspaceRoot","workspaceFile","name","revision","dirty","folders","warnings"]);
            value["projectCount"] = json!(array(&snapshot, "projects").len());
            value
        },
        "workspace_list" => page(array(&snapshot, "projects").iter().map(|p| fields(p, &["name","kind","path","nodeScript"])).collect(), options),
        "runtime_list" => page(array(&snapshot, "projects").iter().filter(|p| options.id.as_deref().is_none_or(|id| p["projectPath"].as_str() == Some(id))).map(|p| {
            if options.view == View::Detail { p.clone() } else { fields(p, &["projectPath","nodeScript","managedState","primaryPids","listeningPorts","portError"]) }
        }).collect(), options),
        "workspace_children" | "workspace_search" => {
            let key = if tool == "workspace_children" { "entries" } else { "matches" };
            let mut value = fields(&snapshot, &["path","revision","truncated","warnings"]);
            value[key] = page(array(&snapshot, key).to_vec(), options);
            value
        },
        "workspace_file_read" => file(&snapshot, options)?,
        "java_output" | "node_output" => output(&snapshot, options)?,
        "runtime_diagnostics" => {
            let mut value = fields(&snapshot, &["runId","state","truncated","redacted","text"]);
            if options.view == View::Detail { value["excerpt"] = text_page(snapshot["excerpt"].as_str().unwrap_or(""), options); }
            value
        },
        _ => return Err(error("invalid_request", "不支持的上下文投影")),
    };
    result["snapshotId"] = json!(token);
    Ok(result)
}

fn board(snapshot: &Value, options: &ReadOptions) -> Result<Value> {
    let mut value = fields(snapshot, &["workspaceKey","revision"]);
    let tasks = array(snapshot, "tasks");
    if options.section == Some(Section::Feature) {
        if options.view == View::Detail || options.id.is_some() {
            value["feature"] = fields(options.selected(array(snapshot, "features"))?, &["id","name","description","goal"]);
        } else {
            value["features"] = page(array(snapshot, "features").iter().map(|f| fields(f, &["id","name"])).collect(), options);
        }
        return Ok(value);
    }
    if options.view == View::History && options.id.is_none() {
        value["activity"] = page(array(snapshot, "activity").iter().rev().cloned().collect(), options);
        return Ok(value);
    }
    if matches!(options.view, View::Detail | View::History) {
        let task = options.selected(tasks)?;
        if options.view == View::History {
            value["taskId"] = task["id"].clone();
            value["activity"] = page(array(task, "activity").iter().rev().cloned().collect(), options);
        } else {
            value["task"] = fields(task, &["id","title","description","acceptance","feature","priority","status","owner","ownerId","blocked","result","createdAt","statusChangedAt","codexThreadId"]);
            value["activityCount"] = json!(array(task, "activity").len());
            value["feature"] = array(snapshot, "features").iter().find(|f| f["id"] == task["feature"]).map(|f| fields(f, &["id","name","description","goal"])).unwrap_or(Value::Null);
        }
    } else {
        let mut counts = json!({"todo":0,"doing":0,"review":0,"done":0});
        for task in tasks { if let Some(status) = task["status"].as_str() { counts[status] = json!(counts[status].as_u64().unwrap_or(0) + 1); } }
        value["counts"] = counts;
        if options.view != View::Status {
            value["features"] = page(array(snapshot, "features").iter().map(|f| fields(f, &["id","name"])).collect(), options);
            let query = options.query.as_deref().unwrap_or("").to_lowercase();
            let filtered = tasks.iter().filter(|t| options.feature.as_ref().is_none_or(|f| t["feature"].as_str() == Some(f.as_str()))
                && options.status.as_ref().is_none_or(|s| t["status"].as_str() == Some(s.as_str()))
                && (query.is_empty() || ["id","title","description"].iter().any(|k| t[*k].as_str().unwrap_or("").to_lowercase().contains(&query))))
                .map(|t| {
                    let mut row = fields(t, &["id","title","feature","priority","status","ownerId"]);
                    row["featureName"] = array(snapshot, "features").iter().find(|f| f["id"] == t["feature"]).map(|f| f["name"].clone()).unwrap_or(Value::Null);
                    row["blocked"] = json!(t["blocked"].as_str().is_some_and(|s| !s.is_empty())); row
                }).collect();
            value["tasks"] = page(filtered, options);
            value["filters"] = json!({"feature":options.feature,"status":options.status,"query":options.query});
        }
    }
    Ok(value)
}

fn qa(snapshot: &Value, options: &ReadOptions) -> Result<Value> {
    let mut value = fields(snapshot, &["workspaceKey","revision"]);
    if options.view == View::Detail && options.section == Some(Section::Run) {
        let run = options.selected(array(snapshot, "runs"))?;
        let id = options.item_id.as_deref().ok_or_else(|| error("invalid_request", "读取请求/响应需提供 read.itemId（callId）"))?;
        let step = array(run, "results").iter().find(|s| s["callId"].as_str() == Some(id)).ok_or_else(|| error("not_found", "执行步骤不存在"))?;
        value["step"] = fields(step, &["callId","scenarioId","status","error"]);
        value["exchange"] = text_page(&serde_json::to_string(step).map_err(|e| error("invalid_request", e.to_string()))?, options);
    } else if options.view == View::Detail {
        value["scenario"] = options.selected(array(&snapshot["suite"], "scenarios"))?.clone();
        value["suite"] = fields(&snapshot["suite"], &["name","baseUrl","siteUrl"]);
    } else if options.view == View::History {
        let run = options.selected(array(snapshot, "runs"))?;
        value["run"] = fields(run, &["id","scenarioId","status","currentCall","error"]);
        value["results"] = page(array(run, "results").iter().map(|r| fields(r, &["callId","scenarioId","name","status","httpStatus","elapsedMs","checks","error"])).collect(), options);
    } else {
        value["suite"] = fields(&snapshot["suite"], &["name","baseUrl"]);
        if options.view != View::Status {
            value["scenarios"] = page(array(&snapshot["suite"], "scenarios").iter().map(|s| fields(s, &["id","module","title"])).collect(), options);
        }
        value["runs"] = page(array(snapshot, "runs").iter().filter(|r| options.id.as_ref().is_none_or(|id| r["id"].as_str() == Some(id.as_str()))).map(|r| {
            let mut row = fields(r, &["id","scenarioId","status","currentCall","error"]);
            row["stepCount"] = json!(array(r, "results").len());
            row["passedCount"] = json!(array(r, "results").iter().filter(|s| s["status"] == "passed").count());
            row["failedCount"] = json!(array(r, "results").iter().filter(|s| s["status"] == "failed").count()); row
        }).collect(), options);
    }
    Ok(value)
}

fn diagnosis(snapshot: &Value, options: &ReadOptions) -> Result<Value> {
    let sessions = snapshot.as_array().map(Vec::as_slice).unwrap_or(&[]);
    if matches!(options.view, View::Detail | View::History) {
        let session = options.selected(sessions)?;
        let mut value = fields(session, &["id","revision","projectPath","pid","runId","problem","status","progress","conclusion"]);
        value["evidenceCount"] = json!(array(session, "evidence").len());
        if options.view == View::History {
            value["evidence"] = page(array(session, "evidence").iter().map(|e| {
                let mut row = fields(e, &["command","truncated","timedOut","recordedAt"]);
                row["outputPreview"] = json!(preview(e, "output")); row
            }).collect(), options);
        } else if options.section == Some(Section::Evidence) {
            let index = options.index.ok_or_else(|| error("invalid_request", "读取证据需提供 read.index（从 0 开始）"))?;
            let evidence = array(session, "evidence").get(index).ok_or_else(|| error("not_found", "证据不存在"))?;
            value["evidence"] = fields(evidence, &["command","truncated","timedOut","recordedAt"]);
            value["evidence"]["output"] = text_page(evidence["output"].as_str().unwrap_or(""), options);
        }
        Ok(value)
    } else {
        Ok(page(sessions.iter().rev().filter(|s| options.id.as_ref().is_none_or(|id| s["id"].as_str() == Some(id.as_str()))).map(|s| {
            let mut row = fields(s, &["id","revision","projectPath","status","progress"]);
            row["evidenceCount"] = json!(array(s, "evidence").len()); row
        }).collect(), options))
    }
}

fn file(snapshot: &Value, options: &ReadOptions) -> Result<Value> {
    let mut value = fields(snapshot, &["path","revision","readonly","readonlyReason","lineEnding","bom"]);
    let content = snapshot["content"].as_str().unwrap_or("");
    let lines: Vec<_> = content.split_inclusive('\n').collect();
    let mut text = String::new();
    let mut chars = 0;
    let mut complete = 0;
    let mut next_char = 0;
    for (index, line) in lines.iter().skip(options.offset).take(options.limit.unwrap_or(80)).enumerate() {
        let start = if index == 0 { options.char_offset } else { 0 };
        let length = line.chars().count();
        if start > length { return Err(error("invalid_request", "charOffset exceeds line length")); }
        let chunk: String = line.chars().skip(start).take(options.budget() - chars).collect();
        let used = chunk.chars().count();
        text.push_str(&chunk); chars += used;
        if start + used < length { next_char = start + used; break; }
        complete += 1;
        if chars >= options.budget() { break; }
    }
    let next = options.offset.saturating_add(complete);
    value["content"] = json!(text);
    value["startLine"] = json!(options.offset.saturating_add(1));
    value["charOffset"] = json!(options.char_offset);
    value["totalLines"] = json!(lines.len());
    value["nextOffset"] = if next < lines.len() { json!(next) } else { Value::Null };
    value["nextCharOffset"] = json!(next_char);
    value["hasMore"] = json!(next < lines.len());
    value["partial"] = json!(options.offset > 0 || options.char_offset > 0 || next < lines.len());
    value["requiresFullReadForWrite"] = json!(true);
    Ok(value)
}

fn output(snapshot: &Value, options: &ReadOptions) -> Result<Value> {
    let mut value = fields(snapshot, &["runId","state","pid","exitCode","truncated","arthasUrl","attaching"]);
    let lines = array(snapshot, "lines");
    if options.view == View::Status { return Ok(value); }
    if options.char_offset > 0 && snapshot["truncated"] == true {
        return Err(error("conflict", "日志窗口已变化，请清零 charOffset 并重新读取"));
    }
    let mut rows = Vec::new();
    let mut chars = 0;
    let mut next = snapshot["nextCursor"].clone();
    let mut next_char = 0;
    for (index, line) in lines.iter().take(options.limit.unwrap_or(50)).enumerate() {
        let text = line["text"].as_str().unwrap_or("");
        let start = if index == 0 { options.char_offset } else { 0 };
        let size = text.chars().count();
        if start > size { return Err(error("invalid_request", "charOffset exceeds log line length")); }
        let chunk: String = text.chars().skip(start).take(options.budget() - chars).collect();
        let used = chunk.chars().count();
        let mut row = fields(line, &["sequence","stream"]);
        row["text"] = json!(chunk);
        chars += used;
        if start + used < size {
            row["partial"] = json!(true);
            next_char = start + used;
            next = json!(line["sequence"].as_u64().unwrap_or(1).saturating_sub(1));
            rows.push(row); break;
        }
        next = line["sequence"].clone();
        rows.push(row);
        if chars >= options.budget() { break; }
    }
    value["nextCursor"] = next;
    value["nextCharOffset"] = json!(next_char);
    value["hasMore"] = json!(next_char > 0 || rows.len() < lines.len() || lines.len() == 500);
    value["lines"] = json!(rows);
    Ok(value)
}

/// A mutation receipt never invites overwriting a task from a partial summary.
pub fn receipt(snapshot: Value, options: &ReadOptions) -> Result<Value> {
    if options.view == View::Full { return Ok(snapshot); }
    let mut value = fields(&snapshot, &["workspaceKey","revision","id","status","progress","conclusion"]);
    value["saved"] = json!(true);
    if let Some(run) = array(&snapshot, "runs").first() {
        value["latestRun"] = fields(run, &["id","scenarioId","status","currentCall","error"]);
    }
    if let Some(id) = &options.id {
        value["id"] = json!(id);
        if let Some(task) = array(&snapshot, "tasks").iter().find(|t| t["id"].as_str() == Some(id.as_str())) {
            value["task"] = fields(task, &["id","title","status","ownerId","blocked"]);
        }
        if let Some(feature) = array(&snapshot, "features").iter().find(|f| f["id"].as_str() == Some(id.as_str())) {
            value["feature"] = fields(feature, &["id","name"]);
        }
    }
    Ok(value)
}
