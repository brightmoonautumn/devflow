use super::*;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection { pub repository: String, pub version: String, pub paths: Vec<String> }
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mutation { Add, Rollback }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitMutationRequest { pub expected_revision: u64, pub selections: Vec<Selection>, pub operation: Mutation }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCommitRequest { pub expected_revision: u64, pub selections: Vec<Selection>, pub message: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCommitApplyRequest { pub expected_revision: u64, pub plan_id: String }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoOutcome { pub repository: String, pub paths: Vec<String>, pub commit: Option<String>, pub error: Option<WorkspaceError> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOutcome { pub results: Vec<RepoOutcome> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRepo { pub repository: String, pub branch: String, pub files: Vec<GitDiff> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitPreview { pub plan_id: String, pub revision: u64, pub message: String, pub repositories: Vec<PreviewRepo> }
pub(in super::super) struct CommitPlan { id: String, revision: u64, message: String, created: Instant, repos: Vec<PreparedRepo> }
struct PreparedRepo { root: PathBuf, repo: GitRepository, files: Vec<PreparedFile> }
struct PreparedFile { change: GitChange, bytes: Option<Vec<u8>> }

fn selected(service: &WorkspaceService, selection: &Selection, revision: u64) -> Result<PreparedRepo> {
    if selection.paths.is_empty() || selection.paths.len() > 500 { return Err(error("limit_exceeded", "每个仓库请选择 1–500 个文件")); }
    let root = service.git_repository(&selection.repository, revision)?;
    let repo = repository_status(service, &root)?;
    if repo.version != selection.version { return Err(error("conflict", "Git 状态已变化，请刷新并重新选择")); }
    if let Some(reason) = &repo.blocked { return Err(error("conflict", reason.clone())); }
    if repo.truncated { return Err(error("limit_exceeded", "仓库变更列表不完整，请缩小仓库范围或先处理异常文件")); }
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut bytes = 0;
    for path in &selection.paths {
        if !seen.insert(path) { continue; }
        let change = repo.changes.iter().find(|entry| &entry.path == path).ok_or_else(|| error("conflict", "所选文件已不在变更列表中"))?;
        if !change.selectable { return Err(error("invalid_path", change.reason.clone().unwrap_or_else(|| "该文件不可操作".into()))); }
        let snapshot = working_bytes(service, &root, change)?;
        bytes += snapshot.as_ref().map_or(0, Vec::len);
        if bytes > 16 * 1024 * 1024 { return Err(error("limit_exceeded", "单个仓库所选文件快照超过 16 MiB，请分批处理")); }
        files.push(PreparedFile { change: change.clone(), bytes: snapshot });
    }
    Ok(PreparedRepo { root, repo, files })
}
fn selection_paths(prepared: &PreparedRepo) -> Vec<String> {
    let mut paths = Vec::new();
    for file in &prepared.files {
        if let Some(original) = &file.change.original_path { if !paths.contains(original) { paths.push(original.clone()); } }
        if !paths.contains(&file.change.path) { paths.push(file.change.path.clone()); }
    }
    paths
}
fn preflight(service: &WorkspaceService, prepared: &PreparedRepo, revision: u64) -> Result<()> {
    service.git_repository(&prepared.repo.root, revision)?;
    let current = repository_status(service, &prepared.root)?;
    if current.head != prepared.repo.head || current.version != prepared.repo.version {
        return Err(error("conflict", "仓库 HEAD、索引或工作区状态已变化，请重新预览"));
    }
    for file in &prepared.files {
        if working_bytes(service, &prepared.root, &file.change)? != file.bytes { return Err(error("file_conflict", format!("{} 在预览后被修改，请重新预览", file.change.path))); }
    }
    Ok(())
}
fn validate_selections(selections: &[Selection]) -> Result<()> {
    if selections.is_empty() || selections.len() > 32 { return Err(error("limit_exceeded", "每次请选择 1–32 个仓库")); }
    let mut seen = HashSet::new();
    for selection in selections {
        if !seen.insert(comparison_path(Path::new(&selection.repository))) { return Err(error("invalid_path", "同一仓库不能重复提交操作")); }
    }
    Ok(())
}
impl WorkspaceService {
    pub fn git_mutate(&self, request: GitMutationRequest, authenticated: bool) -> Result<GitOutcome> {
        self.git_roots(request.expected_revision, authenticated)?;
        validate_selections(&request.selections)?;
        let _operations = self.git_operations.lock().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let mut prepared = Vec::new();
        for selection in &request.selections {
            let repo = selected(self, selection, request.expected_revision)?;
            for file in &repo.files {
                match request.operation {
                    Mutation::Add if !file.change.untracked => return Err(error("invalid_path", "Add to VCS 仅用于未跟踪文件")),
                    Mutation::Rollback if file.change.untracked || file.change.submodule => return Err(error("invalid_path", "未跟踪文件不删除；子模块指针请在子仓库中处理，不自动切换其 HEAD")),
                    _ => {}
                }
                if matches!(request.operation, Mutation::Rollback) {
                    let path = self.git_path(&repo.root, &file.change.path)?;
                    if fs::metadata(&path).is_ok_and(|metadata| metadata.permissions().readonly()) { return Err(error("permission_denied", "只读文件不能回滚")); }
                    let state = self.lock()?;
                    if state.session.as_ref().and_then(|session| session.file.as_deref()).is_some_and(|config| comparison_path(config) == comparison_path(&path)) {
                        return Err(error("permission_denied", "当前工作区配置不能通过 Git 面板回滚"));
                    }
                    if let Some(original) = &file.change.original_path {
                        if self.git_path(&repo.root, original)?.exists() { return Err(error("file_conflict", "重命名前的路径已存在文件，请先处理后再回滚")); }
                    }
                }
            }
            prepared.push(repo);
        }
        for repo in &prepared { preflight(self, repo, request.expected_revision)?; }
        let mut results = Vec::new();
        for repo in prepared {
            let paths = selection_paths(&repo);
            let mut completed_paths = Vec::new();
            let result = (|| -> Result<()> {
                preflight(self, &repo, request.expected_revision)?;
                match request.operation {
                    Mutation::Add => { run(&repo.root, &["add", "--pathspec-from-file=-", "--pathspec-file-nul"], Some(command::pathspecs(&paths)), true)?.checked()?; completed_paths = paths.clone(); }
                    Mutation::Rollback => {
                        // Per-file commands keep newly added disk files intact. Each
                        // failed batch reports already-completed paths below.
                        for file in &repo.files {
                            if file.change.head_mode == "000000" {
                                let spec = literal(&file.change.path);
                                run(&repo.root, &["rm", "--cached", "--force", "--", &spec], None, true)?.checked()?;
                            } else {
                                let paths: Vec<_> = file.change.original_path.iter().cloned().chain(std::iter::once(file.change.path.clone())).collect();
                                run(&repo.root, &["restore", "--source=HEAD", "--staged", "--worktree", "--pathspec-from-file=-", "--pathspec-file-nul"], Some(command::pathspecs(&paths)), true)?.checked()?;
                            }
                            completed_paths.push(file.change.path.clone());
                            if let Some(old) = &file.change.original_path { completed_paths.push(old.clone()); }
                        }
                    }
                }
                Ok(())
            })();
            results.push(RepoOutcome { repository: repo.repo.root, paths: completed_paths, commit: None, error: result.err() });
        }
        Ok(GitOutcome { results })
    }

    pub fn git_commit_preview(&self, request: GitCommitRequest, authenticated: bool) -> Result<GitCommitPreview> {
        self.git_roots(request.expected_revision, authenticated)?;
        validate_selections(&request.selections)?;
        if request.message.trim().is_empty() || request.message.len() > 32 * 1024 || request.message.contains('\0') { return Err(error("invalid_query", "请输入不超过 32 KiB 的提交信息")); }
        let _operations = self.git_operations.lock().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let mut repos = Vec::new(); let mut previews = Vec::new(); let mut bytes = 0;
        for selection in &request.selections {
            let repo = selected(self, selection, request.expected_revision)?;
            if repo.repo.branch == "(detached)" { return Err(error("conflict", "子仓库当前处于 detached HEAD，请先在 Git/IDE 中切换到目标分支再提交")); }
            let mut diffs = Vec::new();
            for file in &repo.files {
                if file.change.untracked { return Err(error("invalid_path", "未跟踪文件请先 Add to VCS，再勾选提交")); }
                bytes += file.bytes.as_ref().map_or(0, Vec::len);
                let diff = diff_for(self, &repo.root, &repo.repo, &file.change)?;
                bytes += diff.before.len() + diff.after.len();
                if bytes > 16 * 1024 * 1024 { return Err(error("limit_exceeded", "提交预览与快照合计超过 16 MiB，请分批提交")); }
                diffs.push(diff);
            }
            preflight(self, &repo, request.expected_revision)?;
            previews.push(PreviewRepo { repository: repo.repo.root.clone(), branch: repo.repo.branch.clone(), files: diffs });
            repos.push(repo);
        }
        for parent in &repos {
            for file in parent.files.iter().filter(|file| file.change.submodule) {
                let child = parent.root.join(&file.change.path).canonicalize()?;
                if repos.iter().any(|repo| repo.root.starts_with(&child)) {
                    return Err(error("conflict", "同一批不能同时提交子仓库与其父指针。请先提交子仓库，再刷新、预览并提交父指针。"));
                }
            }
        }
        repos.sort_by_key(|repo| repo.root.components().count());
        let id = uuid::Uuid::new_v4().to_string();
        *self.git_plan.lock().map_err(|_| error("unavailable", "提交计划暂不可用"))? = Some(CommitPlan { id: id.clone(), revision: request.expected_revision, message: request.message.clone(), created: Instant::now(), repos });
        Ok(GitCommitPreview { plan_id: id, revision: request.expected_revision, message: request.message, repositories: previews })
    }

    pub fn git_commit_apply(&self, request: GitCommitApplyRequest, authenticated: bool) -> Result<GitOutcome> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _operations = self.git_operations.lock().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let mut stored = self.git_plan.lock().map_err(|_| error("unavailable", "提交计划暂不可用"))?;
        let plan = stored.as_ref().ok_or_else(|| error("conflict", "提交预览已失效，请重新预览"))?;
        if plan.id != request.plan_id || plan.revision != request.expected_revision || plan.created.elapsed() > Duration::from_secs(300) { return Err(error("conflict", "提交预览过期或工作区已改变")); }
        for repo in &plan.repos { preflight(self, repo, request.expected_revision)?; }
        let plan = stored.take().ok_or_else(|| error("conflict", "提交预览已失效"))?;
        let mut results = Vec::new();
        for repo in plan.repos {
            let paths = selection_paths(&repo);
            let result = (|| -> Result<String> {
                preflight(self, &repo, request.expected_revision)?;
                // Git's built-in partial commit locks and reconciles the real
                // index, preserving staged changes belonging to unchecked files.
                // A message temp file keeps pathspec NUL input on stdin.
                let message_path = std::env::temp_dir().join(format!("devflow-git-message-{}.txt", uuid::Uuid::new_v4()));
                let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&message_path)?;
                let written = file.write_all(plan.message.as_bytes()).and_then(|_| file.sync_all()); drop(file);
                let committed = written.map_err(WorkspaceError::from).and_then(|_| {
                    let message_name = message_path.to_str().ok_or_else(|| error("unsupported_file", "临时目录不是 UTF-8"))?;
                    run(&repo.root, &["commit", "--only", "--file", message_name, "--pathspec-from-file=-", "--pathspec-file-nul"], Some(command::pathspecs(&paths)), true).and_then(|output| output.checked())
                });
                let _ = fs::remove_file(&message_path);
                committed?;
                Ok(text(&checked(&repo.root, &["rev-parse", "--verify", "HEAD"])?)?.trim().to_owned())
            })();
            match result {
                Ok(commit) => results.push(RepoOutcome { repository: repo.repo.root, paths, commit: Some(commit), error: None }),
                Err(err) => { results.push(RepoOutcome { repository: repo.repo.root, paths: Vec::new(), commit: None, error: Some(err) }); break; }
            }
        }
        Ok(GitOutcome { results })
    }
}
