//! Push exactly one inspected commit to the current branch's configured upstream.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitPushRequest { pub expected_revision: u64, pub repository: String, pub expected_head: String }

#[derive(PartialEq, Debug)]
pub(super) struct Target { pub(super) branch: String, pub(super) remote: String, pub(super) destination: String }

fn single_config(root: &Path, key: &str) -> Result<String> {
    let output = run(root, &["config", "--get-all", key], None, false)?;
    let value = text(&output.stdout)?;
    if output.code != 0 || value.lines().count() != 1 || value.trim().is_empty() {
        return Err(error("invalid_query", "当前分支需要配置唯一上游远端和分支，请先在 Git/IDE 中设置"));
    }
    Ok(value.trim().to_owned())
}

pub(super) fn target(root: &Path) -> Result<Target> {
    let output = run(root, &["symbolic-ref", "--quiet", "HEAD"], None, false)?;
    if output.code != 0 { return Err(error("conflict", "Detached HEAD：请先切换到分支再推送")); }
    let branch = text(&output.stdout)?.trim().to_owned();
    let name = branch.strip_prefix("refs/heads/").ok_or_else(|| error("conflict", "当前 HEAD 不是本地分支"))?;
    let remote = single_config(root, &format!("branch.{name}.remote"))?;
    let destination = single_config(root, &format!("branch.{name}.merge"))?;
    let remotes = text(&checked(root, &["remote"])?)?;
    if remote.starts_with('-') || !remotes.lines().any(|name| name == remote)
        || !destination.starts_with("refs/heads/") || destination.contains(['\n', '\r', '\0']) {
        return Err(error("invalid_query", "推送目标必须是已配置远端的单一上游分支"));
    }
    checked(root, &["check-ref-format", &destination])?;
    Ok(Target { branch, remote, destination })
}

pub(super) fn send(root: &Path, head: &str, expected: &Target) -> Result<()> {
    if !matches!(head.len(), 40 | 64) || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(error("invalid_query", "推送需要完整的提交 SHA"));
    }
    if &target(root)? != expected || text(&checked(root, &["rev-parse", "--verify", "HEAD"])?)?.trim() != head {
        return Err(error("conflict", "当前分支、上游或 HEAD 已改变，未执行推送"));
    }
    let refspec = format!("{head}:{}", expected.destination);
    // Explicit SHA/refspec, no force, tags, mirror or recursive submodule pushes.
    run(root, &["-c", &format!("remote.{}.mirror=false", expected.remote), "push", "--porcelain", "--no-force", "--no-follow-tags", "--recurse-submodules=no", "--", &expected.remote, &refspec], None, true)?.checked()?;
    Ok(())
}

impl WorkspaceService {
    pub fn git_push(&self, request: GitPushRequest, authenticated: bool) -> Result<GitOutcome> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _gate = self.git_operations.write().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let _invalidate = cache::Invalidate { cache: &self.git_cache, roots: vec![root.clone()] };
        let repo = repository_status(self, &root)?;
        if let Some(reason) = repo.blocked { return Err(error("conflict", reason)); }
        let destination = target(&root)?;
        self.git_roots(request.expected_revision, true)?;
        let result = send(&root, &request.expected_head, &destination);
        Ok(GitOutcome { results: vec![operations::RepoOutcome {
            repository: display_path(&root), paths: Vec::new(), commit: Some(request.expected_head),
            error: None, pushed: result.is_ok(), push_error: result.err(),
        }] })
    }
}
