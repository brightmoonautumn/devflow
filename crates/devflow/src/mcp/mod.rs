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
    let search_schema = json!({"type":"object","properties":{"query":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"mode":{"enum":["files","content","both"]},"caseSensitive":{"type":"boolean"},"wholeWord":{"type":"boolean"},"regex":{"type":"boolean"},"includeGlob":{"type":"string"},"excludeGlob":{"type":"string"}},"required":["query","expectedRevision"],"additionalProperties":false});
    let git_selection = json!({"type":"object","properties":{"repository":{"type":"string"},"version":{"type":"string"},"paths":{"type":"array","minItems":1,"maxItems":500,"items":{"type":"string"}}},"required":["repository","version","paths"],"additionalProperties":false});
    // Expand each schema independently; a single json! token-munching array
    // makes macro recursion grow with the total size of the tool catalog.
    let tools = vec![
        json!({"name":"workspace_terminal","description":"Manage local owner interactive PTY terminals in active workspace folders. Requires owner authentication and current revision. Operations list/create/read/input/resize/close use the same core as Web UI. Input executes with the local user shell privileges, not a filesystem sandbox. Switching or closing workspaces closes terminals; removing a folder closes its terminals. Save/add-folder retains remaining sessions. Output data is a byte array preserving ANSI and UTF-8; read incrementally with nextCursor, reset renderer when truncated. Maximum 12 sessions, 1 MiB retained per session.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"list"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"create"},"folder":{"type":"string"},"cols":{"type":"integer","minimum":2,"maximum":500},"rows":{"type":"integer","minimum":1,"maximum":300}},"required":["type","folder","cols","rows"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"read"},"id":{"type":"string"},"after":{"type":"integer","minimum":0}},"required":["type","id","after"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"input"},"id":{"type":"string"},"data":{"type":"string","maxLength":65536}},"required":["type","id","data"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"resize"},"id":{"type":"string"},"cols":{"type":"integer","minimum":2,"maximum":500},"rows":{"type":"integer","minimum":1,"maximum":300}},"required":["type","id","cols","rows"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"close"},"id":{"type":"string"}},"required":["type","id"],"additionalProperties":false}]}},"required":["expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_git_update_check","description":"Inspect local branch, upstream and clean-worktree readiness. No fetch. Pass version to update_start.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"}},"required":["expectedRevision","repository"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_update_start","description":"Start background fetch and merge of configured upstream. Requires clean index/worktree and check version; untracked collisions rejected by Git. Never pushes, stashes or rebases. Poll progress. Stop batches on conflicts/failure.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"}},"required":["expectedRevision","repository","version"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_git_update_progress","description":"Read current background update phase/result; optional jobId. Not a cancellation endpoint.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"jobId":{"type":"string"}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_merge_status","description":"Read active ordinary merge, conflict files, changed paths and exact snapshot version. Recover after restart. No writes.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"},"message":{"type":"string","maxLength":65536}},"required":["expectedRevision","repository"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_merge_file","description":"Read index stage 1/base, 2/local and 3/incoming plus working result for one active merge conflict. UTF-8 editor limit 1 MiB.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"},"message":{"type":"string","maxLength":65536}},"required":["expectedRevision","repository","path"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_merge_resolve","description":"Save one conflict resolution and stage that path only. Requires current merge version. edit content must have no conflict markers; local/incoming choose entire version; a missing side deletes. Special conflicts require external tools.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"},"choice":{"enum":["edit","local","incoming","delete"]},"content":{"type":"string","maxLength":1048576}},"required":["expectedRevision","repository","version","path","choice"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_git_merge_finish","description":"Finish merge with current version after all conflicts resolved. Optional commit message. Commits the merge index, not a normal partial commit; hooks/signing remain active.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"},"message":{"type":"string","maxLength":65536}},"required":["expectedRevision","repository","version"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_git_merge_abort","description":"Abort current merge after caller confirms loss of merge resolutions. Requires current version; extra unstaged edits block abort. Never reset --hard.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"},"message":{"type":"string","maxLength":65536}},"required":["expectedRevision","repository","version"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_share_create","description":"Create a read-only invitation for the current workspace, frozen to its included folders. Valid 5–1440 minutes, revoked on restart. Return token only once; attach to /share/#token on the dedicated LAN listener. Guests can read files/search/Git diff/logs, never mutate. Only local owner may create.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"lifetimeMinutes":{"type":"integer","minimum":5,"maximum":1440}},"required":["expectedRevision","lifetimeMinutes"],"additionalProperties":false},"annotations":{"readOnlyHint":false}}),
        read("workspace_share_list", "List active read-only invitations (no tokens). Local owner only."),
        json!({"name":"workspace_share_revoke","description":"Revoke an invitation by id. Local owner only. Subsequent guest reads fail, including responses still being prepared.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":false}}),
        json!({"name":"workspace_share_read","description":"Read through an invitation's fixed workspace permissions. Request type is workspace, children(path), file(path), search(query, optional mode/caseSensitive/wholeWord/regex/includeGlob/excludeGlob), runtime (scoped project metrics), metrics (backend process metrics), git_status, git_blame(path), git_diff(repository/version/path), git_history(repository, optional skip/limit/reference/query/version), git_history_overview(optional repositories/reference/query/limit/cursor), git_sync(optional repositories), git_history_commit(repository/commit, optional parent), git_history_diff(repository/commit/path, optional parent), or output(projectPath, optional script/after/runId). Unknown actions and fields are rejected in core. This local MCP adapter still requires owner transport authentication; LAN guests use /api/shared/read on the separate listener.","inputSchema":{"type":"object","properties":{"token":{"type":"string"},"request":{"type":"object"}},"required":["token","request"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_blame","description":"Read last author and author date per line for one workspace file/editor snapshot. At most 1 MiB and 10000 lines. Uncommitted lines have null commit. Does not load History details or write files.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"path":{"type":"string"},"content":{"type":"string"}},"required":["expectedRevision","path"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_history_overview","description":"Read a bounded multi-repository log. Empty repositories means all discovered authorized roots. Merge by commit date while preserving each root topology. Opaque nextCursor must be returned unchanged; root, filter or ref changes invalidate it. Every row includes its repository. No network.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repositories":{"type":"array","maxItems":64,"items":{"type":"string"}},"reference":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"string","maxLength":32768}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_sync","description":"Compare current branches to locally known upstream refs; no fetch or worktree scan. Returns ahead/behind counts, reasons, fetch preflight versions and last successful fetch time recorded in this session. Empty repositories means all discovered roots; no claim of server freshness.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repositories":{"type":"array","maxItems":64,"items":{"type":"string"}}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_fetch","description":"Fetch only the current branch configured upstream into its remote-tracking ref. Requires sync version, authentication and active workspace membership. Never merges, rebases, stashes, pushes or recursively fetches submodules. 120 second process timeout. Returns refreshed local upstream counts.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"version":{"type":"string"}},"required":["expectedRevision","repository","version"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false}}),
        json!({"name":"workspace_git_history","description":"Read bounded local commit history with parents and refs for a workspace repository. Paginate with skip/limit and version; reference is all, HEAD or an existing full ref. Search commit messages with literal query. No fetch or writes.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"skip":{"type":"integer","minimum":0,"maximum":10000},"limit":{"type":"integer","minimum":1,"maximum":100},"reference":{"type":"string"},"query":{"type":"string"},"version":{"type":"string"}},"required":["expectedRevision","repository"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_history_commit","description":"Read full commit message and up to 2000 changed files versus a direct parent (default first). Requires full commit SHA. Root commits compare with empty tree. No rename detection.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"commit":{"type":"string"},"parent":{"type":"string"}},"required":["expectedRevision","repository","commit"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_history_diff","description":"Read immutable before/after blobs for a file changed by this commit versus its direct parent. Historical diff never enables working-tree revert. UTF-8 text at most 1 MiB per side; gitlinks show SHAs.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"repository":{"type":"string"},"commit":{"type":"string"},"parent":{"type":"string"},"path":{"type":"string"}},"required":["expectedRevision","repository","commit","path"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_repositories","description":"Read the cached workspace repository/submodule catalog without scanning file changes. Optional force rebuilds discovery. Same scope and authentication as Git status.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"force":{"type":"boolean"}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_status","description":"Read cached workspace Git status. Stale snapshots refresh in the background (refreshing/stale flags); force requests a fresh bounded wait. Mutation validation never uses this cache. Parent submodule pointers and child repository files are separate groups with separate versions. Discovers initialized index gitlinks without recursively scanning .git/dependencies. Needs authentication and current workspace revision.","inputSchema":{"type":"object","properties":{"force":{"type":"boolean"},"expectedRevision":{"type":"integer","minimum":0}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_diff","description":"Read HEAD versus current disk content for a changed file, or old/new submodule commit IDs for a parent gitlink. Additions/deletions use an empty side. Binary/large files return an explanation. Requires the repository version from workspace_git_status.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer"},"repository":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"}},"required":["expectedRevision","repository","version","path"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_mutate","description":"Add selected untracked files to VCS or rollback selected tracked changes to HEAD. Rollback of newly added files unstages and preserves disk content. Never deletes untracked files or checks out child submodule HEADs. Caller must confirm rollback's listed paths. Returns per-repository successful paths/errors; batches are not atomic.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer"},"operation":{"enum":["add","rollback"]},"selections":{"type":"array","minItems":1,"maxItems":32,"items":git_selection.clone()}},"required":["expectedRevision","operation","selections"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_git_commit_preview","description":"Prepare local commits for exactly the checked tracked files, returning actual diff previews and a 5-minute planId. Requires a message and status versions. Preserves staged unchecked files. Does not allow a child commit and its parent's gitlink in one batch: commit child first, refresh, then select parent pointer. Never pushes.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer"},"message":{"type":"string","minLength":1},"selections":{"type":"array","minItems":1,"maxItems":32,"items":git_selection}},"required":["expectedRevision","message","selections"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_git_push","description":"Push expectedHead (full SHA) from the current branch to its configured upstream without force, tags or recursive submodules. Retains local commits on failure; requires authentication and workspace membership.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer"},"repository":{"type":"string"},"expectedHead":{"type":"string"}},"required":["expectedRevision","repository","expectedHead"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_git_commit_apply","description":"Apply an inspected local commit plan. Rechecks HEAD/index/disk snapshots and consumes plan. Git hooks/signing remain enabled. Returns commit IDs per repository; stops on error, previously created commits remain. Does not automatically commit parent pointers. Optional push=true pushes each successful commit to its configured upstream without force; reports pushError separately and retains local commits.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer"},"planId":{"type":"string"},"push":{"type":"boolean","default":false}},"required":["expectedRevision","planId"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_file_copy","description":"Copy an existing regular file within the active authorized workspace to a uniquely named sibling (副本). Copies disk bytes, never overwrites existing files or copies directories.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false}}),
        json!({"name":"workspace_file_delete","description":"Permanently delete one regular file within the active authorized workspace. Requires authentication and current revision; rejects directories, symlinks, excluded directories, readonly files and active workspace configuration.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_replace_preview","description":"Preview content replacements without writing. Pass search, replacement, optional preserveCase, optional path (one file), optional line/column (one occurrence). Requires complete bounded content search; filename matches are never renamed. Returns before/after samples and a server-side planId valid for 5 minutes; only the latest plan survives. Regex replacements support $&, $0..$99, $$, and escaped newline/tab/backslash.","inputSchema":{"type":"object","properties":{"search":search_schema,"replacement":{"type":"string"},"preserveCase":{"type":"boolean"},"path":{"type":"string"},"line":{"type":"integer","minimum":1},"column":{"type":"integer","minimum":1}},"required":["search","replacement"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        json!({"name":"workspace_replace_apply","description":"Apply an inspected replacement planId at its revision. Validates exact disk snapshots, workspace membership and readonly protection before writes. Consumes the plan; reports exact applied files and a failure on partial I/O errors. Batch replacement is not a filesystem transaction.","inputSchema":{"type":"object","properties":{"planId":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["planId","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"node_execute","description":"Start or stop an authorized Node service using a discovered package script, never an arbitrary command. For aggregate projects pass nodeScript from workspace_get as script. npm/pnpm/yarn are supported; dependencies are not installed automatically.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"start"},"packageManager":{"enum":["npm","pnpm","yarn"]},"args":{"type":"array","items":{"type":"string"}}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"stop"},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false}]}},"required":["projectPath","expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"node_options","description":"Read discovered launch scripts and preferred package manager for an active workspace Node service.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        json!({"name":"node_output","description":"Read bounded Node stdout/stderr and run state; pass runId and nextCursor as runId/after. Include script for aggregate services.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"script":{"type":"string"},"after":{"type":"integer","minimum":0},"runId":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),

        json!({"name":"workspace_file_read","description":"Read an existing UTF-8 text file up to 1 MiB inside active authorized workspace folders. Returns exact content for expectedContent when saving. Binary files and ignored directories are rejected.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_file_write","description":"Save an existing active workspace UTF-8 text file up to 1 MiB, preserving BOM and line endings. Pass exact content returned by workspace_file_read as expectedContent; external changes produce file_conflict. Does not create files or execute commands.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"expectedContent":{"type":"string"},"content":{"type":"string"}},"required":["path","expectedRevision","expectedContent","content"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_search","description":"Search filenames/relative paths and UTF-8 content together by default (mode=both); files/content modes remain available for compatibility. Uses ripgrep in active authorized workspace folders. Respects ignore files; excludes dependency/build/Git directories and symlinks. Bounded to 200 results and 5 seconds. Content results contain 1-based line/UTF-16 column and preview; searches disk, not unsaved drafts.","inputSchema":{"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":1024},"mode":{"enum":["files","content","both"],"default":"both"},"expectedRevision":{"type":"integer","minimum":0},"caseSensitive":{"type":"boolean"},"wholeWord":{"type":"boolean"},"regex":{"type":"boolean"},"includeGlob":{"type":"string","maxLength":1024},"excludeGlob":{"type":"string","maxLength":1024}},"required":["query","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false}}),
        read("workspace_list", "List projects in the active workspace (legacy array result)"),
        read("workspace_get", "Read active folders, file, projects, dirty state and revision. Read this before mutations."),
        read("workspace_refresh", "Refresh runnable application discovery from source and return the active workspace; does not run Maven or start services"),
        read("workspace_catalog", "List recentWorkspaces in most-recent-first order, configured allowed roots and workspace files directly within them; no recursive scan"),
        json!({"name":"workspace_pick","description":"Ask the local desktop user to choose a workspace file or folder in a native system dialog. Only the chosen location and folders declared by a chosen workspace gain session access. Cancellation returns a null path. Requires human interaction; may take up to 180 seconds.","inputSchema":{"type":"object","required":["kind","expectedRevision"],"properties":{"kind":{"enum":["workspace_file","folder"]},"expectedRevision":{"type":"integer","minimum":0}},"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false}}),
        json!({"name":"workspace_browse","description":"Browse one level of allowlisted folders and .code-workspace files before opening a workspace. Omit path to list allowed roots.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"workspace_create_from_folder","description":"Atomically create a workspace file containing an existing allowlisted folder; never overwrites files.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"folder":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"discardChanges":{"type":"boolean"}},"required":["path","folder","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_close","description":"Close the active workspace without stopping processes or deleting files. Dirty state requires explicit discardChanges=true; save first to retain changes.","inputSchema":{"type":"object","properties":{"expectedRevision":{"type":"integer","minimum":0},"discardChanges":{"type":"boolean"}},"required":["expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"workspace_children","description":"List one directory within the active workspace; directories first, no recursion or dependency/build/Git directories. Read workspace_get for expectedRevision.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0}},"required":["path","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        read("runtime_list", "Read observed process metrics for active workspace projects; no process operations"),
        json!({"name":"java_execute","description":"Start or stop a Java workspace project, or attach official Arthas to a selected application PID. JVM/application arguments are arrays of literal arguments, profiles are Spring profiles. Start compiles Maven runtime dependencies then launches the selected main class; poll java_output for progress.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"expectedRevision":{"type":"integer"},"operation":{"oneOf":[{"type":"object","properties":{"type":{"const":"start"},"jvmArgs":{"type":"array","items":{"type":"string"}},"args":{"type":"array","items":{"type":"string"}},"profiles":{"type":"array","items":{"type":"string"}},"mainClass":{"type":"string"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"stop"},"pid":{"type":"integer"}},"required":["type"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"attach"},"pid":{"type":"integer"}},"required":["type","pid"],"additionalProperties":false}]}},"required":["projectPath","expectedRevision","operation"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true}}),
        json!({"name":"java_options","description":"List detected Java main classes and Spring application profile files; additional profiles may be supplied explicitly.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        json!({"name":"java_output","description":"Read bounded stdout/stderr/build/Arthas output and run state. Pass returned runId and nextCursor as runId/after to continue; a new run resets the cursor.","inputSchema":{"type":"object","properties":{"projectPath":{"type":"string"},"after":{"type":"integer","minimum":0},"runId":{"type":"string"}},"required":["projectPath"],"additionalProperties":false},"annotations":{"readOnlyHint":true}}),
        read("devflow_metrics", "Read DevFlow backend process CPU and physical memory only; excludes Codex/browser and managed projects. CPU is normalized to total logical CPU capacity; first sample is null."),
        write("workspace_open_recent", "Reopen a path returned by workspace_catalog recentWorkspaces, restoring only that entry's remembered roots. Never grants access to arbitrary paths. Unsaved workspace changes require discardChanges=true.", true, false),
        write("workspace_open", "Open an allowlisted .code-workspace file or directory. Unsaved changes require explicit discardChanges=true.", true, false),
        write("workspace_create", "Create a new .code-workspace file and make it active; never overwrites an existing file", true, false),
        write("workspace_add_folder", "Add an existing allowlisted directory to the active workspace in memory; call workspace_save to persist", true, true),
        write("workspace_remove_folder", "Remove a root folder reference from the active workspace in memory; never deletes files or stops processes. Call workspace_save to persist.", true, false),
        write("workspace_save", "Save active workspace; optional path for Save As. Refuses overwriting unrelated or externally changed files.", false, false)
    ];
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
            if name == "workspace_terminal" {
                let authenticated = security::authenticated(&headers, &state);
                let result = call_core(state, move |engine| {
                    let request = serde_json::from_value(arguments).map_err(|err: serde_json::Error| WorkspaceError { code: "invalid_query", message: err.to_string() })?;
                    engine.workspace.terminal_execute(request, authenticated)
                }).await;
                Ok(tool_result(result))
            } else if matches!(name, "workspace_share_create" | "workspace_share_list" | "workspace_share_revoke" | "workspace_share_read") {
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
            } else if matches!(name, "workspace_git_update_check" | "workspace_git_update_start" | "workspace_git_update_progress" | "workspace_git_merge_status" | "workspace_git_merge_file" | "workspace_git_merge_resolve" | "workspace_git_merge_finish" | "workspace_git_merge_abort" | "workspace_git_repositories" | "workspace_git_blame" | "workspace_git_history" | "workspace_git_history_commit" | "workspace_git_history_diff" | "workspace_git_status" | "workspace_git_diff" | "workspace_git_mutate" | "workspace_git_commit_preview" | "workspace_git_commit_apply" | "workspace_git_push" | "workspace_git_history_overview" | "workspace_git_sync" | "workspace_git_fetch") {
                let authenticated = security::authenticated(&headers, &state);
                let name = name.to_owned();
                let result = call_core(state, move |engine| {
                    macro_rules! invoke {
                        ($ty:ty, $method:ident) => {{
                            let request = serde_json::from_value::<$ty>(arguments).map_err(|err| WorkspaceError { code: "invalid_query", message: err.to_string() })?;
                            Ok(json!(engine.workspace.$method(request, authenticated)?))
                        }};
                    }
                    match name.as_str() {
                        "workspace_git_update_check" => invoke!(devflow_core::workspace::GitUpdateRequest, git_update_check),
                        "workspace_git_update_start" => invoke!(devflow_core::workspace::GitUpdateRequest, git_update_start),
                        "workspace_git_update_progress" => invoke!(devflow_core::workspace::GitUpdateProgressRequest, git_update_progress),
                        "workspace_git_merge_status" => invoke!(devflow_core::workspace::GitMergeRequest, git_merge_status),
                        "workspace_git_merge_file" => invoke!(devflow_core::workspace::GitMergeRequest, git_merge_file),
                        "workspace_git_merge_resolve" => invoke!(devflow_core::workspace::GitResolveRequest, git_merge_resolve),
                        "workspace_git_merge_finish" => invoke!(devflow_core::workspace::GitMergeRequest, git_merge_finish),
                        "workspace_git_merge_abort" => invoke!(devflow_core::workspace::GitMergeRequest, git_merge_abort),
                        "workspace_git_blame" => invoke!(devflow_core::workspace::GitBlameRequest, git_blame),
                        "workspace_git_history" => invoke!(devflow_core::workspace::GitHistoryRequest, git_history),
                        "workspace_git_history_commit" => invoke!(devflow_core::workspace::GitHistoryCommitRequest, git_history_commit),
                        "workspace_git_history_diff" => invoke!(devflow_core::workspace::GitHistoryDiffRequest, git_history_diff),
                        "workspace_git_repositories" => invoke!(devflow_core::workspace::GitStatusRequest, git_repositories),
                        "workspace_git_status" => invoke!(devflow_core::workspace::GitStatusRequest, git_status),
                        "workspace_git_diff" => invoke!(devflow_core::workspace::GitDiffRequest, git_diff),
                        "workspace_git_mutate" => invoke!(devflow_core::workspace::GitMutationRequest, git_mutate),
                        "workspace_git_history_overview" => invoke!(devflow_core::workspace::GitHistoryOverviewRequest, git_history_overview),
                        "workspace_git_sync" => invoke!(devflow_core::workspace::GitSyncRequest, git_sync),
                        "workspace_git_fetch" => invoke!(devflow_core::workspace::GitFetchRequest, git_fetch),
                        "workspace_git_push" => invoke!(devflow_core::workspace::GitPushRequest, git_push),
                        "workspace_git_commit_preview" => invoke!(devflow_core::workspace::GitCommitRequest, git_commit_preview),
                        _ => invoke!(devflow_core::workspace::GitCommitApplyRequest, git_commit_apply),
                    }
                }).await;
                Ok(tool_result(result))
            } else if name == "workspace_replace_preview" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::ReplacePreviewRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.workspace.replace_preview(request, authenticated)).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_replace_apply" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::ReplaceApplyRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.workspace.replace_apply(request, authenticated)).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_search" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspaceSearchRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.workspace.search(request, authenticated)).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_file_copy" || name == "workspace_file_delete" {
                let authenticated = security::authenticated(&headers, &state);
                let copy = name == "workspace_file_copy";
                match serde_json::from_value::<devflow_core::workspace::WorkspaceFileReadRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| {
                        if copy { e.workspace.copy_file(request, authenticated) }
                        else { e.workspace.delete_file(request, authenticated) }
                    }).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_file_read" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspaceFileReadRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.workspace.read_file(request, authenticated)).await)),
                    Err(err) => Err(json!({"code":-32602,"message":err.to_string()})),
                }
            } else if name == "workspace_file_write" {
                let authenticated = security::authenticated(&headers, &state);
                match serde_json::from_value::<devflow_core::workspace::WorkspaceFileWriteRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.workspace.write_file(request, authenticated)).await)),
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
                match serde_json::from_value::<devflow_core::workspace::WorkspacePickRequest>(arguments) {
                    Ok(request) => Ok(tool_result(call_core(state, move |e| e.workspace.pick_local(request, authenticated)).await)),
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
    fn workspace_tools_advertise_revision_and_required_paths() {
        let catalog = tools();
        let tools = catalog["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 57);
        for name in [
            "workspace_terminal",
            "workspace_git_update_check",
            "workspace_git_update_start",
            "workspace_git_update_progress",
            "workspace_git_merge_status",
            "workspace_git_merge_file",
            "workspace_git_merge_resolve",
            "workspace_git_merge_finish",
            "workspace_git_merge_abort",
            "workspace_share_create",
            "workspace_git_blame",
            "workspace_git_history",
            "workspace_git_history_commit",
            "workspace_git_history_diff",
            "workspace_git_repositories",
            "workspace_git_status",
            "workspace_git_diff",
            "workspace_git_mutate",
            "workspace_git_commit_preview",
            "workspace_git_commit_apply",
            "workspace_git_push",
            "workspace_git_history_overview",
            "workspace_git_sync",
            "workspace_git_fetch",
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
