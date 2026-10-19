//! Minimal JSON-RPC adapter. Workspace business rules live only in devflow-core.
//! This seed still requires full MCP transport/lifecycle compliance before release.
use crate::{AppState, call_core, security};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use devflow_core::workspace::{
    WorkspaceChildrenRequest, WorkspaceError, WorkspaceOperation, WorkspaceRequest,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn tool_result<T: serde::Serialize>(result: Result<T, WorkspaceError>) -> Value {
    match result {
        Ok(value) => {
            let value = json!(value);
            let structured = if value.is_object() {
                value.clone()
            } else {
                json!({"result":value})
            };
            json!({"structuredContent":structured,"content":[{"type":"text","text":value.to_string()}]})
        }
        Err(err) => {
            json!({"isError":true,"content":[{"type":"text","text":json!(err).to_string()}]})
        }
    }
}
fn tools() -> Value {
    let read = |name: &str, description: &str| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true}});
    let write = |name: &str, description: &str, path_required: bool, extra_name: bool| {
        let mut properties = json!({"expectedRevision":{"type":"integer","minimum":0},"path":{"type":"string"},"discardChanges":{"type":"boolean","default":false}});
        if extra_name {
            properties["name"] = json!({"type":"string"});
        }
        let required = if path_required {
            json!(["expectedRevision", "path"])
        } else {
            json!(["expectedRevision"])
        };
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}})
    };
    json!({"tools":[
        {"name":"node_execute","description":"Start or stop an authorized Node service using a discovered package script, never an arbitrary command. For aggregate projects pass nodeScript from workspace_get as script. npm/pnpm/yarn are supported; dependencies are not installed automatically.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"start"},"packageManager":{"enum":["npm","pnpm","yarn"]},"args":{"type":"array","items":{"type":"string"}}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"stop"},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false}]}},"required":["projectPath","expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}},
        {"name":"node_options","description":"Read discovered launch scripts and preferred package manager for an active workspace Node service.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}},
        {"name":"node_output","description":"Read bounded Node stdout/stderr and run state; pass runId and nextCursor as runId/after. Include script for aggregate services.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"},"after":{"type":"integer","minimum":0},"runId":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}},

        read("workspace_list", "List projects in the active workspace (legacy array result)"),
        read("workspace_get", "Read active folders, file, projects, dirty state and revision. Read this before mutations."),
        read("workspace_refresh", "Refresh runnable application discovery from source and return the active workspace; does not run Maven or start services"),
        read("workspace_catalog", "List configured allowed roots and workspace files directly within them; no recursive scan"),
        {"name":"workspace_browse","description":"Browse one level of allowlisted folders and .code-workspace files before opening a workspace. Omit path to list allowed roots.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false},"annotations":{"readOnlyHint":true}},
        {"name":"workspace_create_from_folder","description":"Atomically create a workspace file containing an existing allowlisted folder; never overwrites files.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"folder":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"discardChanges":{"type":"boolean"}},"required":["path","folder","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}},
        {"name":"workspace_close","description":"Close the active workspace without stopping processes or deleting files. Dirty state requires explicit discardChanges=true; save first to retain changes.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"discardChanges":{"type":"boolean"}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}},
        {"name":"workspace_children","description":"List one directory within the active workspace; directories first, no recursion or dependency/build/Git directories. Read workspace_get for expectedRevision.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}},
        read("runtime_list", "Read observed process metrics for active workspace projects; no process operations"),
        {"name":"java_execute","description":"Start or stop a Java workspace project, or attach official Arthas to a selected application PID. JVM/application arguments are arrays of literal arguments, profiles are Spring profiles. Start compiles Maven runtime dependencies then launches the selected main class; poll java_output for progress.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"expectedRevision":{"type":"integer"},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"start"},"jvmArgs":{"type":"array","items":{"type":"string"}},"args":{"type":"array","items":{"type":"string"}},"profiles":{"type":"array","items":{"type":"string"}},"mainClass":{"type":"string"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"stop"},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"attach"},"pid":{"type":"integer"}},"required":["type","pid"],"additionalProperties":false}]}},"required":["projectPath","expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}},
        {"name":"java_options","description":"List detected Java main classes and Spring application profile files; additional profiles may be supplied explicitly.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true}},
        {"name":"java_output","description":"Read bounded stdout/stderr/build/Arthas output and run state. Pass returned runId and nextCursor as runId/after to continue; a new run resets the cursor.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"after":{"type":"integer","minimum":0},"runId":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true}},
        read("devflow_metrics", "Read DevFlow backend process CPU and physical memory only; excludes Codex/browser and managed projects. CPU is normalized to total logical CPU capacity; first sample is null."),
        write("workspace_open", "Open an allowlisted .code-workspace file or directory. Unsaved changes require explicit discardChanges=true.", true, false),
        write("workspace_create", "Create a new .code-workspace file and make it active; never overwrites an existing file", true, false),
        write("workspace_add_folder", "Add an existing allowlisted directory to the active workspace in memory; call workspace_save to persist", true, true),
        write("workspace_remove_folder", "Remove a root folder reference from the active workspace in memory; never deletes files or stops processes. Call workspace_save to persist.", true, false),
        write("workspace_save", "Save active workspace; optional path for Save As. Refuses overwriting unrelated or externally changed files.", false, false)
    ]})
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let Some(id) = request.get("id") else {
        return Err(StatusCode::ACCEPTED);
    };
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Ok(Json(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"Invalid Request"}}),
        ));
    }
    let params = request.get("params").cloned().unwrap_or(json!({}));
    let result: Result<Value, Value> = match method {
        "initialize" => Ok(
            json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"devflow","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools()),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            if name == "node_execute" {
                match serde_json::from_value::<devflow_core::runtime::node::NodeRequest>(arguments)
                {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| e.node_execute(request)).await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if matches!(name, "node_options" | "node_output") {
                let output = name == "node_output";
                match serde_json::from_value::<devflow_core::runtime::node::NodeQuery>(arguments) {
                    Ok(query) => Ok(tool_result(
                        call_core(state, move |e| {
                            let project = e.workspace.node_runtime_project(
                                &query.project_path,
                                query.script.as_deref(),
                                None,
                            )?;
                            if output {
                                Ok(json!(e.node.output(
                                    &project,
                                    query.after,
                                    query.run_id.as_deref()
                                )?))
                            } else {
                                Ok(json!(devflow_core::runtime::node::options(&project)?))
                            }
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "java_execute" {
                match serde_json::from_value::<devflow_core::runtime::java::JavaRequest>(arguments)
                {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| e.java_execute(request)).await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if matches!(name, "java_output" | "java_options") {
                let output = name == "java_output";
                match serde_json::from_value::<devflow_core::runtime::java::JavaQuery>(arguments) {
                    Ok(query) => Ok(tool_result(
                        call_core(state, move |e| {
                            let project = e.workspace.runtime_project(&query.project_path, None)?;
                            if output {
                                Ok(json!(e.java.output(
                                    &project,
                                    query.after,
                                    query.run_id.as_deref()
                                )?))
                            } else {
                                Ok(json!(devflow_core::runtime::java::options(&project)?))
                            }
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_browse" {
                match serde_json::from_value::<devflow_core::workspace::WorkspaceBrowseRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| e.workspace.browse(request)).await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if matches!(name, "workspace_close" | "workspace_create_from_folder")
                && (!arguments.is_object()
                    || arguments.as_object().is_some_and(|args| {
                        args.keys().any(|key| {
                            !matches!(key.as_str(), "expectedRevision" | "discardChanges")
                                && !(name == "workspace_create_from_folder"
                                    && matches!(key.as_str(), "path" | "folder"))
                        })
                    }))
            {
                Err(json!({"code":-32602,"message":"Unknown tool arguments"}))
            } else if matches!(name, "workspace_close" | "workspace_create_from_folder") {
                // Decode using the same strict operation DTO as the HTTP adapter.
                let mut operation = arguments.clone();
                if let Some(object) = operation.as_object_mut() {
                    object.remove("expectedRevision");
                    object.remove("discardChanges");
                    object.insert(
                        "type".into(),
                        json!(if name == "workspace_close" {
                            "close"
                        } else {
                            "create_from_folder"
                        }),
                    );
                }
                let request = json!({"expectedRevision":arguments.get("expectedRevision"),"discardChanges":arguments.get("discardChanges").cloned().unwrap_or(json!(false)),"operation":operation});
                match serde_json::from_value::<WorkspaceRequest>(request) {
                    Ok(request) => {
                        let authenticated = security::authenticated(&headers, &state);
                        Ok(tool_result(
                            call_core(state, move |e| e.workspace.execute(request, authenticated))
                                .await,
                        ))
                    }
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_children" {
                match serde_json::from_value::<WorkspaceChildrenRequest>(arguments) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| e.workspace.children(request)).await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if matches!(
                name,
                "workspace_list"
                    | "workspace_get"
                    | "workspace_refresh"
                    | "workspace_catalog"
                    | "runtime_list"
                    | "devflow_metrics"
            ) {
                if !arguments.is_object() || arguments.as_object().is_some_and(|a| !a.is_empty()) {
                    Err(json!({"code":-32602,"message":"This tool accepts no arguments"}))
                } else {
                    Ok(match name {
                        "workspace_list" => tool_result(
                            call_core(state, |e| Ok(e.workspace.snapshot()?.projects)).await,
                        ),
                        "workspace_get" => {
                            tool_result(call_core(state, |e| e.workspace.snapshot()).await)
                        }
                        "workspace_refresh" => {
                            tool_result(call_core(state, |e| e.workspace.refresh()).await)
                        }
                        "workspace_catalog" => {
                            tool_result(call_core(state, |e| e.workspace.catalog()).await)
                        }
                        "devflow_metrics" => tool_result(
                            call_core(state, |e| e.runtime.self_snapshot().map_err(Into::into))
                                .await,
                        ),
                        _ => tool_result(call_core(state, |e| e.runtime_snapshot()).await),
                    })
                }
            } else {
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Arguments {
                    expected_revision: u64,
                    path: Option<String>,
                    name: Option<String>,
                    #[serde(default)]
                    discard_changes: bool,
                }
                if !matches!(
                    name,
                    "workspace_open"
                        | "workspace_create"
                        | "workspace_add_folder"
                        | "workspace_remove_folder"
                        | "workspace_save"
                ) {
                    Err(json!({"code":-32602,"message":"Unknown tool"}))
                } else {
                    match serde_json::from_value::<Arguments>(arguments) {
                        Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                        Ok(args) if name != "workspace_add_folder" && args.name.is_some() => Err(
                            json!({"code":-32602,"message":"name is only valid for workspace_add_folder"}),
                        ),
                        Ok(args) => {
                            let operation = match (name, args.path) {
                                ("workspace_open", Some(path)) => {
                                    Some(WorkspaceOperation::Open { path })
                                }
                                ("workspace_create", Some(path)) => {
                                    Some(WorkspaceOperation::Create { path })
                                }
                                ("workspace_add_folder", Some(path)) => {
                                    Some(WorkspaceOperation::AddFolder {
                                        path,
                                        name: args.name,
                                    })
                                }
                                ("workspace_remove_folder", Some(path)) => {
                                    Some(WorkspaceOperation::RemoveFolder { path })
                                }
                                ("workspace_save", path) => Some(WorkspaceOperation::Save { path }),
                                _ => None,
                            };
                            if let Some(operation) = operation {
                                let authenticated = security::authenticated(&headers, &state);
                                let request = WorkspaceRequest {
                                    expected_revision: args.expected_revision,
                                    discard_changes: args.discard_changes,
                                    operation,
                                };
                                Ok(tool_result(
                                    call_core(state, move |e| {
                                        e.workspace.execute(request, authenticated)
                                    })
                                    .await,
                                ))
                            } else {
                                Err(json!({"code":-32602,"message":"path is required"}))
                            }
                        }
                    }
                }
            }
        }
        _ => Err(json!({"code":-32601,"message":"Method not found"})),
    };
    Ok(Json(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error}),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_tools_advertise_revision_and_required_paths() {
        let catalog = tools();
        let tools = catalog["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 21);
        for name in [
            "workspace_open",
            "workspace_create",
            "workspace_add_folder",
            "workspace_remove_folder",
            "workspace_save",
            "workspace_close",
            "workspace_create_from_folder",
            "java_execute",
            "node_execute",
        ] {
            let tool = tools.iter().find(|t| t["name"] == name).unwrap();
            assert!(
                tool["inputSchema"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("expectedRevision"))
            );
        }
    }
}
