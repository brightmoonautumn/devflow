//! Update and merge lifecycle. No adapter is allowed to write the Git index directly.
use super::*;

const TEXT_LIMIT: usize = 1024 * 1024;
const SNAPSHOT_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitUpdateRequest { pub expected_revision: u64, pub repository: String, pub version: Option<String> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitUpdateProgressRequest { pub expected_revision: u64, pub job_id: Option<String> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitMergeRequest {
    pub expected_revision: u64, pub repository: String,
    pub version: Option<String>, pub path: Option<String>, pub message: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitResolveRequest {
    pub expected_revision: u64, pub repository: String, pub version: String, pub path: String,
    /// edit, local, incoming, delete. Versions always refer to merge index stages.
    pub choice: String, pub content: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitUpdateInfo {
    pub repository: String, pub branch: String, pub upstream: Option<String>,
    pub version: String, pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitUpdateProgress {
    pub job_id: String, pub repository: String, pub phase: String,
    pub done: bool, pub message: String,
}
pub(in super::super) struct UpdateJob { revision: u64, progress: GitUpdateProgress }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeFile { pub path: String, pub local_deleted: bool, pub incoming_deleted: bool, pub supported: bool }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitMergeState {
    pub repository: String, pub branch: String, pub active: bool, pub version: String,
    pub files: Vec<MergeFile>, pub message: String, pub changed_paths: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConflictContent {
    pub path: String, pub version: String, pub base: Option<String>, pub local: Option<String>,
    pub incoming: Option<String>, pub result: Option<String>, pub reason: Option<String>,
    pub local_deleted: bool, pub incoming_deleted: bool,
}
#[derive(Clone, Default)]
struct Stages { path: String, entries: [Option<(String, String)>; 3] }

fn bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit as u64 { return Err(error("limit_exceeded", "合并文件或索引超过处理上限，请在外部 Git 工具处理")); }
    let mut bytes = Vec::new(); file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit { return Err(error("limit_exceeded", "合并文件超过处理上限")); } Ok(bytes)
}
fn unmerged(root: &Path) -> Result<Vec<Stages>> {
    let bytes = checked(root, &["ls-files", "--unmerged", "-z"])?;
    parse_stages(&bytes)
}
fn parse_stages(bytes: &[u8]) -> Result<Vec<Stages>> {
    let mut files: Vec<Stages> = Vec::new();
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let record = text(record)?;
        let (meta, path) = record.split_once('\t').ok_or_else(|| error("git_failed", "无效的冲突索引"))?;
        let parts: Vec<_> = meta.split(' ').collect();
        if parts.len() != 3 { return Err(error("git_failed", "无效的冲突索引")); }
        let stage = parts[2].parse::<usize>().ok().filter(|s| (1..=3).contains(s)).ok_or_else(|| error("git_failed", "无效的冲突阶段"))?;
        if !parts[1].bytes().all(|b| b.is_ascii_hexdigit()) || !matches!(parts[1].len(), 40 | 64) { return Err(error("git_failed", "无效的冲突对象")); }
        if !files.iter().any(|f| f.path == path) { files.push(Stages { path: path.into(), ..Stages::default() }); }
        files.iter_mut().find(|f| f.path == path).unwrap().entries[stage - 1] = Some((parts[0].into(), parts[1].into()));
        if files.len() > 2000 { return Err(error("limit_exceeded", "冲突文件超过 2000 个，请在外部处理")); }
    }
    Ok(files)
}
fn ordinary(stages: &Stages) -> bool {
    let has_base = stages.entries[0].is_some();
    let has_local = stages.entries[1].is_some();
    let has_incoming = stages.entries[2].is_some();
    (has_local || has_incoming) && (has_base || (has_local && has_incoming))
        && stages.entries.iter().flatten().all(|(mode, _)| matches!(mode.as_str(), "100644" | "100755"))
}
fn blob(root: &Path, entry: &Option<(String, String)>) -> Result<Option<Vec<u8>>> {
    entry.as_ref().map(|(_, oid)| {
        let size = text(&checked(root, &["cat-file", "-s", oid])?)?.trim().parse::<usize>().map_err(|_| error("git_failed", "无法读取对象大小"))?;
        if size > 8 * TEXT_LIMIT { return Err(error("limit_exceeded", "版本超过 8 MiB，请使用外部工具合并")); }
        checked(root, &["cat-file", "blob", oid])
    }).transpose()
}
fn editable(bytes: &[u8]) -> Option<String> {
    if bytes.len() > TEXT_LIMIT || bytes.iter().any(|b| *b < 0x20 && !matches!(*b, b'\t' | b'\r' | b'\n' | 0x0c)) { return None; }
    std::str::from_utf8(bytes).ok().map(|s| s.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n"))
}
fn has_markers(content: &str) -> bool {
    content.lines().any(|l| l.starts_with("<<<<<<<") || l.starts_with("=======") || l.starts_with(">>>>>>>") || l.starts_with("|||||||"))
}

impl WorkspaceService {
    fn update_info(&self, request: &GitUpdateRequest) -> Result<GitUpdateInfo> {
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let repo = repository_status(self, &root)?;
        // No path filtering: updates may change tracked files in excluded build directories too.
        let dirty = !checked(&root, &["status", "--porcelain=v1", "--untracked-files=no", "--ignore-submodules=none"])?.is_empty();
        let tracking = run(&root, &["rev-parse", "--symbolic-full-name", "@{upstream}"], None, false)?;
        let upstream = if tracking.code == 0 { Some(text(&tracking.stdout)?.trim().to_owned()) } else { None };
        let mut token = checked(&root, &["status", "--porcelain=v2", "-z", "--untracked-files=no", "--ignore-submodules=none"])?;
        if let Some(ref upstream) = upstream { token.extend_from_slice(upstream.as_bytes()); }
        token.extend_from_slice(repo.branch.as_bytes());
        token.extend_from_slice(repo.head.as_deref().unwrap_or_default().as_bytes());
        if let Some(metadata) = self.git_cache.metadata(&root) {
            if metadata.git_dir.join("index").is_file() { token.extend(bounded_file(&metadata.git_dir.join("index"), SNAPSHOT_LIMIT)?); }
            for config in [metadata.common_dir.join("config"), metadata.git_dir.join("config.worktree")] {
                if config.is_file() { token.extend(bounded_file(&config, TEXT_LIMIT)?); }
            }
        }
        let reason = repo.blocked.or_else(|| {
            if repo.head.is_none() { Some("仓库还没有提交".into()) }
            else if repo.branch == "(detached)" { Some("Detached HEAD：请先切换到分支".into()) }
            else if upstream.is_none() { Some("当前分支没有配置上游，请先在 Git/IDE 中设置".into()) }
            else if dirty { Some("请先提交或在外部 Stash 本地修改，再更新项目".into()) }
            else { None }
        });
        Ok(GitUpdateInfo { repository: display_path(&root), branch: repo.branch, upstream, version: fingerprint(&token), reason })
    }
    pub fn git_update_check(&self, request: GitUpdateRequest, authenticated: bool) -> Result<GitUpdateInfo> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _gate = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作锁不可用"))?;
        let info = self.update_info(&request)?; self.git_roots(request.expected_revision, true)?; Ok(info)
    }
    pub fn git_update_start(&self, request: GitUpdateRequest, authenticated: bool) -> Result<GitUpdateProgress> {
        self.git_roots(request.expected_revision, authenticated)?;
        let mut slot = self.git_update.lock().map_err(|_| error("unavailable", "更新任务不可用"))?;
        if slot.as_ref().is_some_and(|j| !j.progress.done) { return Err(error("conflict", "已有 Git 更新任务正在执行")); }
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        if request.version.is_none() { return Err(error("conflict", "请先检查更新状态")); }
        let progress = GitUpdateProgress { job_id: uuid::Uuid::new_v4().to_string(), repository: display_path(&root), phase: "queued".into(), done: false, message: "等待更新".into() };
        *slot = Some(UpdateJob { revision: request.expected_revision, progress: progress.clone() });
        let service = self.clone(); let id = progress.job_id.clone();
        let spawned = std::thread::Builder::new().name("git-update".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| service.perform_update(&request, &id)))
                .unwrap_or_else(|_| Err(error("unavailable", "更新任务意外中断，请刷新 Git 状态确认结果")));
            service.git_cache.invalidate_repositories(std::slice::from_ref(&root));
            if let Ok(mut slot) = service.git_update.lock() {
                if let Some(job) = slot.as_mut().filter(|j| j.progress.job_id == id) {
                    let (phase, message) = match result { Ok(value) => value, Err(err) => ("failed".into(), err.message) };
                    job.progress.phase = phase; job.progress.message = message; job.progress.done = true;
                }
            }
        });
        if let Err(err) = spawned { *slot = None; return Err(error("unavailable", format!("无法创建更新任务：{err}"))); }
        Ok(progress)
    }
    fn update_phase(&self, id: &str, phase: &str, message: &str) {
        if let Ok(mut slot) = self.git_update.lock() {
            if let Some(job) = slot.as_mut().filter(|j| j.progress.job_id == id) { job.progress.phase = phase.into(); job.progress.message = message.into(); }
        }
    }
    fn perform_update(&self, request: &GitUpdateRequest, id: &str) -> Result<(String, String)> {
        let _gate = self.git_operations.write().map_err(|_| error("unavailable", "Git 操作锁不可用"))?;
        let info = self.update_info(request)?;
        if request.version.as_deref() != Some(info.version.as_str()) { return Err(error("conflict", "仓库状态已变化，请重新检查")); }
        if let Some(reason) = info.reason { return Err(error("conflict", reason)); }
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let branch = text(&checked(&root, &["symbolic-ref", "--quiet", "HEAD"])?)?.trim().to_owned();
        let remote = text(&checked(&root, &["config", "--get", &format!("branch.{}.remote", info.branch)])?)?.trim().to_owned();
        let source = text(&checked(&root, &["config", "--get", &format!("branch.{}.merge", info.branch)])?)?.trim().to_owned();
        // Remote is a configured name, never a caller-provided URL or command fragment.
        let names = text(&checked(&root, &["remote"])?)?;
        if remote.starts_with('-') || !names.lines().any(|r| r == remote) || !source.starts_with("refs/heads/") || source.contains(['\n', '\r', '\0']) { return Err(error("invalid_query", "上游必须是已配置远端的单一分支")); }
        self.update_phase(id, "fetching", "正在获取上游代码…");
        // Fetch only the configured upstream branch, without recursive submodule downloads.
        let fetch_generation = self.git_cache.generation();
        let fetch_identity = self.git_fetch_identity(&root);
        command::run_with_timeout(&root, &["fetch", "--no-tags", "--no-recurse-submodules", "--", &remote, &source], None, true, Duration::from_secs(180))?.checked()?;
        if let Some(identity) = fetch_identity { self.record_git_fetch(&root, fetch_generation, identity); }
        let target = text(&checked(&root, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])?)?.trim().to_owned();
        let fresh = self.update_info(request)?;
        if fresh.version != info.version || fresh.reason.is_some() || text(&checked(&root, &["symbolic-ref", "--quiet", "HEAD"])?)?.trim() != branch { return Err(error("conflict", "获取期间本地仓库已改变，请重新检查后更新")); }
        self.update_phase(id, "merging", "正在合并到当前分支…");
        // Block workspace switching and file auto-saves during the worktree mutation.
        let state = self.lock()?;
        if state.revision != request.expected_revision { return Err(error("conflict", "工作区已切换，未执行合并")); }
        let old = checked(&root, &["rev-parse", "HEAD"])?;
        let merged = run(&root, &["-c", "merge.autoStash=false", "-c", "merge.conflictStyle=diff3", "-c", "submodule.recurse=false", "merge", "--ff", "--commit", "--no-squash", "--no-edit", "--no-autostash", &target], None, true)?;
        drop(state);
        if !unmerged(&root)?.is_empty() { return Ok(("conflicts".into(), "更新遇到冲突，请打开 Merge 处理".into())); }
        merged.checked()?;
        if self.git_cache.metadata(&root).is_some_and(|m| m.git_dir.join("MERGE_HEAD").is_file()) {
            return Ok(("conflicts".into(), "合并尚待完成，请打开 Merge 检查并完成合并".into()));
        }
        self.discovery.lock().map_err(|_| error("unavailable", "服务发现缓存不可用"))?.clear();
        let unchanged = old == checked(&root, &["rev-parse", "HEAD"])?;
        Ok(("completed".into(), if unchanged { "已是最新".into() } else { "更新完成".into() }))
    }
    pub fn git_update_progress(&self, request: GitUpdateProgressRequest, authenticated: bool) -> Result<Option<GitUpdateProgress>> {
        self.git_roots(request.expected_revision, authenticated)?;
        let slot = self.git_update.lock().map_err(|_| error("unavailable", "更新任务不可用"))?;
        let Some(job) = slot.as_ref().filter(|j| j.revision == request.expected_revision && request.job_id.as_ref().is_none_or(|id| *id == j.progress.job_id)) else { return Ok(None); };
        self.git_repository(&job.progress.repository, request.expected_revision)?;
        Ok(Some(job.progress.clone()))
    }
    fn merge_state(&self, root: &Path) -> Result<GitMergeState> {
        let metadata = self.git_cache.metadata(root).ok_or_else(|| error("unavailable", "仓库元数据不可用"))?;
        let active = metadata.git_dir.join("MERGE_HEAD").is_file();
        let branch = text(&run(root, &["symbolic-ref", "--short", "--quiet", "HEAD"], None, false)?.stdout)?.trim().to_owned();
        let mut state = GitMergeState { repository: display_path(root), branch, active, version: String::new(), files: Vec::new(), message: String::new(), changed_paths: Vec::new() };
        if !active { return Ok(state); }
        if ["rebase-merge", "rebase-apply", "CHERRY_PICK_HEAD", "REVERT_HEAD", "MERGE_AUTOSTASH"].iter().any(|n| metadata.git_dir.join(n).exists()) { return Err(error("conflict", "当前操作包含变基、拣选或外部自动 Stash，请在 Git/IDE 中继续")); }
        let conflicts = unmerged(root)?;
        state.files = conflicts.iter().map(|f| MergeFile { path: f.path.clone(), local_deleted: f.entries[1].is_none(), incoming_deleted: f.entries[2].is_none(), supported: ordinary(f) && self.git_path(root, &f.path).is_ok() }).collect();
        let mut token = bounded_file(&metadata.git_dir.join("index"), SNAPSHOT_LIMIT)?;
        token.extend(checked(root, &["rev-parse", "HEAD"])?);
        token.extend(bounded_file(&metadata.git_dir.join("MERGE_HEAD"), 1024)?);
        state.message = bounded_file(&metadata.git_dir.join("MERGE_MSG"), 64 * 1024).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_else(|| "Merge upstream".into());
        let changed = checked(root, &["diff", "--name-only", "-z", "HEAD", "--"])?;
        let mut paths: Vec<String> = changed.split(|b| *b == 0).filter(|p| !p.is_empty()).map(text).collect::<Result<_>>()?;
        for f in &conflicts { if !paths.contains(&f.path) { paths.push(f.path.clone()); } }
        if paths.len() > 2000 { return Err(error("limit_exceeded", "合并文件超过 2000 个，请在外部处理")); }
        for relative in paths {
            // Unsupported paths remain visible, but are never read or written through the app.
            if let Ok(path) = self.git_path(root, &relative) {
                token.extend_from_slice(relative.as_bytes()); token.push(0);
                if path.is_file() && !fs::symlink_metadata(&path)?.file_type().is_symlink() {
                    let bytes = bounded_file(&path, SNAPSHOT_LIMIT)?;
                    token.extend_from_slice(&(bytes.len() as u64).to_le_bytes()); token.extend(bytes);
                } else { token.extend_from_slice(&u64::MAX.to_le_bytes()); }
                if token.len() > SNAPSHOT_LIMIT { return Err(error("limit_exceeded", "合并快照超过 32 MiB，请在外部处理")); }
            }
            state.changed_paths.push(relative);
        }
        state.version = fingerprint(&token); Ok(state)
    }
    pub fn git_merge_status(&self, request: GitMergeRequest, authenticated: bool) -> Result<GitMergeState> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _gate = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作锁不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let value = self.merge_state(&root)?; self.git_roots(request.expected_revision, true)?; Ok(value)
    }
    pub fn git_merge_file(&self, request: GitMergeRequest, authenticated: bool) -> Result<GitConflictContent> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _gate = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作锁不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let state = self.merge_state(&root)?;
        if !state.active { return Err(error("conflict", "当前没有进行中的 Merge")); }
        let relative = request.path.ok_or_else(|| error("invalid_query", "缺少冲突路径"))?;
        let file = unmerged(&root)?.into_iter().find(|f| f.path == relative).ok_or_else(|| error("conflict", "该文件已解决或冲突状态已改变"))?;
        let path = self.git_path(&root, &relative)?;
        let mut value = GitConflictContent { path: relative, version: state.version.clone(), base: None, local: None, incoming: None, result: None, reason: None, local_deleted: file.entries[1].is_none(), incoming_deleted: file.entries[2].is_none() };
        if !ordinary(&file) { value.reason = Some("重命名、符号链接或子模块等特殊冲突，请使用外部工具处理".into()); return Ok(value); }
        let bytes: Vec<_> = file.entries.iter().map(|e| blob(&root, e)).collect::<Result<_>>()?;
        let texts: Vec<_> = bytes.iter().map(|b| b.as_deref().map(editable).unwrap_or_else(|| Some(String::new()))).collect();
        let disk = if path.exists() { Some(bounded_file(&path, 8 * TEXT_LIMIT)?) } else { None };
        if texts.iter().any(Option::is_none) || disk.as_deref().is_some_and(|b| editable(b).is_none()) {
            value.reason = Some("二进制、非 UTF-8 或超过 1 MiB，仅支持整文件选择本地/远端版本".into());
        } else {
            value.base = texts[0].clone(); value.local = texts[1].clone(); value.incoming = texts[2].clone();
            value.result = Some(disk.as_deref().and_then(editable).unwrap_or_default());
        }
        if self.merge_state(&root)?.version != state.version { return Err(error("conflict", "读取期间合并状态已改变，请刷新")); }
        self.git_roots(request.expected_revision, true)?; Ok(value)
    }
    pub fn git_merge_resolve(&self, request: GitResolveRequest, authenticated: bool) -> Result<GitMergeState> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _gate = self.git_operations.write().map_err(|_| error("unavailable", "Git 操作锁不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let current = self.merge_state(&root)?;
        if !current.active || current.version != request.version { return Err(error("conflict", "合并状态或文件已改变，请重新读取")); }
        let file = unmerged(&root)?.into_iter().find(|f| f.path == request.path).ok_or_else(|| error("conflict", "文件不在冲突列表"))?;
        if !ordinary(&file) { return Err(error("unsupported_file", "特殊冲突请在外部处理")); }
        let path = self.git_path(&root, &request.path)?;
        let original = if path.exists() { Some(bounded_file(&path, 8 * TEXT_LIMIT)?) } else { None };
        let selected_entry = match request.choice.as_str() { "incoming" => file.entries[2].as_ref(), _ => file.entries[1].as_ref().or(file.entries[2].as_ref()) };
        let bytes = match request.choice.as_str() {
            "local" => blob(&root, &file.entries[1])?,
            "incoming" => blob(&root, &file.entries[2])?,
            "delete" if file.entries[1].is_none() || file.entries[2].is_none() => None,
            "edit" => {
                let content = request.content.as_deref().ok_or_else(|| error("invalid_query", "缺少合并内容"))?;
                if editable(content.as_bytes()).is_none() || has_markers(content) { return Err(error("conflict", "合并结果不是受支持的文本或仍包含冲突标记")); }
                let template = original.as_deref();
                let normalized = content.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
                let mut content = if template.is_some_and(|b| b.windows(2).any(|w| w == b"\r\n")) { normalized.replace('\n', "\r\n") }
                    else if template.is_some_and(|b| b.contains(&b'\r')) { normalized.replace('\n', "\r") } else { normalized };
                if template.is_some_and(|b| b.starts_with(&[0xef, 0xbb, 0xbf])) { content.insert(0, '\u{feff}'); }
                Some(content.into_bytes())
            },
            _ => return Err(error("invalid_query", "无效的冲突解决方式")),
        };
        let state = self.lock()?;
        if state.revision != request.expected_revision { return Err(error("conflict", "工作区已改变")); }
        self.validate_merge_write(&state, &path)?;
        if self.git_path(&root, &request.path)? != path { return Err(error("conflict", "目标路径已改变")); }
        if let Some(ref bytes) = bytes {
            let directory = path.parent().ok_or_else(|| error("invalid_path", "无效文件路径"))?;
            if !directory.is_dir() { return Err(error("unsupported_file", "目标目录不存在，请在外部处理目录冲突")); }
            let temp = directory.join(format!(".devflow-merge-{}.tmp", uuid::Uuid::new_v4()));
            let result = (|| -> Result<()> {
                let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
                out.write_all(bytes)?; out.sync_all()?; drop(out);
                if path.exists() { fs::set_permissions(&temp, fs::metadata(&path)?.permissions())?; }
                #[cfg(unix)]
                if let Some((mode, _)) = selected_entry {
                    use std::os::unix::fs::PermissionsExt;
                    let mut permissions = fs::metadata(&temp)?.permissions();
                    let bits = permissions.mode(); permissions.set_mode(if mode == "100755" { bits | 0o111 } else { bits & !0o111 });
                    fs::set_permissions(&temp, permissions)?;
                }
                if self.merge_state(&root)?.version != request.version { return Err(error("conflict", "写入前合并状态已改变")); }
                fs::rename(&temp, &path)?; Ok(())
            })();
            if result.is_err() { let _ = fs::remove_file(&temp); } result?;
        } else if path.exists() {
            if self.merge_state(&root)?.version != request.version { return Err(error("conflict", "删除前合并状态已改变")); }
            fs::remove_file(&path)?;
        }
        // A failed add intentionally leaves the saved worktree intact and the conflict unresolved.
        let spec = literal(&request.path);
        let staged = (|| -> Result<()> {
            run(&root, &["add", "-A", "--", &spec], None, true)?.checked()?;
            if bytes.is_some() {
                if let Some((mode, _)) = selected_entry {
                    run(&root, &["update-index", if mode == "100755" { "--chmod=+x" } else { "--chmod=-x" }, "--", &request.path], None, true)?.checked()?;
                }
            }
            Ok(())
        })();
        drop(state);
        self.git_cache.invalidate_repositories(std::slice::from_ref(&root)); staged?;
        self.merge_state(&root)
    }
    pub fn git_merge_finish(&self, request: GitMergeRequest, authenticated: bool) -> Result<GitMergeState> { self.merge_complete(request, authenticated, false) }
    pub fn git_merge_abort(&self, request: GitMergeRequest, authenticated: bool) -> Result<GitMergeState> { self.merge_complete(request, authenticated, true) }
    fn merge_complete(&self, request: GitMergeRequest, authenticated: bool, abort: bool) -> Result<GitMergeState> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _gate = self.git_operations.write().map_err(|_| error("unavailable", "Git 操作锁不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let current = self.merge_state(&root)?;
        if !current.active || request.version.as_deref() != Some(current.version.as_str()) { return Err(error("conflict", "合并状态已改变，请刷新后重试")); }
        if !abort && !current.files.is_empty() { return Err(error("conflict", "还有未解决的冲突文件")); }
        let unstaged = checked(&root, &["diff-files", "--name-only", "-z", "--"])?;
        for p in unstaged.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            if !current.files.iter().any(|f| f.path.as_bytes() == p) { return Err(error("conflict", "存在合并后额外的未暂存修改，请先在 Git/IDE 中处理")); }
        }
        let state = self.lock()?;
        if state.revision != request.expected_revision { return Err(error("conflict", "工作区已改变")); }
        let result = if abort { run(&root, &["-c", "merge.autoStash=false", "merge", "--abort"], None, true)?.checked() } else {
            let message = request.message.as_deref().unwrap_or(&current.message);
            if message.trim().is_empty() || message.len() > 64 * 1024 || message.contains('\0') { return Err(error("invalid_query", "请填写有效的合并提交信息（最多 64 KiB）")); }
            run(&root, &["commit", "--file=-"], Some(message.as_bytes().to_vec()), true)?.checked()
        };
        drop(state); self.git_cache.invalidate_repositories(std::slice::from_ref(&root)); result?;
        self.discovery.lock().map_err(|_| error("unavailable", "服务发现缓存不可用"))?.clear();
        self.merge_state(&root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stages_preserve_chinese_paths_and_missing_base() {
        let raw = format!("100644 {} 2\t中文 文件.txt\0{}", "a".repeat(40), format!("100755 {} 3\t中文 文件.txt\0", "b".repeat(40)));
        let files = parse_stages(raw.as_bytes()).unwrap();
        assert_eq!(files.len(), 1); assert!(files[0].entries[0].is_none()); assert!(ordinary(&files[0]));
        assert_eq!(files[0].path, "中文 文件.txt");
        assert!(parse_stages(b"100644 bad 4\tx\0").is_err());
    }
    #[test]
    fn binary_and_unresolved_markers_cannot_be_saved_as_text() {
        assert!(editable(b"a\0b").is_none()); assert!(editable(&vec![b'a'; TEXT_LIMIT + 1]).is_none());
        assert_eq!(editable("\u{feff}中文\r\n".as_bytes()).unwrap(), "中文\n");
        assert!(has_markers("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> branch\n"));
        assert!(!has_markers("ordinary < text\n"));
    }
}
