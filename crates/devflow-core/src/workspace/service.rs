//! VS Code-compatible local workspaces. JSONC syntax/editing is provided by
//! dprint/jsonc-parser (MIT); path/name/URI semantics follow microsoft/vscode's
//! src/vs/platform/workspaces/common/workspaces.ts. No VS Code code is vendored.
use super::{DetectedProject, Discovery, discover_services};
use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstRootNode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FOLDERS: usize = 512;

#[path = "files.rs"]
mod files;
pub use files::{WorkspaceFile, WorkspaceFileMutation, WorkspaceFileReadRequest, WorkspaceFileWriteRequest};
#[path = "search.rs"]
mod search;
pub use search::{WorkspaceSearchRequest, WorkspaceSearch};
#[path = "replace.rs"]
mod replace;
pub use replace::{ReplacePreviewRequest, ReplaceApplyRequest, ReplacePreview, ReplaceOutcome};
#[path = "git/mod.rs"]
mod git;
pub use git::{GitStatusRequest, GitDiffRequest, GitMutationRequest, GitCommitRequest, GitCommitApplyRequest, GitStatus, GitDiff, GitOutcome, GitCommitPreview};
#[path = "recent.rs"]
mod recent;
pub use recent::RecentWorkspaceEntry;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceError {
    pub code: &'static str,
    pub message: String,
}
impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for WorkspaceError {}
impl From<std::io::Error> for WorkspaceError {
    fn from(value: std::io::Error) -> Self {
        let code = match value.kind() {
            std::io::ErrorKind::NotFound => "not_found",
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            _ => "io_error",
        };
        error(code, value.to_string())
    }
}
fn error(code: &'static str, message: impl Into<String>) -> WorkspaceError {
    WorkspaceError {
        code,
        message: message.into(),
    }
}
type Result<T> = std::result::Result<T, WorkspaceError>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFolder {
    pub name: String,
    pub path: String,
    pub status: String,
    pub message: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    // Retained for existing HTTP clients. Multi-root clients use folders.
    pub workspace_root: Option<String>,
    pub workspace_file: Option<String>,
    pub name: String,
    pub revision: u64,
    pub dirty: bool,
    pub folders: Vec<WorkspaceFolder>,
    pub projects: Vec<DetectedProject>,
    pub warnings: Vec<String>,
    pub hint: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCatalog {
    pub allowed_roots: Vec<String>,
    pub workspace_files: Vec<String>,
    pub warnings: Vec<String>,
    pub recent_workspaces: Vec<RecentWorkspaceEntry>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTreeEntry {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChildren {
    pub path: String,
    pub revision: u64,
    pub entries: Vec<WorkspaceTreeEntry>,
    pub truncated: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceChildrenRequest {
    pub path: String,
    pub expected_revision: u64,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceBrowseRequest {
    pub path: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBrowse {
    pub path: Option<String>,
    pub parent: Option<String>,
    pub entries: Vec<WorkspaceTreeEntry>,
    pub truncated: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    pub expected_revision: u64,
    #[serde(default)]
    pub discard_changes: bool,
    pub operation: WorkspaceOperation,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceOperation {
    Open { path: String },
    OpenRecent { path: String },
    Create { path: String },
    CreateFromFolder { path: String, folder: String },
    Close,
    AddFolder { path: String, name: Option<String> },
    RemoveFolder { path: String },
    Save { path: Option<String> },
}

struct Session {
    file: Option<PathBuf>,
    base: PathBuf,
    source: String,
    saved_source: Option<String>,
    dirty: bool,
}
#[derive(Default)]
struct State {
    revision: u64,
    session: Option<Session>,
    notice: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePickKind {
    WorkspaceFile,
    Folder,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspacePickRequest {
    pub kind: WorkspacePickKind,
    pub expected_revision: u64,
}
#[derive(Debug, Serialize)]
pub struct WorkspacePickResult {
    pub path: Option<String>,
}
#[derive(Default)]
struct LocalGrants {
    roots: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

/// One instance is shared by HTTP and MCP; native selections grant session access.
pub struct WorkspaceService {
    roots: Vec<PathBuf>,
    local_grants: Mutex<LocalGrants>,
    picker: Mutex<()>,
    state: Mutex<State>,
    discovery: Mutex<HashMap<PathBuf, (Instant, Discovery)>>,
    recent_path: Mutex<Option<PathBuf>>,
    replacement: Mutex<Option<replace::ReplacementPlan>>,
    git_operations: Mutex<()>,
    git_plan: Mutex<Option<git::CommitPlan>>,
}

impl WorkspaceService {
    pub fn new(roots: Vec<PathBuf>, initial: Option<PathBuf>) -> Result<Self> {
        let mut canonical = Vec::new();
        for root in roots {
            let root = root.canonicalize()?;
            if !root.is_dir() {
                return Err(error("invalid_path", "授权根必须是目录"));
            }
            if !canonical.contains(&root) {
                canonical.push(root);
            }
        }
        let service = Self {
            roots: canonical,
            local_grants: Mutex::new(LocalGrants::default()),
            picker: Mutex::new(()),
            state: Mutex::new(State::default()),
            discovery: Mutex::new(HashMap::new()),
            recent_path: Mutex::new(None),
            replacement: Mutex::new(None),
            git_operations: Mutex::new(()),
            git_plan: Mutex::new(None),
        };
        if let Some(path) = initial {
            let session = service.open_session(&path)?;
            service.lock()?.session = Some(session);
        }
        Ok(service)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| error("unavailable", "工作区服务暂不可用"))
    }

    fn allowed_roots(&self) -> Result<Vec<PathBuf>> {
        let grants = self.local_grants.lock().map_err(|_| error("unavailable", "本机位置暂不可用"))?;
        let mut roots = self.roots.clone();
        roots.extend(grants.roots.iter().cloned());
        roots.sort();
        roots.dedup();
        Ok(roots)
    }

    pub fn pick_local(&self, request: WorkspacePickRequest, authenticated: bool) -> Result<WorkspacePickResult> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let _picker = self.picker.try_lock().map_err(|_| error("conflict", "已有一个系统选择窗口，请先完成选择"))?;
        if self.lock()?.revision != request.expected_revision {
            return Err(error("conflict", "工作区已修改，请刷新后重试"));
        }
        let selection = super::picker::pick(request.kind)?;
        self.accept_local_selection(request, selection)
    }

    // Only the trusted OS picker calls this; no client-supplied path can grant access.
    fn accept_local_selection(&self, request: WorkspacePickRequest, selection: Option<String>) -> Result<WorkspacePickResult> {
        let state = self.lock()?;
        if state.revision != request.expected_revision {
            return Err(error("conflict", "选择期间工作区已修改，请刷新后重新选择"));
        }
        let Some(selected) = selection else { return Ok(WorkspacePickResult { path: None }); };
        let selected = Path::new(&selected);
        validate_absolute(selected)?;
        let selected = selected.canonicalize()?;
        let mut roots = Vec::new();
        match request.kind {
            WorkspacePickKind::Folder => {
                if !selected.is_dir() { return Err(error("invalid_path", "请选择存在的文件夹")); }
                roots.push(selected.clone());
            }
            WorkspacePickKind::WorkspaceFile => {
                if !is_workspace_file(&selected) || !selected.is_file() {
                    return Err(error("invalid_path", "请选择 .code-workspace 文件"));
                }
                let source = read_workspace(&selected)?;
                let value = parse_document(&source)?;
                // Choosing a workspace also chooses its explicitly declared local folders.
                for entry in folder_entries(&value)? {
                    if let Ok(folder) = resolve_folder(entry, parent(&selected)?) {
                        if validate_absolute(&folder).is_ok() {
                            if let Ok(folder) = folder.canonicalize() {
                                if folder.is_dir() { roots.push(folder); }
                            }
                        }
                    }
                }
            }
        }
        let mut grants = self.local_grants.lock().map_err(|_| error("unavailable", "本机位置暂不可用"))?;
        for root in roots { if !grants.roots.contains(&root) { grants.roots.push(root); } }
        if matches!(request.kind, WorkspacePickKind::WorkspaceFile) && !grants.files.contains(&selected) {
            grants.files.push(selected.clone());
        }
        Ok(WorkspacePickResult { path: Some(display_path(&selected)) })
    }

    pub fn snapshot(&self) -> Result<WorkspaceSnapshot> {
        let state = self.lock()?;
        self.snapshot_of(&state)
    }
    pub fn runtime_project(&self, path: &str, revision: Option<u64>) -> Result<DetectedProject> {
        let state = self.lock()?;
        if revision.is_some_and(|value| value != state.revision) {
            return Err(error("conflict", "工作区已改变，请重新选择服务"));
        }
        let path = self.authorize(Path::new(path))?;
        self.snapshot_of(&state)?
            .projects
            .into_iter()
            .find(|project| {
                Path::new(&project.path)
                    .canonicalize()
                    .is_ok_and(|candidate| candidate == path)
            })
            .ok_or_else(|| error("outside_workspace", "服务不在当前工作区或没有可运行入口"))
    }
    pub fn refresh(&self) -> Result<WorkspaceSnapshot> {
        self.discovery
            .lock()
            .map_err(|_| error("unavailable", "服务发现缓存暂不可用"))?
            .clear();
        self.snapshot()
    }
    pub fn node_runtime_project(
        &self,
        path: &str,
        script: Option<&str>,
        revision: Option<u64>,
    ) -> Result<DetectedProject> {
        let state = self.lock()?;
        if revision.is_some_and(|value| value != state.revision) {
            return Err(error("conflict", "工作区已改变，请重新选择服务"));
        }
        let path = self.authorize(Path::new(path))?;
        let candidates: Vec<_> = self
            .snapshot_of(&state)?
            .projects
            .into_iter()
            .filter(|project| {
                !project.launch_scripts.is_empty()
                    && Path::new(&project.path)
                        .canonicalize()
                        .is_ok_and(|p| p == path)
                    && project
                        .node_script
                        .as_deref()
                        .is_none_or(|selected| script == Some(selected))
            })
            .collect();
        if candidates.len() != 1 {
            return Err(error(
                "outside_workspace",
                "服务不在当前工作区，或需要指定聚合工程的启动脚本",
            ));
        }
        Ok(candidates.into_iter().next().unwrap())
    }
    fn services_in(&self, root: &Path) -> Result<Discovery> {
        let mut cache = self
            .discovery
            .lock()
            .map_err(|_| error("unavailable", "服务发现缓存暂不可用"))?;
        if let Some((sampled, result)) = cache.get(root) {
            if sampled.elapsed() < Duration::from_secs(30) {
                return Ok(result.clone());
            }
        }
        let result = discover_services(root)?;
        if cache.len() >= 512 {
            cache.clear();
        }
        cache.insert(root.to_owned(), (Instant::now(), result.clone()));
        Ok(result)
    }

    /// Enumerate exactly one directory in the active workspace. Never recurse or
    /// follow child links; both HTTP and MCP use this same bounded operation.
    pub fn children(&self, request: WorkspaceChildrenRequest) -> Result<WorkspaceChildren> {
        let state = self.lock()?;
        if state.revision != request.expected_revision {
            return Err(error("conflict", "工作区已改变，请刷新目录树"));
        }
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
        let path = self.authorize(Path::new(&request.path))?;
        let document = parse_document(&session.source)?;
        let in_workspace = folder_entries(&document)?
            .iter()
            .filter_map(|entry| {
                resolve_folder(entry, &session.base)
                    .ok()
                    .and_then(|p| self.authorize(&p).ok())
            })
            .any(|root| root.is_dir() && path.starts_with(root));
        if !in_workspace {
            return Err(error("outside_workspace", "目录不在当前工作区内"));
        }
        if !path.is_dir() {
            return Err(error("invalid_path", "路径不是目录"));
        }
        if path.components().any(
            |c| matches!(c, Component::Normal(name) if excluded_directory(&name.to_string_lossy())),
        ) {
            return Err(error(
                "excluded_directory",
                "依赖、构建产物和 Git 目录不展开",
            ));
        }
        let mut entries = Vec::new();
        let mut truncated = false;
        for (index, entry) in fs::read_dir(&path)?.enumerate() {
            if index >= 2000 {
                truncated = true;
                break;
            }
            let entry = entry?;
            let kind = entry.file_type()?;
            // Also avoid cycles and directory junction traversal in the tree.
            if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() && excluded_directory(&name) {
                continue;
            }
            let child = match self.authorize(&entry.path()) {
                Ok(path) => path,
                Err(_) => continue,
            };
            if !child.starts_with(&path) {
                continue;
            }
            entries.push(WorkspaceTreeEntry {
                name,
                path: display_path(&child),
                kind: if kind.is_dir() { "directory" } else { "file" },
            });
        }
        entries.sort_by(|a, b| {
            (a.kind != "directory")
                .cmp(&(b.kind != "directory"))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(WorkspaceChildren {
            path: display_path(&path),
            revision: state.revision,
            entries,
            truncated,
        })
    }

    pub fn catalog(&self) -> Result<WorkspaceCatalog> {
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        let roots = self.allowed_roots()?;
        files.extend(self.local_grants.lock().map_err(|_| error("unavailable", "本机位置暂不可用"))?.files.iter().map(|p| display_path(p)));
        for root in &roots {
            match fs::read_dir(root) {
                Ok(entries) => {
                    for entry in entries.take(2001) {
                        if files.len() >= 2000 {
                            warnings.push("工作区文件列表已截断，请浏览进入目标目录查找".into());
                            break;
                        }
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(err) => {
                                warnings.push(err.to_string());
                                continue;
                            }
                        };
                        let path = entry.path();
                        if is_workspace_file(&path)
                            && self.authorize(&path).is_ok()
                            && path.is_file()
                        {
                            files.push(display_path(&path));
                        }
                    }
                }
                Err(err) => warnings.push(format!("{}: {err}", display_path(root))),
            }
        }
        files.sort();
        files.dedup();
        let recent_workspaces = match self.recent_entries() {
            Ok(entries) => entries,
            Err(err) => { warnings.push(format!("最近打开列表读取失败：{err}")); Vec::new() }
        };
        Ok(WorkspaceCatalog {
            allowed_roots: roots.iter().map(|p| display_path(p)).collect(),
            workspace_files: files,
            warnings,
            recent_workspaces,
        })
    }

    /// One level of allowlisted locations for picking folders and workspace
    /// files, including before a workspace is opened. Never reads file contents.
    pub fn browse(&self, request: WorkspaceBrowseRequest) -> Result<WorkspaceBrowse> {
        let Some(requested) = request.path else {
            return Ok(WorkspaceBrowse {
                path: None,
                parent: None,
                entries: self.allowed_roots()?
                    .iter()
                    .map(|root| WorkspaceTreeEntry {
                        name: display_path(root),
                        path: display_path(root),
                        kind: "directory",
                    })
                    .collect(),
                truncated: false,
            });
        };
        let path = self.authorize(Path::new(&requested))?;
        if !path.is_dir() {
            return Err(error("invalid_path", "请选择文件夹"));
        }
        if path.components().any(
            |c| matches!(c, Component::Normal(name) if excluded_directory(&name.to_string_lossy())),
        ) {
            return Err(error(
                "excluded_directory",
                "依赖、构建产物和 Git 目录不展开",
            ));
        }
        let mut entries = Vec::new();
        let mut truncated = false;
        for (index, item) in fs::read_dir(&path)?.enumerate() {
            if index >= 2000 {
                truncated = true;
                break;
            }
            let item = item?;
            let kind = item.file_type()?;
            let name = item.file_name().to_string_lossy().into_owned();
            if kind.is_symlink()
                || (kind.is_dir() && excluded_directory(&name))
                || (!kind.is_dir() && !(kind.is_file() && is_workspace_file(&item.path())))
            {
                continue;
            }
            let Ok(child) = self.authorize(&item.path()) else {
                continue;
            };
            if !child.starts_with(&path) {
                continue;
            }
            entries.push(WorkspaceTreeEntry {
                name,
                path: display_path(&child),
                kind: if kind.is_dir() { "directory" } else { "file" },
            });
        }
        entries.sort_by(|a, b| {
            (a.kind != "directory")
                .cmp(&(b.kind != "directory"))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(WorkspaceBrowse {
            parent: path
                .parent()
                .and_then(|p| self.authorize(p).ok())
                .map(|p| display_path(&p)),
            path: Some(display_path(&path)),
            entries,
            truncated,
        })
    }

    /// Authentication is verified by the adapter; authorization and all business
    /// rules, including optimistic concurrency, are enforced here for both inputs.
    pub fn execute(
        &self,
        request: WorkspaceRequest,
        authenticated: bool,
    ) -> Result<WorkspaceSnapshot> {
        if !authenticated {
            return Err(error("unauthorized", "需要有效的访问令牌"));
        }
        let mut state = self.lock()?;
        if state.revision != request.expected_revision {
            return Err(error("conflict", "工作区已被其他入口修改，请刷新后重试"));
        }
        match request.operation {
            WorkspaceOperation::Close => {
                check_discard(&state, request.discard_changes)?;
                state.session = None;
            }
            WorkspaceOperation::CreateFromFolder { path, folder } => {
                check_discard(&state, request.discard_changes)?;
                let folder = self.authorize(Path::new(&folder))?;
                if !folder.is_dir() {
                    return Err(error("invalid_path", "请选择存在的文件夹"));
                }
                let target = self.save_target(Path::new(&path))?;
                let source = serde_json::to_string_pretty(&serde_json::json!({ "folders": [{"path": display_path(&folder).replace('\\', "/")}], "settings": {} }))
                    .map_err(|err| error("invalid_workspace", err.to_string()))? + "\n";
                self.write_new(&target, &source)?;
                state.session = Some(Session {
                    base: parent(&target)?.to_owned(),
                    file: Some(target),
                    saved_source: Some(source.clone()),
                    source,
                    dirty: false,
                });
            }
            WorkspaceOperation::OpenRecent { path } => {
                check_discard(&state, request.discard_changes)?;
                state.session = Some(self.open_recent_session(&path)?);
            }
            WorkspaceOperation::Open { path } => {
                check_discard(&state, request.discard_changes)?;
                state.session = Some(self.open_session(Path::new(&path))?);
            }
            WorkspaceOperation::Create { path } => {
                check_discard(&state, request.discard_changes)?;
                let target = self.save_target(Path::new(&path))?;
                let source = "{\n\t\"folders\": [],\n\t\"settings\": {}\n}\n".to_owned();
                self.write_new(&target, &source)?;
                state.session = Some(Session {
                    base: parent(&target)?.to_owned(),
                    file: Some(target),
                    saved_source: Some(source.clone()),
                    source,
                    dirty: false,
                });
            }
            WorkspaceOperation::AddFolder { path, name } => {
                if name.as_ref().is_some_and(|n| n.chars().count() > 256) {
                    return Err(error("limit_exceeded", "显示名称不能超过 256 个字符"));
                }
                let folder = self.authorize(Path::new(&path))?;
                if !folder.is_dir() {
                    return Err(error("invalid_path", "请选择存在的文件夹"));
                }
                let session = state
                    .session
                    .as_mut()
                    .ok_or_else(|| error("no_workspace", "请先打开或新建工作区"))?;
                let value = parse_document(&session.source)?;
                let folders = folder_entries(&value)?;
                if folders.len() >= MAX_FOLDERS {
                    return Err(error("limit_exceeded", "工作区最多支持 512 个文件夹"));
                }
                for entry in folders {
                    if let Ok(path) = resolve_folder(entry, &session.base) {
                        if path.canonicalize().ok().as_ref() == Some(&folder) {
                            return Err(error("duplicate_folder", "工作区中已存在该文件夹"));
                        }
                    }
                }
                let mut fields = vec![(
                    "path".into(),
                    CstInputValue::String(display_path(&folder).replace('\\', "/")),
                )];
                if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
                    fields.push(("name".into(), CstInputValue::String(name)));
                }
                let root = parse_cst(&session.source)?;
                let array = root
                    .object_value()
                    .and_then(|o| o.get("folders"))
                    .and_then(|p| p.array_value())
                    .ok_or_else(|| error("invalid_workspace", "folders 必须是数组"))?;
                array.append(CstInputValue::Object(fields));
                let source = root.to_string();
                if source.len() as u64 > MAX_BYTES {
                    return Err(error("limit_exceeded", "工作区文件不能超过 2 MiB"));
                }
                session.source = source;
                session.dirty = true;
            }
            WorkspaceOperation::RemoveFolder { path } => {
                let session = state
                    .session
                    .as_ref()
                    .ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
                let document = parse_document(&session.source)?;
                let entries = folder_entries(&document)?;
                let key = comparison_path(Path::new(&path));
                let indices: Vec<usize> = entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        let resolved = match resolve_folder(entry, &session.base) {
                            Ok(path) => self
                                .authorize(&path)
                                .unwrap_or_else(|_| lexical_path(&path)),
                            Err(_) => PathBuf::from(
                                entry
                                    .get("uri")
                                    .or_else(|| entry.get("path"))
                                    .and_then(Value::as_str)
                                    .unwrap_or(""),
                            ),
                        };
                        (comparison_path(&resolved) == key).then_some(index)
                    })
                    .collect();
                if indices.is_empty() {
                    return Err(error("not_found", "当前工作区没有此根文件夹，请刷新后重试"));
                }
                let root = parse_cst(&session.source)?;
                let array = root
                    .object_value()
                    .and_then(|o| o.get("folders"))
                    .and_then(|p| p.array_value())
                    .ok_or_else(|| error("invalid_workspace", "folders 必须是数组"))?;
                let nodes = array.elements();
                for index in indices.into_iter().rev() {
                    nodes[index].clone().remove();
                }
                let session = state
                    .session
                    .as_mut()
                    .ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
                session.source = root.to_string();
                session.dirty = true;
            }
            WorkspaceOperation::Save { path } => {
                let session = state
                    .session
                    .as_mut()
                    .ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
                let requested = path
                    .map(PathBuf::from)
                    .or_else(|| session.file.clone())
                    .ok_or_else(|| error("invalid_path", "请指定 .code-workspace 保存路径"))?;
                let target = self.save_target(&requested)?;
                let same_file = session.file.as_ref() == Some(&target);
                let source = if parent(&target)? != session.base {
                    rebase(&session.source, &session.base)?
                } else {
                    session.source.clone()
                };
                if same_file {
                    let current = read_workspace(&target)?;
                    if session.saved_source.as_deref() != Some(current.as_str()) {
                        return Err(error(
                            "conflict",
                            "工作区文件已被外部修改，请重新打开后再编辑",
                        ));
                    }
                    self.replace_file(&target, &source, &current)?;
                } else {
                    self.write_new(&target, &source)?;
                }
                session.base = parent(&target)?.to_owned();
                session.file = Some(target);
                session.saved_source = Some(source.clone());
                session.source = source;
                session.dirty = false;
            }
        }
        state.revision += 1;
        state.notice = self.persist_recent(&state).err().map(|err| format!("当前位置已打开，但记忆失败，下次重启可能无法恢复：{err}"));
        self.snapshot_of(&state)
    }

    fn snapshot_of(&self, state: &State) -> Result<WorkspaceSnapshot> {
        let mut result = WorkspaceSnapshot {
            workspace_root: None,
            workspace_file: None,
            name: "未选择工作区".into(),
            revision: state.revision,
            dirty: false,
            folders: Vec::new(),
            projects: Vec::new(),
            warnings: state.notice.clone().into_iter().collect(),
            hint: None,
        };
        let Some(session) = &state.session else {
            result.hint = Some("打开 .code-workspace 文件或文件夹，也可以新建工作区".into());
            return Ok(result);
        };
        result.workspace_file = session.file.as_deref().map(display_path);
        result.dirty = session.dirty;
        let value = parse_document(&session.source)?;
        let mut seen = HashSet::new();
        let mut projects = HashSet::new();
        for entry in folder_entries(&value)? {
            let resolved = resolve_folder(entry, &session.base);
            let (path, status, message) = match resolved {
                Ok(path) => match self.authorize(&path) {
                    Ok(canonical) if canonical.is_dir() => (canonical, "available", None),
                    Ok(canonical) => (canonical, "not_directory", Some("路径不是文件夹".into())),
                    Err(err) => (
                        lexical_path(&path),
                        if err.code == "outside_roots" {
                            "blocked"
                        } else {
                            "unavailable"
                        },
                        Some(err.message),
                    ),
                },
                Err(err) => (
                    PathBuf::from(
                        entry
                            .get("uri")
                            .or_else(|| entry.get("path"))
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    ),
                    "unsupported",
                    Some(err.message),
                ),
            };
            let display = display_path(&path);
            let key = if cfg!(windows) {
                display.to_lowercase()
            } else {
                display.clone()
            };
            if !seen.insert(key) {
                continue;
            }
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| display_path(&path))
                });
            if status == "available" {
                match self.services_in(&path) {
                    Ok(found) => {
                        result.warnings.extend(found.warnings);
                        for project in found.projects {
                            if self.authorize(Path::new(&project.path)).is_ok()
                                && projects.insert(project.runtime_key())
                            {
                                result.projects.push(project);
                            }
                        }
                    }
                    Err(err) => result.warnings.push(format!("{display}: {err}")),
                }
            }
            result.folders.push(WorkspaceFolder {
                name,
                path: display,
                status: status.into(),
                message,
            });
        }
        result.workspace_root = result
            .folders
            .iter()
            .find(|f| f.status == "available")
            .map(|f| f.path.clone());
        result.name = session
            .file
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                result
                    .folders
                    .first()
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| "未命名工作区".into())
            });
        Ok(result)
    }

    fn open_session(&self, path: &Path) -> Result<Session> {
        let path = self.authorize(path)?;
        if path.is_dir() {
            let source = serde_json::to_string_pretty(
                &json!({"folders":[{"path": display_path(&path)}],"settings":{}}),
            )
            .map_err(|e| error("invalid_workspace", e.to_string()))?;
            Ok(Session {
                file: None,
                base: path,
                source,
                saved_source: None,
                dirty: false,
            })
        } else {
            if !is_workspace_file(&path) {
                return Err(error("invalid_path", "请选择 .code-workspace 文件或文件夹"));
            }
            let source = read_workspace(&path)?;
            parse_document(&source)?;
            Ok(Session {
                base: parent(&path)?.to_owned(),
                file: Some(path),
                saved_source: Some(source.clone()),
                source,
                dirty: false,
            })
        }
    }

    fn authorize(&self, path: &Path) -> Result<PathBuf> {
        validate_absolute(path)?;
        // Reject unrelated drives, UNC hosts and device namespaces before any
        // filesystem access. Canonical checks below additionally reject links.
        let candidate = comparison_path(path);
        let roots = self.allowed_roots()?;
        let files = self.local_grants.lock().map_err(|_| error("unavailable", "本机位置暂不可用"))?.files.clone();
        let exact_file = files.iter().any(|file| comparison_path(file) == candidate);
        if !exact_file && !roots.iter().any(|root| {
            let root = comparison_path(root);
            candidate == root || candidate.starts_with(&format!("{root}/"))
        }) {
            return Err(error(
                "outside_roots",
                "请通过系统选择窗口选择此位置，或配置 DEVFLOW_ALLOWED_ROOTS",
            ));
        }
        // Resolve the nearest existing ancestor first, even for missing paths.
        // A junction outside a root must not be reported as an authorized path.
        let resolved = resolve_missing(path)?;
        if !files.contains(&resolved) && !roots.iter().any(|root| resolved.starts_with(root)) {
            return Err(error(
                "outside_roots",
                "请通过系统选择窗口选择此位置，或配置 DEVFLOW_ALLOWED_ROOTS",
            ));
        }
        Ok(resolved.canonicalize()?)
    }

    fn save_target(&self, path: &Path) -> Result<PathBuf> {
        validate_absolute(path)?;
        if !is_workspace_file(path) {
            return Err(error(
                "invalid_path",
                "保存文件必须使用 .code-workspace 扩展名",
            ));
        }
        // An explicitly selected workspace file may be saved without granting its parent.
        let selected_file = self.local_grants.lock()
            .map_err(|_| error("unavailable", "本机位置暂不可用"))?
            .files.iter().any(|file| comparison_path(file) == comparison_path(path));
        if selected_file {
            let target = self.authorize(path)?;
            let meta = fs::symlink_metadata(path)?;
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(error("invalid_path", "不能覆盖链接或目录"));
            }
            return Ok(target);
        }
        let directory = self.authorize(parent(path)?)?;
        if !directory.is_dir() {
            return Err(error("invalid_path", "保存目录不存在"));
        }
        let target = directory.join(
            path.file_name()
                .ok_or_else(|| error("invalid_path", "无效的文件名"))?,
        );
        if let Ok(meta) = fs::symlink_metadata(&target) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(error("invalid_path", "不能覆盖链接或目录"));
            }
            self.authorize(&target)?;
        }
        Ok(target)
    }

    fn write_new(&self, target: &Path, source: &str) -> Result<()> {
        if source.len() as u64 > MAX_BYTES {
            return Err(error("limit_exceeded", "工作区文件不能超过 2 MiB"));
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    error("already_exists", "文件已存在，请打开它或选择其他名称")
                } else {
                    err.into()
                }
            })?;
        if let Err(err) = file
            .write_all(source.as_bytes())
            .and_then(|_| file.sync_all())
        {
            drop(file);
            let _ = fs::remove_file(target);
            return Err(err.into());
        }
        Ok(())
    }

    fn replace_file(&self, target: &Path, source: &str, original: &str) -> Result<()> {
        let permissions = fs::metadata(target)?.permissions();
        if permissions.readonly() {
            return Err(error("permission_denied", "工作区文件是只读的"));
        }
        let temp = parent(target)?.join(format!(".devflow-{}.tmp", uuid::Uuid::new_v4()));
        self.write_new(&temp, source)?;
        let result = (|| {
            self.save_target(target)?;
            if read_workspace(target)? != original {
                return Err(error("conflict", "工作区文件已被外部修改"));
            }
            fs::set_permissions(&temp, permissions)?;
            fs::rename(&temp, target)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn check_discard(state: &State, discard: bool) -> Result<()> {
    if !discard && state.session.as_ref().is_some_and(|s| s.dirty) {
        return Err(error(
            "unsaved_changes",
            "工作区有未保存的更改，请保存或明确放弃更改",
        ));
    }
    Ok(())
}
fn excluded_directory(name: &str) -> bool {
    ["node_modules", "target", ".git", "build", "dist"]
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
}
fn parent(path: &Path) -> Result<&Path> {
    path.parent()
        .ok_or_else(|| error("invalid_path", "路径没有父目录"))
}
fn is_workspace_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("code-workspace"))
}
fn validate_absolute(path: &Path) -> Result<()> {
    if path.as_os_str().len() > 32768 || path.components().count() > 512 {
        return Err(error("limit_exceeded", "路径过长"));
    }
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::Normal(name) if cfg!(windows) && name.to_string_lossy().contains(':'))) {
        return Err(error("invalid_path", "请使用完整的本地绝对路径"));
    }
    Ok(())
}
fn resolve_missing(path: &Path) -> Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // Broken symlinks must not be treated as ordinary missing children.
            if fs::symlink_metadata(path).is_ok() {
                return Err(err.into());
            }
            let base = resolve_missing(parent(path)?)?;
            let name = path
                .file_name()
                .ok_or_else(|| error("invalid_path", "无法解析路径"))?;
            Ok(base.join(name))
        }
        Err(err) => Err(err.into()),
    }
}
pub fn display_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_owned()
    }
}
fn lexical_path(path: &Path) -> PathBuf {
    let plain = PathBuf::from(display_path(path));
    let mut result = PathBuf::new();
    for component in plain.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            component => result.push(component.as_os_str()),
        }
    }
    result
}
pub(super) fn comparison_path(path: &Path) -> String {
    let value = display_path(&lexical_path(path)).replace('\\', "/");
    let value = value.trim_end_matches('/');
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.to_owned()
    }
}
fn options() -> ParseOptions {
    ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    }
}
fn parse_document(source: &str) -> Result<Value> {
    let value: Value =
        jsonc_parser::parse_to_serde_value(source.trim_start_matches('\u{feff}'), &options())
            .map_err(|err| error("invalid_workspace", err.to_string()))?;
    let entries = folder_entries(&value)?;
    if entries.len() > MAX_FOLDERS {
        return Err(error("limit_exceeded", "工作区最多支持 512 个文件夹"));
    }
    Ok(value)
}
fn folder_entries(value: &Value) -> Result<&Vec<Value>> {
    value
        .get("folders")
        .and_then(Value::as_array)
        .ok_or_else(|| error("invalid_workspace", "工作区必须包含 folders 数组"))
}
fn parse_cst(source: &str) -> Result<CstRootNode> {
    CstRootNode::parse(source.trim_start_matches('\u{feff}'), &options())
        .map_err(|err| error("invalid_workspace", err.to_string()))
}
fn resolve_folder(entry: &Value, base: &Path) -> Result<PathBuf> {
    if let Some(path) = entry
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
    {
        let path = Path::new(path);
        Ok(if path.is_absolute() {
            path.to_owned()
        } else {
            base.join(path)
        })
    } else if let Some(uri) = entry.get("uri").and_then(Value::as_str) {
        url::Url::parse(uri)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .ok_or_else(|| {
                error(
                    "unsupported_uri",
                    "仅支持本地 file: URI；远程工作区暂不支持",
                )
            })
    } else {
        Err(error("invalid_folder", "文件夹缺少有效的 path 或 uri"))
    }
}
fn read_workspace(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_BYTES {
        return Err(error("limit_exceeded", "工作区文件不能超过 2 MiB"));
    }
    let mut source = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut source)?;
    if source.len() as u64 > MAX_BYTES {
        return Err(error("limit_exceeded", "工作区文件不能超过 2 MiB"));
    }
    Ok(source)
}
fn rebase(source: &str, base: &Path) -> Result<String> {
    let value = parse_document(source)?;
    let root = parse_cst(source)?;
    let array = root
        .object_value()
        .and_then(|o| o.get("folders"))
        .and_then(|p| p.array_value())
        .ok_or_else(|| error("invalid_workspace", "folders 必须是数组"))?;
    for (entry, node) in folder_entries(&value)?.iter().zip(array.elements()) {
        if entry.get("path").and_then(Value::as_str).is_some() {
            let path = resolve_folder(entry, base)?;
            if let Some(prop) = node.as_object().and_then(|o| o.get("path")) {
                prop.set_value(CstInputValue::String(
                    display_path(&path).replace('\\', "/"),
                ));
            }
        }
    }
    Ok(root.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick_request(kind: WorkspacePickKind, expected_revision: u64) -> WorkspacePickRequest {
        WorkspacePickRequest { kind, expected_revision }
    }

    #[test]
    fn native_folder_selection_grants_only_the_selected_tree() {
        let fixture = Fixture::new();
        let selected = fixture.0.join("中文 项目");
        fs::create_dir_all(selected.join("子目录")).unwrap();
        let service = WorkspaceService::new(vec![], None).unwrap();
        assert_eq!(service.authorize(&selected).unwrap_err().code, "outside_roots");
        let result = service.accept_local_selection(pick_request(WorkspacePickKind::Folder, 0), Some(path(&selected))).unwrap();
        assert!(result.path.is_some());
        assert!(service.authorize(&selected.join("子目录")).is_ok());
        assert_eq!(service.authorize(&fixture.0).unwrap_err().code, "outside_roots");
        assert_eq!(service.snapshot().unwrap().revision, 0);
        assert!(service.snapshot().unwrap().folders.is_empty());
    }

    #[test]
    fn native_workspace_selection_grants_declared_folders_and_exact_file_only() {
        let fixture = Fixture::new();
        let projects = fixture.0.join("项目");
        let config_dir = fixture.0.join("配置");
        fs::create_dir_all(&projects).unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        let config = config_dir.join("中文.code-workspace");
        fs::write(&config, "{\n// 保留注释\n\"folders\": [{\"path\": \"../项目\"}], \"settings\": {}}\n").unwrap();
        let service = WorkspaceService::new(vec![], None).unwrap();
        service.accept_local_selection(pick_request(WorkspacePickKind::WorkspaceFile, 0), Some(path(&config))).unwrap();
        assert!(service.authorize(&projects).is_ok());
        assert!(service.authorize(&config).is_ok());
        assert_eq!(service.authorize(&config_dir).unwrap_err().code, "outside_roots");
        assert!(service.save_target(&config).is_ok());
        assert_eq!(service.save_target(&config_dir.join("other.code-workspace")).unwrap_err().code, "outside_roots");
        let opened = fixture.command(&service, WorkspaceOperation::Open { path: path(&config) }).unwrap();
        assert_eq!(opened.folders[0].status, "available");
        fixture.command(&service, WorkspaceOperation::Save { path: None }).unwrap();
        assert!(fs::read_to_string(&config).unwrap().contains("保留注释"));
    }

    #[test]
    fn native_picker_checks_authentication_revision_and_cancel_without_granting() {
        let fixture = Fixture::new();
        let service = WorkspaceService::new(vec![], None).unwrap();
        assert_eq!(service.pick_local(pick_request(WorkspacePickKind::Folder, 0), false).unwrap_err().code, "unauthorized");
        assert_eq!(service.pick_local(pick_request(WorkspacePickKind::Folder, 1), true).unwrap_err().code, "conflict");
        assert!(service.accept_local_selection(pick_request(WorkspacePickKind::Folder, 0), None).unwrap().path.is_none());
        assert_eq!(service.accept_local_selection(pick_request(WorkspacePickKind::Folder, 1), Some(path(&fixture.0))).unwrap_err().code, "conflict");
        assert_eq!(service.authorize(&fixture.0).unwrap_err().code, "outside_roots");
        assert!(service.catalog().unwrap().allowed_roots.is_empty());
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("devflow-workspace-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn service(&self) -> WorkspaceService {
            WorkspaceService::new(vec![self.0.clone()], None).unwrap()
        }
        fn command(
            &self,
            service: &WorkspaceService,
            operation: WorkspaceOperation,
        ) -> Result<WorkspaceSnapshot> {
            service.execute(
                WorkspaceRequest {
                    expected_revision: service.snapshot()?.revision,
                    discard_changes: false,
                    operation,
                },
                true,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // This fixture owns only its uniquely named directory under temp.
            if self
                .0
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("devflow-workspace-test-"))
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
    fn path(path: &Path) -> String {
        display_path(path)
    }

    #[test]
    fn remove_folder_only_removes_root_references_and_preserves_disk_and_jsonc() {
        let fixture = Fixture::new();
        let folder = fixture.0.join("中文项目");
        fs::create_dir_all(folder.join("src")).unwrap();
        fs::write(folder.join("src/keep.txt"), "keep").unwrap();
        let config = fixture.0.join("remove.code-workspace");
        let original = "{\n// keep workspace comment\n\"folders\":[{\"path\":\"./中文项目\"},{\"path\":\"./中文项目/.\"},{\"path\":\"./missing\"}],\"settings\":{\"keep\":true},\"custom\":42\n}";
        fs::write(&config, original).unwrap();
        let service = fixture.service();
        fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&config),
                },
            )
            .unwrap();
        let revision = service.snapshot().unwrap().revision;
        assert_eq!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::RemoveFolder {
                        path: path(&folder.join("src"))
                    }
                )
                .unwrap_err()
                .code,
            "not_found"
        );
        assert_eq!(revision, service.snapshot().unwrap().revision);
        let removed = fixture
            .command(
                &service,
                WorkspaceOperation::RemoveFolder {
                    path: path(&folder),
                },
            )
            .unwrap();
        assert_eq!(removed.folders.len(), 1);
        assert_eq!(removed.folders[0].status, "unavailable");
        assert!(removed.dirty);
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert_eq!(
            fs::read_to_string(folder.join("src/keep.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            service
                .execute(
                    WorkspaceRequest {
                        expected_revision: revision,
                        discard_changes: false,
                        operation: WorkspaceOperation::RemoveFolder {
                            path: path(&folder)
                        }
                    },
                    true
                )
                .unwrap_err()
                .code,
            "conflict"
        );
        fixture
            .command(
                &service,
                WorkspaceOperation::RemoveFolder {
                    path: removed.folders[0].path.clone(),
                },
            )
            .unwrap();
        assert!(service.snapshot().unwrap().folders.is_empty());
        fixture
            .command(&service, WorkspaceOperation::Save { path: None })
            .unwrap();
        let saved = fs::read_to_string(&config).unwrap();
        assert!(saved.contains("// keep workspace comment"));
        let value = parse_document(&saved).unwrap();
        assert!(value["folders"].as_array().unwrap().is_empty());
        assert_eq!(value["custom"], 42);
        assert_eq!(value["settings"]["keep"], true);
        assert!(folder.join("src/keep.txt").exists());
    }

    #[test]
    fn close_http_request_decodes_and_clears_the_shared_session() {
        let fixture = Fixture::new();
        let service = fixture.service();
        fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&fixture.0),
                },
            )
            .unwrap();
        let revision = service.snapshot().unwrap().revision;
        let request: WorkspaceRequest = serde_json::from_value(serde_json::json!({
            "expectedRevision": revision, "discardChanges": false,
            "operation": { "type": "close" }
        }))
        .unwrap();
        let closed = service.execute(request, true).unwrap();
        assert_eq!(closed.revision, revision + 1);
        assert!(closed.workspace_root.is_none());
        assert!(closed.workspace_file.is_none());
        assert!(service.snapshot().unwrap().folders.is_empty());
    }

    #[test]
    fn browser_is_bounded_and_works_without_an_active_workspace() {
        let fixture = Fixture::new();
        let service = fixture.service();
        fs::create_dir(fixture.0.join("项目")).unwrap();
        fs::create_dir(fixture.0.join("node_modules")).unwrap();
        fs::write(fixture.0.join("可打开.code-workspace"), "{\"folders\":[]}").unwrap();
        fs::write(fixture.0.join("secret.txt"), "not listed").unwrap();
        let roots = service.browse(WorkspaceBrowseRequest::default()).unwrap();
        assert!(roots.path.is_none());
        assert_eq!(roots.entries.len(), 1);
        let listing = service
            .browse(WorkspaceBrowseRequest {
                path: Some(path(&fixture.0)),
            })
            .unwrap();
        assert!(listing.parent.is_none());
        assert_eq!(listing.entries.len(), 2);
        assert_eq!(listing.entries[0].kind, "directory");
        assert!(
            listing
                .entries
                .iter()
                .all(|e| e.name != "secret.txt" && e.name != "node_modules")
        );
        assert_eq!(
            service
                .browse(WorkspaceBrowseRequest {
                    path: Some(path(&fixture.0.join("node_modules")))
                })
                .unwrap_err()
                .code,
            "excluded_directory"
        );
        assert_eq!(
            service
                .browse(WorkspaceBrowseRequest {
                    path: Some(path(fixture.0.parent().unwrap()))
                })
                .unwrap_err()
                .code,
            "outside_roots"
        );
    }

    #[test]
    fn create_from_folder_is_atomic_and_close_protects_unsaved_changes() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let folder = fixture.0.join("中文项目");
        fs::create_dir(&folder).unwrap();
        let target = fixture.0.join("新建.code-workspace");
        let created = fixture
            .command(
                &service,
                WorkspaceOperation::CreateFromFolder {
                    path: path(&target),
                    folder: path(&folder),
                },
            )
            .unwrap();
        assert_eq!(created.folders.len(), 1);
        assert_eq!(created.folders[0].path, path(&folder));
        assert!(!created.dirty);
        let original = fs::read(&target).unwrap();
        assert!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::CreateFromFolder {
                        path: path(&target),
                        folder: path(&fixture.0)
                    }
                )
                .is_err()
        );
        assert_eq!(original, fs::read(&target).unwrap());
        fixture
            .command(
                &service,
                WorkspaceOperation::AddFolder {
                    path: path(&fixture.0),
                    name: None,
                },
            )
            .unwrap();
        assert_eq!(
            fixture
                .command(&service, WorkspaceOperation::Close)
                .unwrap_err()
                .code,
            "unsaved_changes"
        );
        let revision = service.snapshot().unwrap().revision;
        let closed = service
            .execute(
                WorkspaceRequest {
                    expected_revision: revision,
                    discard_changes: true,
                    operation: WorkspaceOperation::Close,
                },
                true,
            )
            .unwrap();
        assert!(closed.folders.is_empty());
        assert!(closed.projects.is_empty());
        assert!(closed.workspace_file.is_none());
        assert!(target.is_file());
        assert_eq!(original, fs::read(&target).unwrap());
        assert_eq!(
            service
                .execute(
                    WorkspaceRequest {
                        expected_revision: revision,
                        discard_changes: true,
                        operation: WorkspaceOperation::Close
                    },
                    true
                )
                .unwrap_err()
                .code,
            "conflict"
        );
    }

    #[test]
    fn jsonc_relative_paths_names_and_unavailable_folders() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("项目")).unwrap();
        let config = fixture.0.join("中文.code-workspace");
        fs::write(&config, "\u{feff}{\n// 中文注释\n\"folders\":[{\"path\":\"./项目\",\"name\":\"后端\"},{\"path\":\"missing\"},{\"uri\":\"vscode-remote://ssh-remote/server\"},],\"settings\":{\"url\":\"https://example.test/a//b\"},}").unwrap();
        let service = fixture.service();
        let snapshot = fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&config),
                },
            )
            .unwrap();
        assert_eq!(snapshot.name, "中文");
        assert_eq!(snapshot.folders.len(), 3);
        assert_eq!(snapshot.folders[0].name, "后端");
        assert_eq!(snapshot.folders[0].path, path(&fixture.0.join("项目")));
        assert_eq!(snapshot.folders[0].status, "available");
        assert_eq!(snapshot.folders[1].status, "unavailable");
        assert_eq!(snapshot.folders[2].status, "unsupported");
    }

    #[test]
    fn create_add_save_and_reopen_preserves_configuration_and_comments() {
        let fixture = Fixture::new();
        let config = fixture.0.join("sample.code-workspace");
        let folder = fixture.0.join("后端");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("pom.xml"), "<project/>").unwrap();
        let service = fixture.service();
        fixture
            .command(
                &service,
                WorkspaceOperation::Create {
                    path: path(&config),
                },
            )
            .unwrap();
        let original = "{\n// keep me\n\"folders\":[],\"settings\":{\"java.test\":true},\"extensions\":{\"recommendations\":[\"redhat.java\"]},\"custom\":42\n}";
        fs::write(&config, original).unwrap();
        fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&config),
                },
            )
            .unwrap();
        let added = fixture
            .command(
                &service,
                WorkspaceOperation::AddFolder {
                    path: path(&folder),
                    name: Some("应用".into()),
                },
            )
            .unwrap();
        assert!(added.dirty);
        // A POM alone is a library descriptor, not evidence of a runnable service.
        assert!(added.projects.is_empty());
        assert_eq!(read_workspace(&config).unwrap(), original);
        fixture
            .command(&service, WorkspaceOperation::Save { path: None })
            .unwrap();
        let saved = read_workspace(&config).unwrap();
        assert!(saved.contains("// keep me"));
        let value = parse_document(&saved).unwrap();
        assert_eq!(value["settings"]["java.test"], true);
        assert_eq!(value["extensions"]["recommendations"][0], "redhat.java");
        assert_eq!(value["custom"], 42);
        let reopened = fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&config),
                },
            )
            .unwrap();
        assert!(!reopened.dirty);
        assert_eq!(reopened.folders[0].name, "应用");
    }

    #[test]
    fn direct_folder_can_be_saved_and_save_as_rebases_relative_paths() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("app")).unwrap();
        fs::create_dir(fixture.0.join("other")).unwrap();
        let service = fixture.service();
        let opened = fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&fixture.0.join("app")),
                },
            )
            .unwrap();
        assert!(opened.workspace_file.is_none());
        fixture
            .command(
                &service,
                WorkspaceOperation::Save {
                    path: Some(path(&fixture.0.join("folder.code-workspace"))),
                },
            )
            .unwrap();
        let config = fixture.0.join("relative.code-workspace");
        fs::write(
            &config,
            "{\"folders\":[{\"path\":\"app\",\"name\":\"App\",\"custom\":true}],\"settings\":{}}",
        )
        .unwrap();
        fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&config),
                },
            )
            .unwrap();
        let saved = fixture
            .command(
                &service,
                WorkspaceOperation::Save {
                    path: Some(path(&fixture.0.join("other/copy.code-workspace"))),
                },
            )
            .unwrap();
        assert_eq!(saved.folders[0].path, path(&fixture.0.join("app")));
        let value =
            parse_document(&read_workspace(&fixture.0.join("other/copy.code-workspace")).unwrap())
                .unwrap();
        assert_eq!(value["folders"][0]["custom"], true);
        assert_eq!(
            parse_document(&read_workspace(&config).unwrap()).unwrap()["folders"][0]["path"],
            "app"
        );
    }

    #[test]
    fn shared_revision_authentication_duplicates_and_unsaved_changes() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let unauthenticated = WorkspaceRequest {
            expected_revision: 0,
            discard_changes: false,
            operation: WorkspaceOperation::Open {
                path: path(&fixture.0),
            },
        };
        assert_eq!(
            service.execute(unauthenticated, false).unwrap_err().code,
            "unauthorized"
        );
        fixture
            .command(
                &service,
                WorkspaceOperation::Create {
                    path: path(&fixture.0.join("one.code-workspace")),
                },
            )
            .unwrap();
        let stale = WorkspaceRequest {
            expected_revision: 0,
            discard_changes: false,
            operation: WorkspaceOperation::Open {
                path: path(&fixture.0),
            },
        };
        assert_eq!(service.execute(stale, true).unwrap_err().code, "conflict");
        fixture
            .command(
                &service,
                WorkspaceOperation::AddFolder {
                    path: path(&fixture.0),
                    name: None,
                },
            )
            .unwrap();
        assert_eq!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::AddFolder {
                        path: path(&fixture.0),
                        name: None
                    }
                )
                .unwrap_err()
                .code,
            "duplicate_folder"
        );
        assert_eq!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::Open {
                        path: path(&fixture.0)
                    }
                )
                .unwrap_err()
                .code,
            "unsaved_changes"
        );
        let discard = WorkspaceRequest {
            expected_revision: service.snapshot().unwrap().revision,
            discard_changes: true,
            operation: WorkspaceOperation::Open {
                path: path(&fixture.0),
            },
        };
        assert!(!service.execute(discard, true).unwrap().dirty);
    }

    #[test]
    fn refuses_unrelated_overwrites_external_edits_and_outside_roots() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let service = fixture.service();
        assert_eq!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::Open {
                        path: path(&outside.0)
                    }
                )
                .unwrap_err()
                .code,
            "outside_roots"
        );
        let config = fixture.0.join("a.code-workspace");
        fixture
            .command(
                &service,
                WorkspaceOperation::Create {
                    path: path(&config),
                },
            )
            .unwrap();
        assert_eq!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::Create {
                        path: path(&config)
                    }
                )
                .unwrap_err()
                .code,
            "already_exists"
        );
        let external = "{\"folders\":[],\"settings\":{\"changed\":true}}";
        fs::write(&config, external).unwrap();
        assert_eq!(
            fixture
                .command(&service, WorkspaceOperation::Save { path: None })
                .unwrap_err()
                .code,
            "conflict"
        );
        assert_eq!(read_workspace(&config).unwrap(), external);
        assert_eq!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::Save {
                        path: Some(path(&outside.0.join("x.code-workspace")))
                    }
                )
                .unwrap_err()
                .code,
            "outside_roots"
        );
        assert!(!outside.0.join("x.code-workspace").exists());
        // A non-existent path outside the roots must be rejected before stat.
        assert_eq!(
            service
                .authorize(&outside.0.join("missing/deep"))
                .unwrap_err()
                .code,
            "outside_roots"
        );
    }

    #[test]
    fn file_uri_deduplicates_path_entries_and_preserves_unknown_folders() {
        let fixture = Fixture::new();
        let uri = url::Url::from_directory_path(&fixture.0)
            .unwrap()
            .to_string();
        let config = fixture.0.join("uri.code-workspace");
        fs::write(&config, json!({"folders":[{"uri":uri,"name":"第一项"},{"path":"."},{"invalid":true}],"settings":{}}).to_string()).unwrap();
        let service = fixture.service();
        let snapshot = fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&config),
                },
            )
            .unwrap();
        assert_eq!(snapshot.folders.len(), 2);
        assert_eq!(snapshot.folders[0].name, "第一项");
        assert_eq!(snapshot.folders[1].status, "unsupported");
    }

    #[test]
    fn invalid_jsonc_and_non_workspace_files_are_rejected() {
        for source in [
            "{}",
            "{\"folders\":{}}",
            "{folders:[]}",
            "{\"folders\":[] \"settings\":{}}",
            "{\"folders\":[",
        ] {
            assert!(parse_document(source).is_err(), "{source}");
        }
        assert!(parse_document("{//ok\n\"folders\":[],}").is_ok());
        let fixture = Fixture::new();
        let file = fixture.0.join("package.json");
        fs::write(&file, "{}").unwrap();
        assert_eq!(
            fixture
                .command(
                    &fixture.service(),
                    WorkspaceOperation::Open { path: path(&file) }
                )
                .unwrap_err()
                .code,
            "invalid_path"
        );
    }

    #[test]
    fn limits_large_documents_and_folder_counts() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let config = fixture.0.join("large.code-workspace");
        fs::write(&config, " ".repeat(MAX_BYTES as usize + 1)).unwrap();
        assert_eq!(
            fixture
                .command(
                    &service,
                    WorkspaceOperation::Open {
                        path: path(&config)
                    }
                )
                .unwrap_err()
                .code,
            "limit_exceeded"
        );
        let oversized = json!({"folders": vec![json!({"path":"."}); MAX_FOLDERS + 1]}).to_string();
        assert_eq!(
            parse_document(&oversized).unwrap_err().code,
            "limit_exceeded"
        );
    }

    #[test]
    fn tree_reads_one_level_sorts_directories_and_excludes_artifacts() {
        let fixture = Fixture::new();
        let root = fixture.0.join("app");
        for directory in [
            "src/nested",
            "node_modules",
            "target",
            ".git",
            "build",
            "dist",
            "中文",
        ] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        fs::write(root.join("package.json"), "{}").unwrap();
        fs::write(root.join("src/main.ts"), "export {};").unwrap();
        let service = fixture.service();
        let active = fixture
            .command(&service, WorkspaceOperation::Open { path: path(&root) })
            .unwrap();
        let children = service
            .children(WorkspaceChildrenRequest {
                path: path(&root),
                expected_revision: active.revision,
            })
            .unwrap();
        assert_eq!(
            children
                .entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            vec!["src", "中文", "package.json"]
        );
        assert_eq!(children.entries[0].kind, "directory");
        assert_eq!(children.entries[2].kind, "file");
        assert!(
            !children
                .entries
                .iter()
                .any(|e| e.name == "nested" || e.name == "main.ts")
        );
        let source = service
            .children(WorkspaceChildrenRequest {
                path: path(&root.join("src")),
                expected_revision: active.revision,
            })
            .unwrap();
        assert_eq!(source.entries.len(), 2);
        assert_eq!(source.entries[0].name, "nested");
        assert_eq!(
            service
                .children(WorkspaceChildrenRequest {
                    path: path(&root.join("node_modules")),
                    expected_revision: active.revision
                })
                .unwrap_err()
                .code,
            "excluded_directory"
        );
    }
    #[test]
    fn refresh_rediscovers_services_without_changing_workspace_revision() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("pom.xml"), "<project/>").unwrap();
        let service = fixture.service();
        let before = fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&fixture.0),
                },
            )
            .unwrap();
        assert!(before.projects.is_empty());
        let source = fixture.0.join("src/main/java/App.java");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(
            source,
            "@SpringBootApplication class App { public static void main(String[] args) {} }",
        )
        .unwrap();
        assert!(service.snapshot().unwrap().projects.is_empty());
        let refreshed = service.refresh().unwrap();
        assert_eq!(refreshed.projects.len(), 1);
        assert_eq!(refreshed.projects[0].main_classes, vec!["App"]);
        assert_eq!(refreshed.revision, before.revision);
        assert!(!refreshed.dirty);
    }

    #[test]
    fn tree_rejects_paths_outside_active_workspace_and_stale_revisions() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("app")).unwrap();
        fs::create_dir(fixture.0.join("other")).unwrap();
        let service = fixture.service();
        let active = fixture
            .command(
                &service,
                WorkspaceOperation::Open {
                    path: path(&fixture.0.join("app")),
                },
            )
            .unwrap();
        assert_eq!(
            service
                .children(WorkspaceChildrenRequest {
                    path: path(&fixture.0.join("other")),
                    expected_revision: active.revision
                })
                .unwrap_err()
                .code,
            "outside_workspace"
        );
        assert_eq!(
            service
                .children(WorkspaceChildrenRequest {
                    path: path(&fixture.0.join("app")),
                    expected_revision: 0
                })
                .unwrap_err()
                .code,
            "conflict"
        );
    }

    #[test]
    fn symlink_escape_is_blocked_when_supported() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let link = fixture.0.join("escape");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside.0, &link);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside.0, &link);
        #[cfg(any(windows, unix))]
        if linked.is_ok() {
            let service = fixture.service();
            assert_eq!(service.authorize(&link).unwrap_err().code, "outside_roots");
            assert_eq!(
                service
                    .save_target(&link.join("escape.code-workspace"))
                    .unwrap_err()
                    .code,
                "outside_roots"
            );
            assert_eq!(
                service.authorize(&link.join("missing")).unwrap_err().code,
                "outside_roots"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Reads the user's D:\\WorkSpace fixtures; run explicitly when local acceptance is authorized"]
    fn opens_all_nine_real_workspace_fixtures_read_only() {
        let base = PathBuf::from(r"D:\WorkSpace");
        for name in [
            "2.0.0",
            "ai-chat",
            "extreme-plus-portal",
            "extreme-skills",
            "framework-master",
            "health-management",
            "main-system",
            "plan",
            "vue3-upgrade-design",
        ] {
            let config = base.join(format!("{name}.code-workspace"));
            let document = parse_document(&read_workspace(&config).unwrap()).unwrap();
            let entries = folder_entries(&document).unwrap();
            let mut roots = vec![base.clone()];
            for entry in entries {
                if let Ok(folder) = resolve_folder(entry, &base) {
                    if folder.is_dir() {
                        roots.push(folder);
                    }
                }
            }
            let service = WorkspaceService::new(roots, Some(config)).unwrap();
            let snapshot = service.snapshot().unwrap();
            assert_eq!(snapshot.folders.len(), entries.len(), "{name}");
            for (entry, folder) in entries.iter().zip(&snapshot.folders) {
                let resolved = resolve_folder(entry, &base).unwrap();
                if resolved.is_dir() {
                    assert_eq!(folder.status, "available", "{name}: {}", folder.path);
                    assert_eq!(folder.path, path(&resolved.canonicalize().unwrap()));
                } else {
                    assert_ne!(folder.status, "available", "{name}: {}", folder.path);
                }
            }
        }
    }
}
