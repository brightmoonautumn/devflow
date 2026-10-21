//! Local upstream comparisons and explicit, fetch-only remote refresh.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitSyncRequest { pub expected_revision: u64, #[serde(default)] pub repositories: Vec<String> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitFetchRequest { pub expected_revision: u64, pub repository: String, pub version: String }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSyncInfo {
    pub repository: String, pub branch: String, pub upstream: Option<String>, pub ahead: Option<u64>, pub behind: Option<u64>,
    pub last_fetched_at: Option<u64>, pub version: String, pub reason: Option<String>, pub can_fetch: bool,
    #[serde(skip)] fetch_identity: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSync { pub repositories: Vec<GitSyncInfo>, pub warnings: Vec<String> }

impl WorkspaceService {
    pub(super) fn git_fetch_identity(&self, root: &Path) -> Option<String> { self.sync_info(root).ok().map(|info| info.fetch_identity) }
    pub(super) fn record_git_fetch(&self, root: &Path, generation: u64, identity: String) {
        if let Ok(time) = SystemTime::now().duration_since(UNIX_EPOCH) { self.git_cache.remember_fetch(generation, root, identity, time.as_secs()); }
    }
    fn sync_info(&self, root: &Path) -> Result<GitSyncInfo> {
        // This does not scan the index or working tree, and never contacts a remote.
        let symbolic = run(root, &["symbolic-ref", "--quiet", "HEAD"], None, false)?;
        let branch_ref = if symbolic.code == 0 { text(&symbolic.stdout)?.trim().to_owned() } else { String::new() };
        let branch = branch_ref.strip_prefix("refs/heads/").unwrap_or("(detached)").to_owned();
        let refs = checked(root, &["for-each-ref", "--format=%(refname)%00%(objectname)%00%(upstream)%00%(upstream:remotename)%00%(upstream:remoteref)", "refs/heads/", "refs/remotes/"])?;
        let output = text(&refs)?;
        let row = output.lines().map(|line| line.split('\0').collect::<Vec<_>>()).find(|parts| parts.len() == 5 && parts[0] == branch_ref);
        let upstream = row.as_ref().and_then(|parts| (!parts[2].is_empty()).then(|| parts[2].to_owned()));
        let mut token = refs; token.extend_from_slice(branch_ref.as_bytes());
        let mut fetch_identity = String::new();
        if let Some(row) = &row {
            // Destination/config changes invalidate fetch preflight, not just branch tips.
            let config = run(root, &["config", "--get-regexp", "^(remote\\..*\\.(url|fetch)|branch\\..*\\.(remote|merge))$"], None, false)?;
            if !matches!(config.code, 0 | 1) { config.checked()?; }
            else { token.extend_from_slice(&config.stdout); fetch_identity = fingerprint(&config.stdout); }
            token.extend_from_slice(row[1].as_bytes());
        }
        let version = fingerprint(&token);
        let fetch_identity = format!("{}:{fetch_identity}", upstream.as_deref().unwrap_or_default());
        let mut info = GitSyncInfo { repository: display_path(root), branch, upstream: upstream.clone(), ahead: None, behind: None,
            last_fetched_at: self.git_cache.fetched_at(root, &fetch_identity), fetch_identity, version, reason: None, can_fetch: false };
        let Some(row) = row else { info.reason = Some(if branch_ref.is_empty() { "Detached HEAD：请先切换到分支" } else { "仓库还没有提交" }.into()); return Ok(info); };
        let Some(upstream) = upstream else { info.reason = Some("未设置上游".into()); return Ok(info); };
        let remotes = text(&checked(root, &["remote"])?)?;
        info.can_fetch = upstream.starts_with("refs/remotes/") && !row[3].starts_with('-') && remotes.lines().any(|name| name == row[3]) && row[4].starts_with("refs/heads/");
        let known = output.lines().map(|line| line.split('\0').collect::<Vec<_>>()).find(|parts| parts.len() == 5 && parts[0] == upstream);
        let Some(known) = known else { info.reason = Some("上游引用尚不可用，请先获取远端状态".into()); return Ok(info); };
        // Local upstreams can be compared, but never fetch from an arbitrary path or URL.
        let count = run(root, &["rev-list", "--left-right", "--count", &format!("{}...{}", row[1], known[1]), "--"], None, false)?;
        if count.code == 0 {
            let values = text(&count.stdout)?;
            let values: Vec<_> = values.split_whitespace().collect();
            if values.len() == 2 { info.ahead = values[0].parse().ok(); info.behind = values[1].parse().ok(); }
        }
        if info.ahead.is_none() || info.behind.is_none() { info.reason = Some("上游引用尚不可用，请先获取远端状态".into()); }
        if !info.can_fetch && info.reason.is_none() { info.reason = Some("上游不是已配置远端的分支，仅展示本地比较".into()); }
        Ok(info)
    }
    pub fn git_sync(&self, request: GitSyncRequest, authenticated: bool) -> Result<GitSync> {
        self.git_roots(request.expected_revision, authenticated)?;
        let (roots, warnings) = self.git_selected_roots(request.expected_revision, &request.repositories)?;
        let repositories = overview::parallel_roots(&roots, |root| {
            let _gate = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
            let root = self.git_repository(&display_path(root), request.expected_revision)?;
            match self.sync_info(&root) {
                Ok(info) => Ok(info),
                Err(err) => Ok(GitSyncInfo { repository: display_path(&root), branch: String::new(), upstream: None, ahead: None, behind: None, last_fetched_at: None, fetch_identity: String::new(), version: String::new(), reason: Some(err.message), can_fetch: false }),
            }
        })?;
        self.git_roots(request.expected_revision, true)?; Ok(GitSync { repositories, warnings })
    }
    pub fn git_fetch(&self, request: GitFetchRequest, authenticated: bool) -> Result<GitSyncInfo> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _gate = self.git_operations.write().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let info = self.sync_info(&root)?;
        if request.version.is_empty() || info.version != request.version { return Err(error("conflict", "分支或远端配置已变化，请刷新远端状态后重试")); }
        if !info.can_fetch { return Err(error("invalid_query", info.reason.unwrap_or_else(|| "请先设置远端上游".into()))); }
        let target = push::target(&root)?;
        let upstream = info.upstream.ok_or_else(|| error("invalid_query", "未设置上游"))?;
        let refspec = format!("+{}:{}", target.destination, upstream);
        // Only remote-tracking refs are mutable here: never fetch into a local branch.
        if !upstream.starts_with("refs/remotes/") { return Err(error("invalid_query", "仅允许获取到当前分支的远程跟踪引用")); }
        let generation = self.git_cache.generation();
        let _invalidate = cache::Invalidate { cache: &self.git_cache, roots: vec![root.clone()] };
        self.git_roots(request.expected_revision, true)?;
        if self.sync_info(&root)?.version != request.version { return Err(error("conflict", "获取前仓库状态已变化，请刷新后重试")); }
        command::run_with_timeout(&root, &["fetch", "--atomic", "--no-tags", "--no-recurse-submodules", "--", &target.remote, &refspec], None, true, Duration::from_secs(120))?.checked()?;
        self.git_roots(request.expected_revision, true)?;
        self.record_git_fetch(&root, generation, info.fetch_identity);
        self.sync_info(&root)
    }
}
