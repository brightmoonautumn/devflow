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

mod board;
mod diagnosis;

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
    let search_schema = json!({"type":"object","properties":{"query":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"mode":{"enum":["files","content","both"]},"caseSensitive":{"type":"boolean"},"wholeWord":{"type":"boolean"},"regex":{"type":"boolean"},"includeGlob":{"type":"string"},"excludeGlob":{"type":"string"}},"required":["query","expectedRevision"],"additionalProperties":false});
    // Expand each schema independently; a single json! token-munching array
    // makes macro recursion grow with the total size of the tool catalog.
    let mut tools = vec![
        json!({"name":"runtime_diagnostics","description":"Export bounded, best-effort credential-redacted troubleshooting context for a Java/Node project in the active workspace. Returns a short Agent handoff in text and a separate redacted log excerpt. Pass the current runId to reject replaced runs; include script for Node aggregate services. Does not execute commands or restart services.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"kind":{"enum":["java","node"]},"script":{"type":"string"},"runId":{"type":"string"}},"required":["projectPath","kind"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        board::read_tool(),
        board::write_tool(),
        json!({"name":"workspace_terminal","description":"Manage local owner interactive PTY terminals in active workspace folders. Requires owner authentication and current revision. Operations list/create/read/input/resize/close use the same core as Web UI. Input executes with the local user shell privileges, not a filesystem sandbox. Switching or closing workspaces closes terminals; removing a folder closes its terminals. Save/add-folder retains remaining sessions. Output data is a byte array preserving ANSI and UTF-8; read incrementally with nextCursor, reset renderer when truncated. Maximum 12 sessions, 1 MiB retained per session.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"list"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"create"},"folder":{"type":"string"},"cols":{"type":"integer","minimum":2,"maximum":500},"rows":{"type":"integer","minimum":1,"maximum":300}},"required":["type","folder","cols","rows"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"read"},"id":{"type":"string"},"after":{"type":"integer","minimum":0}},"required":["type","id","after"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"input"},"id":{"type":"string"},"data":{"type":"string","maxLength":65536}},"required":["type","id","data"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"resize"},"id":{"type":"string"},"cols":{"type":"integer","minimum":2,"maximum":500},"rows":{"type":"integer","minimum":1,"maximum":300}},"required":["type","id","cols","rows"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"close"},"id":{"type":"string"}},"required":["type","id"],"additionalProperties":false}]}},"required":["expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_share_create","description":"Create a read-only invitation for the current workspace, frozen to its included folders. Valid 5–1440 minutes, revoked on restart. Return token only once; attach to /share/#token on the dedicated LAN listener. Guests can read files/search/logs, never mutate. Only local owner may create.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"lifetimeMinutes":{"type":"integer","minimum":5,"maximum":1440}},"required":["expectedRevision","lifetimeMinutes"],"additionalProperties":false},"annotations":{"readOnlyHint":false}}),
        read(
            "workspace_share_list",
            "List active read-only invitations (no tokens). Local owner only.",
        ),
        json!({"name":"workspace_share_revoke","description":"Revoke an invitation by id. Local owner only. Subsequent guest reads fail, including responses still being prepared.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":false}}),
        json!({"name":"workspace_share_read","description":"Read through an invitation's fixed workspace permissions. Request type is workspace, children(path), file(path), search(query, optional mode/caseSensitive/wholeWord/regex/includeGlob/excludeGlob), runtime (scoped project metrics), metrics (backend process metrics), or output(projectPath, optional script/after/runId). Unknown actions and fields are rejected in core. This local MCP adapter still requires owner transport authentication; LAN guests use /api/shared/read on the separate listener.","inputSchema":{"type":"object","properties":{"token":{"type":"string"},"request":{"type":"object"}},"required":["token","request"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_file_copy","description":"Copy an existing regular file within the active authorized workspace to a uniquely named sibling (副本). Copies disk bytes, never overwrites existing files or copies directories.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false}}),
        json!({"name":"workspace_file_delete","description":"Permanently delete a regular file or recursively delete a subdirectory and all its contents within the active authorized workspace. Requires authentication and current revision; rejects workspace roots, ancestors of workspace roots or protected configuration, symlink/reparse targets, excluded target paths and readonly targets. Child links are not followed. Recursive deletion may partially succeed before an I/O error.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_replace_preview","description":"Preview content replacements without writing. Pass search, replacement, optional preserveCase, optional path (one file), optional line/column (one occurrence). Requires complete bounded content search; filename matches are never renamed. Returns before/after samples and a server-side planId valid for 5 minutes; only the latest plan survives. Regex replacements support $&, $0..$99, $$, and escaped newline/tab/backslash.","inputSchema":{"type":"object","properties":{"search":search_schema,"replacement":{"type":"string"},"preserveCase":{"type":"boolean"},"path":{"type":"string"},"line":{"type":"integer","minimum":1},"column":{"type":"integer","minimum":1}},"required":["search","replacement"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        json!({"name":"workspace_replace_apply","description":"Apply an inspected replacement planId at its revision. Validates exact disk snapshots, workspace membership and readonly protection before writes. Consumes the plan; reports exact applied files and a failure on partial I/O errors. Batch replacement is not a filesystem transaction.","inputSchema":{"type":"object","properties":{"planId":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["planId","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"node_execute","description":"Start, restart or stop an authorized Node service using a discovered package script, never an arbitrary command. For aggregate projects pass nodeScript from workspace_get as script. npm/pnpm/yarn are supported; dependencies are not installed automatically. Restart validates launch options before stopping and waits for process-tree cleanup; pass the same launch options as start. External restart requires a single unambiguous service PID.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"start"},"packageManager":{"enum":["npm","pnpm","yarn"]},"args":{"type":"array","items":{"type":"string"}}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"restart"},"packageManager":{"enum":["npm","pnpm","yarn"]},"args":{"type":"array","items":{"type":"string"}},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"stop"},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false}]}},"required":["projectPath","expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"node_options","description":"Read discovered launch scripts and preferred package manager for an active workspace Node service.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        json!({"name":"node_output","description":"Read bounded Node stdout/stderr and run state; pass runId and nextCursor as runId/after. Include script for aggregate services.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"},"after":{"type":"integer","minimum":0},"runId":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        json!({"name":"workspace_file_read","description":"Read an existing UTF-8 text file up to 1 MiB inside active authorized workspace folders. Returns exact content for expectedContent when saving. Binary files and ignored directories are rejected.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_file_write","description":"Save an existing active workspace UTF-8 text file up to 1 MiB, preserving BOM and line endings. Pass exact content returned by workspace_file_read as expectedContent; external changes produce file_conflict. Does not create files or execute commands.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"expectedContent":{"type":"string"},"content":{"type":"string"}},"required":["path","expectedRevision","expectedContent","content"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_search","description":"Search filenames/relative paths and UTF-8 content together by default (mode=both); files/content modes remain available for compatibility. Uses ripgrep in active authorized workspace folders. Respects ignore files; excludes dependency/build/metadata directories and symlinks. Bounded to 200 results and 5 seconds. Content results contain 1-based line/UTF-16 column and preview; searches disk, not unsaved drafts.","inputSchema":{"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":1024},"mode":{"enum":["files","content","both"],"default":"both"},"expectedRevision":{"type":"integer","minimum":0},"caseSensitive":{"type":"boolean"},"wholeWord":{"type":"boolean"},"regex":{"type":"boolean"},"includeGlob":{"type":"string","maxLength":1024},"excludeGlob":{"type":"string","maxLength":1024}},"required":["query","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        read(
            "workspace_list",
            "List projects in the active workspace (legacy array result)",
        ),
        read(
            "workspace_get",
            "Read active folders, file, projects, dirty state and revision. Read this before mutations.",
        ),
        read(
            "workspace_refresh",
            "Refresh runnable application discovery from source and return the active workspace; does not run Maven or start services",
        ),
        read(
            "workspace_catalog",
            "List recentWorkspaces in most-recent-first order, configured allowed roots and workspace files directly within them; no recursive scan",
        ),
        json!({"name":"workspace_pick","description":"Ask the local desktop user to choose a workspace file or folder in a native system dialog. Only the chosen location and folders declared by a chosen workspace gain session access. Cancellation returns a null path. Requires human interaction; may take up to 180 seconds. Supply a unique UUID requestId to allow workspace_pick_cancel.","inputSchema":{"type":"object","required":["kind","expectedRevision"],"properties":{"kind":{"enum":["workspace_file","folder"]},"expectedRevision":{"type":"integer","minimum":0},"requestId":{"type":"string","format":"uuid"}},"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false}}),
        json!({"name":"workspace_pick_cancel","description":"Cancel only the active native picker with the matching requestId. Requires authentication. A cancelled selection grants no access. Returns conflict if the request has not started or has already ended.","inputSchema":{"type":"object","required":["requestId"],"properties":{"requestId":{"type":"string","format":"uuid"}},"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false}}),
        json!({"name":"workspace_browse","description":"Browse one level of allowlisted folders and .code-workspace files before opening a workspace. Omit path to list allowed roots.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_create_from_folder","description":"Atomically create a workspace file containing an existing allowlisted folder; never overwrites files.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"folder":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"discardChanges":{"type":"boolean"}},"required":["path","folder","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_close","description":"Close the active workspace without stopping processes or deleting files. Dirty state requires explicit discardChanges=true; save first to retain changes.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"discardChanges":{"type":"boolean"}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_children","description":"List one directory within the active workspace; directories first, no recursion or dependency/build/metadata directories. Read workspace_get for expectedRevision.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        read(
            "runtime_list",
            "Read observed process metrics for active workspace projects; no process operations",
        ),
        json!({"name":"java_execute","description":"Start, restart or stop a Java workspace project, or attach official Arthas to a selected application PID. JVM/application arguments are arrays of literal arguments, profiles are Spring profiles. Start checks source/configuration/output fingerprints: unchanged launches skip Maven; changed modules and affected upstream-chain consumers build together using Maven models and workspace outputs. buildMode=install enables packaging/install compatibility; mavenProfiles are independent of Spring profiles; forceBuild bypasses the cache. Requires Maven 3.6+ and a full JDK 11+. Poll java_output for cache/build progress. Restart validates launch options before stopping and waits for process-tree cleanup; pass the same launch options as start. External restart requires a single unambiguous service PID.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"expectedRevision":{"type":"integer"},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"start"},"jvmArgs":{"type":"array","items":{"type":"string"}},"args":{"type":"array","items":{"type":"string"}},"profiles":{"type":"array","items":{"type":"string"}},"mainClass":{"type":"string"},"mavenProfiles":{"type":"array","items":{"type":"string"},"maxItems":16},"buildMode":{"enum":["workspace","install"],"default":"workspace"},"forceBuild":{"type":"boolean","default":false}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"restart"},"jvmArgs":{"type":"array","items":{"type":"string"}},"args":{"type":"array","items":{"type":"string"}},"profiles":{"type":"array","items":{"type":"string"}},"mainClass":{"type":"string"},"mavenProfiles":{"type":"array","items":{"type":"string"},"maxItems":16},"buildMode":{"enum":["workspace","install"],"default":"workspace"},"forceBuild":{"type":"boolean","default":false},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"stop"},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"attach"},"pid":{"type":"integer"}},"required":["type","pid"],"additionalProperties":false}]}},"required":["projectPath","expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"java_options","description":"List detected Java main classes and Spring application profile files; additional profiles may be supplied explicitly.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"java_output","description":"Read bounded stdout/stderr/build/Arthas output and run state. Pass returned runId and nextCursor as runId/after to continue; a new run resets the cursor.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"after":{"type":"integer","minimum":0},"runId":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        read(
            "devflow_metrics",
            "Read DevFlow backend process CPU and physical memory only; excludes Codex/browser and managed projects. CPU is normalized to total logical CPU capacity; first sample is null.",
        ),
        write(
            "workspace_open_recent",
            "Reopen a path returned by workspace_catalog recentWorkspaces, restoring only that entry's remembered roots. Never grants access to arbitrary paths. Unsaved workspace changes require discardChanges=true.",
            true,
            false,
        ),
        write(
            "workspace_open",
            "Open an allowlisted .code-workspace file or directory. Unsaved changes require explicit discardChanges=true.",
            true,
            false,
        ),
        write(
            "workspace_create",
            "Create a new .code-workspace file and make it active; never overwrites an existing file",
            true,
            false,
        ),
        write(
            "workspace_add_folder",
            "Add an existing allowlisted directory to the active workspace. Automatically saves when a .code-workspace file is open; failure leaves membership unchanged. Folder-only sessions require workspace_save with a chosen file path.",
            true,
            true,
        ),
        write(
            "workspace_remove_folder",
            "Remove a root folder reference; never deletes files or stops project processes. Automatically saves to the open .code-workspace file before applying membership changes; failure leaves membership unchanged. Folder-only sessions require workspace_save with a chosen file path.",
            true,
            false,
        ),
        write(
            "workspace_save",
            "Save active workspace; optional path for Save As. Refuses overwriting unrelated or externally changed files.",
            false,
            false,
        ),
    ];
    tools.extend(diagnosis::tools());
    json!({"tools": tools})
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
            if name == "board_get" || name == "board_execute" {
                let authenticated = security::authenticated(&headers, &state);
                Ok(tool_result(board::execute(state, name, arguments, authenticated).await))
            } else if name == "workspace_terminal" {
                let authenticated = security::authenticated(&headers, &state);
                let result = call_core(state, move |engine| {
                    let request =
                        serde_json::from_value(arguments).map_err(|err: serde_json::Error| {
                            WorkspaceError {
                                code: "invalid_query",
                                message: err.to_string(),
                            }
                        })?;
                    engine.workspace.terminal_execute(request, authenticated)
                })
                .await;
                Ok(tool_result(result))
            } else if matches!(
                name,
                "workspace_share_create"
                    | "workspace_share_list"
                    | "workspace_share_revoke"
                    | "workspace_share_read"
            ) {
                let authenticated = security::authenticated(&headers, &state);
                let network = state.share_network.clone();
                let name = name.to_owned();
                let result = call_core(state, move |engine| {
                    let invalid = |err: serde_json::Error| WorkspaceError { code: "invalid_query", message: err.to_string() };
                    if !authenticated { return Err(WorkspaceError { code: "unauthorized", message: "需要本机管理令牌".into() }); }
                    match name.as_str() {
                        "workspace_share_list" => Ok(json!({"network":network,"invitations":engine.workspace.share_list(true)?})),
                        "workspace_share_create" => {
                            if network.address.is_none() { return Err(WorkspaceError { code: "unavailable", message: "分享端口未启动".into() }); }
                            Ok(json!(engine.workspace.share_create(serde_json::from_value(arguments).map_err(invalid)?, true)?))
                        }
                        "workspace_share_revoke" => engine.workspace.share_revoke(serde_json::from_value(arguments).map_err(invalid)?, true),
                        _ => {
                            #[derive(serde::Deserialize)]
                            #[serde(deny_unknown_fields)]
                            struct Read { token: String, request: devflow_core::workspace::ShareReadRequest }
                            let request: Read = serde_json::from_value(arguments).map_err(invalid)?;
                            engine.share_read(&request.token, request.request)
                        }
                    }
                }).await;
                Ok(tool_result(result))
            } else if name == "workspace_replace_preview" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::ReplacePreviewRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| {
                            e.workspace.replace_preview(request, authenticated)
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_replace_apply" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::ReplaceApplyRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| {
                            e.workspace.replace_apply(request, authenticated)
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_search" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspaceSearchRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| e.workspace.search(request, authenticated)).await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_file_copy" || name == "workspace_file_delete" {
                let authenticated = security::authenticated(&headers, &state);
                let copy = name == "workspace_file_copy";
                match serde_json::from_value::<devflow_core::workspace::WorkspaceFileReadRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| {
                            if copy {
                                e.workspace.copy_file(request, authenticated)
                            } else {
                                e.workspace.delete_file(request, authenticated)
                            }
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_file_read" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspaceFileReadRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| {
                            e.workspace.read_file(request, authenticated)
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_file_write" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspaceFileWriteRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| {
                            e.workspace.write_file(request, authenticated)
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "diagnosis_get" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::runtime::diagnosis::DiagnosisQuery>(arguments) {
                    Ok(query) => Ok(tool_result(call_core(state, move |e| e.diagnosis_get(query, authenticated)).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "diagnosis_execute" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::runtime::diagnosis::DiagnosisRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.diagnosis_execute(request, authenticated)).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "runtime_diagnostics" {
                match serde_json::from_value::<devflow_core::runtime::diagnostics::DiagnosticsQuery>(arguments) {
                    Ok(query) => Ok(tool_result(call_core(state, move |e| e.runtime_diagnostics(query)).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "node_execute" {
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
            } else if name == "workspace_pick" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspacePickRequest>(
                    arguments,
                ) {
                    Ok(request) => Ok(tool_result(
                        call_core(state, move |e| {
                            e.workspace.pick_local(request, authenticated)
                        })
                        .await,
                    )),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_pick_cancel" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspacePickCancelRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.workspace.cancel_local_pick(request, authenticated)).await)),
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
                        | "workspace_open_recent"
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
                                ("workspace_open_recent", Some(path)) => {
                                    Some(WorkspaceOperation::OpenRecent { path })
                                }
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
    fn diagnostics_tool_is_read_only_and_identifies_node_services_and_runs() {
        let catalog = tools();
        let tool = catalog["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "runtime_diagnostics").unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        for property in ["projectPath", "kind", "script", "runId"] { assert!(tool["inputSchema"]["properties"][property].is_object()); }
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    #[test]
    fn runtime_tools_advertise_restart_with_launch_options() {
        let catalog = tools();
        for (name, option) in [
            ("java_execute", "mainClass"),
            ("node_execute", "packageManager"),
        ] {
            let tool = catalog["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap();
            let restart = tool["inputSchema"]["properties"]["operation"]["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .find(|variant| variant["properties"]["type"]["const"] == "restart")
                .unwrap();
            assert!(restart["properties"].get(option).is_some());
            assert!(restart["properties"].get("pid").is_some());
            assert_eq!(restart["additionalProperties"], false);
        }
    }
    #[test]
    fn workspace_tools_advertise_revision_and_required_paths() {
        let catalog = tools();
        let tools = catalog["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 40);
        for name in [
            "board_get",
            "workspace_terminal",
            "workspace_share_create",
            "workspace_replace_apply",
            "workspace_search",
            "workspace_file_read",
            "workspace_file_write",
            "workspace_file_copy",
            "workspace_file_delete",
            "workspace_pick",
            "workspace_open_recent",
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
