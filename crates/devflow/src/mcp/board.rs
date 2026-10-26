use super::*;

pub fn sync_tool() -> Value {
    json!({"name":"board_sync","description":"Read/configure Gitee board snapshot synchronization or push now. Use workspace_get and board_get first. Configuring automatic=true authorizes background uploads every 30 seconds while this workspace is active. Credentials come only from backend DEVFLOW_GITEE_TOKEN. Never configure or push without user authorization. Restore requires the human Web UI. feature empty string means unclassified; task_create requires only title. AI task refinement uses task_update after discussing the proposal with the user; do not claim or execute merely to refine a task.","inputSchema":{"type":"object","properties":{"workspaceKey":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"boardRevision":{"type":"integer","minimum":0},"action":{"enum":["status","configure","push"]},"settings":{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"branch":{"type":"string"},"boardId":{"type":"string","format":"uuid"},"automatic":{"type":"boolean"}},"required":["owner","repo","branch","boardId","automatic"],"additionalProperties":false}},"required":["workspaceKey","expectedRevision","boardRevision","action"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}})
}

pub fn read_tool() -> Value {
    json!({"name":"board_get","description":"Read the active workspace's persisted board, including feature context, tasks, execution progress, delivery results, history and revision. First use workspace_get for workspaceKey (workspaceFile or workspaceRoot) and expectedRevision. Only local owner transport is supported.","inputSchema":{"type":"object","properties":{"workspaceKey":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["workspaceKey","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}})
}
pub fn write_tool() -> Value {
    let string = json!({"type":"string"});
    let feature = json!({"type":"object","properties":{"id":string,"name":string,"description":string,"goal":string,"x":{"type":"number"},"y":{"type":"number"}},"required":["id","name","description","goal","x","y"],"additionalProperties":false});
    let edges = json!({"type":"array","items":{"type":"array","items":{"type":"string"},"minItems":2,"maxItems":2}});
    let operation = |kind: &str, mut properties: Value, required: Vec<&str>| {
        properties["type"] = json!({"const":kind});
        let mut fields = vec!["type"]; fields.extend(required);
        json!({"type":"object","properties":properties,"required":fields,"additionalProperties":false})
    };
    let task = json!({"title":string,"description":string,"acceptance":string,"feature":string,"priority":{"enum":["高","普通"]}});
    let mut update = task.clone(); update["id"] = string.clone();
    let variants = vec![
        operation("feature_save", json!({"feature":feature}), vec!["feature"]),
        operation("feature_delete", json!({"id":string,"moveTo":string}), vec!["id"]),
        operation("edges", json!({"edges":edges}), vec!["edges"]),
        operation("task_create", task, vec!["title"]),
        operation("task_update", update, vec!["id","title","description","acceptance","feature","priority"]),
        operation("task_delete", json!({"id":string}), vec!["id"]),
        operation("action", json!({"id":string,"action":{"enum":["claim","release","submit","progress","block","unblock"]},"text":string}), vec!["id","action"]),
    ];
    json!({"name":"board_execute","description":"Mutate the same board used by Web UI. Only write agreed tasks when requested by the user. sessionId must be a stable unique id for this AI conversation (1–200 bytes); reuse it for claim/progress/release/submit. Session ids identify cooperative owners, not independent authentication principals: all clients use the local owner's transport token. Re-read board_get after conflicts; never blindly overwrite. Creation always enters todo. task_create requires only title; feature defaults to empty string (unclassified), priority defaults to 普通, description and acceptance default to empty. Refine task metadata with task_update without claiming or executing; discuss suggestions before applying them. Claim is atomic; only its owner may progress/submit/release/block. Submission needs actual delivery and validation notes; progress/block need text. Human Web UI can release stalled claims, reject with reason, approve, reopen. Claiming never starts an agent or executes source code. Feature ids should be stable UUIDs; feature_save edits by id. Deleting a populated feature requires moveTo. Mutations return the full new snapshot and board revision.","inputSchema":{"type":"object","properties":{"sessionId":{"type":"string","minLength":1,"maxLength":200},"request":{"type":"object","properties":{"workspaceKey":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"boardRevision":{"type":"integer","minimum":0},"operation":{"oneOf":variants}},"required":["workspaceKey","expectedRevision","boardRevision","operation"],"additionalProperties":false}},"required":["sessionId","request"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}})
}
pub async fn execute(state: Arc<AppState>, name: &str, arguments: Value, authenticated: bool) -> Result<devflow_core::workspace::BoardSnapshot, WorkspaceError> {
    let invalid = |e: serde_json::Error| WorkspaceError { code: "invalid_request", message: e.to_string() };
    if name == "board_get" {
        let request = serde_json::from_value(arguments).map_err(invalid)?;
        call_core(state, move |e| e.workspace.board_read(request, authenticated)).await
    } else {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Args { session_id: String, request: devflow_core::workspace::BoardRequest }
        let args: Args = serde_json::from_value(arguments).map_err(invalid)?;
        if args.session_id.trim().is_empty() || args.session_id.len() > 200 {
            return Err(WorkspaceError { code: "invalid_request", message: "sessionId 需为 1–200 字节的稳定会话标识".into() });
        }
        let actor = format!("Codex:{}", args.session_id);
        call_core(state, move |e| e.workspace.board_execute(args.request, authenticated, &actor, false)).await
    }
}
