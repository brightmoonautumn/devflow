//! Workspace-scoped Git porcelain. Parent gitlinks and child repositories never
//! share a selection or an index. HTTP and MCP use these same operations.
use super::*;
mod command;
mod operations;
#[cfg(test)]
mod tests;
use command::{checked, literal, run, text};
pub use operations::{GitMutationRequest, GitCommitRequest, GitCommitApplyRequest, GitOutcome, GitCommitPreview};
pub(super) use operations::CommitPlan;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitStatusRequest { pub expected_revision: u64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitDiffRequest {
    pub expected_revision: u64, pub repository: String, pub version: String, pub path: String,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    pub path: String, pub original_path: Option<String>, pub status: String,
    pub kind: String, pub untracked: bool, pub submodule: bool,
    pub pointer_changed: bool, pub submodule_dirty: bool,
    pub selectable: bool, pub reason: Option<String>,
    pub head_oid: String, pub index_oid: String, pub head_mode: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepository {
    pub root: String, pub name: String, pub branch: String, pub head: Option<String>,
    pub parent: Option<String>, pub submodule_path: Option<String>, pub version: String,
    pub changes: Vec<GitChange>, pub blocked: Option<String>, pub truncated: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus { pub revision: u64, pub repositories: Vec<GitRepository>, pub warnings: Vec<String> }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub repository: String, pub path: String, pub original_path: Option<String>,
    pub before: String, pub after: String, pub message: Option<String>, pub submodule: bool,
}

fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes { hash ^= *byte as u64; hash = hash.wrapping_mul(0x100000001b3); }
    format!("{hash:016x}")
}

impl WorkspaceService {
    fn git_roots(&self, revision: u64, authenticated: bool) -> Result<Vec<PathBuf>> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let state = self.lock()?;
        if state.revision != revision { return Err(error("conflict", "工作区已改变，请刷新 Git 状态")); }
        let session = state.session.as_ref().ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
        let document = parse_document(&session.source)?;
        let mut roots = Vec::new();
        for entry in folder_entries(&document)? {
            if let Ok(root) = resolve_folder(entry, &session.base).and_then(|p| self.authorize(&p)) {
                if root.is_dir() && !roots.contains(&root) { roots.push(root); }
            }
        }
        Ok(roots)
    }

    fn git_repository(&self, requested: &str, revision: u64) -> Result<PathBuf> {
        let roots = self.git_roots(revision, true)?;
        let root = self.authorize(Path::new(requested))?;
        if !roots.iter().any(|allowed| root.starts_with(allowed)) {
            return Err(error("outside_workspace", "请将完整仓库根目录加入当前工作区"));
        }
        if !root.join(".git").exists() { return Err(error("invalid_path", "此目录不是已初始化的 Git 仓库根目录")); }
        let top = PathBuf::from(text(&checked(&root, &["rev-parse", "--show-toplevel"])?)?.trim());
        if top.canonicalize()? != root { return Err(error("invalid_path", "Git 仓库根目录不匹配")); }
        Ok(root)
    }

    fn git_path(&self, root: &Path, relative: &str) -> Result<PathBuf> {
        let path = Path::new(relative);
        if relative.is_empty() || relative.len() > 32768 || path.is_absolute() || path.components().count() > 512
            || path.components().any(|c| !matches!(c, Component::Normal(_)))
            || path.components().any(|c| matches!(c, Component::Normal(name) if excluded_directory(&name.to_string_lossy()))) {
            return Err(error("invalid_path", "无效或已排除的 Git 文件路径"));
        }
        let candidate = root.join(path);
        let mut ancestor = candidate.as_path();
        while !ancestor.exists() {
            if fs::symlink_metadata(ancestor).is_ok() { return Err(error("invalid_path", "Git 文件不能通过符号链接访问")); }
            ancestor = ancestor.parent().ok_or_else(|| error("invalid_path", "无效路径"))?;
        }
        let authorized = self.authorize(ancestor)?;
        if !authorized.starts_with(root) || fs::symlink_metadata(&candidate).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(error("outside_workspace", "Git 文件路径越界或为符号链接"));
        }
        // Check intermediate links too, including Windows junction escapes.
        for parent in candidate.ancestors().take_while(|p| *p != root) {
            if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) { return Err(error("invalid_path", "Git 文件不能通过符号链接访问")); }
        }
        Ok(candidate)
    }

    pub fn git_status(&self, request: GitStatusRequest, authenticated: bool) -> Result<GitStatus> {
        let roots = self.git_roots(request.expected_revision, authenticated)?;
        let _operations = self.git_operations.lock().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        command::executable()?;
        let mut status = GitStatus { revision: request.expected_revision, repositories: Vec::new(), warnings: Vec::new() };
        let mut queue: std::collections::VecDeque<(PathBuf, Option<String>, Option<String>, usize)> = roots.into_iter().map(|root| (root, None, None, 0)).collect();
        let mut seen = HashSet::new();
        let started = Instant::now();
        let mut visited = 0;
        while let Some((directory, parent, submodule_path, depth)) = queue.pop_front() {
            if status.repositories.len() >= 64 || visited >= 10000 || started.elapsed() > Duration::from_secs(20) {
                status.warnings.push("仓库发现已达到预算（64 仓库、10000 目录项或 20 秒），请缩小工作区范围。".into()); break;
            }
            let Ok(directory) = self.authorize(&directory) else { continue; };
            if !seen.insert(directory.clone()) { continue; }
            if directory.join(".git").exists() {
                match self.git_repository(&display_path(&directory), request.expected_revision).and_then(|root| repository_status(self, &root)) {
                    Ok(mut repository) => {
                        repository.parent = parent; repository.submodule_path = submodule_path;
                        match gitlinks(&directory) {
                            Ok(links) => for link in links {
                                if let Ok(child) = self.git_path(&directory, &link) {
                                    if child.is_dir() && child.join(".git").exists() { queue.push_back((child, Some(repository.root.clone()), Some(link), 0)); }
                                }
                            },
                            Err(err) => status.warnings.push(format!("{}：{}", repository.name, err.message)),
                        }
                        status.repositories.push(repository);
                    }
                    Err(err) => status.warnings.push(format!("{}：{}", display_path(&directory), err.message)),
                }
                continue; // Never walk repository contents to discover children: use index gitlinks.
            }
            if depth >= 4 { continue; }
            let Ok(entries) = fs::read_dir(&directory) else { continue; };
            for entry in entries {
                visited += 1; if visited >= 10000 { break; }
                let Ok(entry) = entry else { continue; };
                if !entry.file_type().is_ok_and(|kind| kind.is_dir() && !kind.is_symlink()) { continue; }
                if excluded_directory(&entry.file_name().to_string_lossy()) { continue; }
                queue.push_back((entry.path(), None, None, depth + 1));
            }
        }
        // A submodule may also be an explicit workspace root and be discovered first.
        let parents: Vec<_> = status.repositories.iter().map(|repo| repo.root.clone()).collect();
        for parent in parents {
            if let Ok(links) = gitlinks(Path::new(&parent)) {
                for link in links {
                    let candidate = Path::new(&parent).join(&link).canonicalize().ok();
                    if let Some(child) = status.repositories.iter_mut().find(|repo| candidate.as_ref().is_some_and(|p| comparison_path(p) == comparison_path(Path::new(&repo.root)))) {
                        child.parent = Some(parent.clone()); child.submodule_path = Some(link);
                    }
                }
            }
        }
        status.repositories.sort_by(|a, b| a.root.cmp(&b.root));
        self.git_roots(request.expected_revision, true)?;
        Ok(status)
    }

    pub fn git_diff(&self, request: GitDiffRequest, authenticated: bool) -> Result<GitDiff> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _operations = self.git_operations.lock().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let repo = repository_status(self, &root)?;
        if repo.version != request.version { return Err(error("conflict", "Git 状态已变化，请刷新后查看差异")); }
        let change = repo.changes.iter().find(|change| change.path == request.path).ok_or_else(|| error("not_found", "文件不在当前变更列表中"))?;
        let diff = diff_for(self, &root, &repo, change)?;
        self.git_roots(request.expected_revision, true)?;
        Ok(diff)
    }
}

fn gitlinks(root: &Path) -> Result<Vec<String>> {
    let bytes = checked(root, &["ls-files", "--stage", "-z"])?;
    let mut links = Vec::new();
    for record in bytes.split(|b| *b == 0).filter(|r| r.starts_with(b"160000 ")) {
        let record = text(record)?;
        if let Some((metadata, path)) = record.split_once('\t') {
            if metadata.ends_with(" 0") && !links.iter().any(|link| link == path) { links.push(path.to_owned()); }
        }
    }
    Ok(links)
}

fn repository_status(service: &WorkspaceService, root: &Path) -> Result<GitRepository> {
    // Enumerate untracked files separately: ls-files' exclude patterns prune
    // dependency/build directories before descending, rather than filtering a
    // fully enumerated `status --untracked-files=all` result afterwards.
    let mut args = vec!["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=no", "--ignore-submodules=none", "--", "."];
    for pattern in [":(exclude,icase,glob)**/node_modules/**", ":(exclude,icase,glob)**/target/**", ":(exclude,icase,glob)**/build/**", ":(exclude,icase,glob)**/dist/**"] { args.push(pattern); }
    let mut bytes = checked(root, &args)?;
    let untracked = checked(root, &["ls-files", "--others", "--exclude-standard", "--exclude=node_modules/", "--exclude=target/", "--exclude=build/", "--exclude=dist/", "-z"])?;
    for path in untracked.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
        bytes.extend_from_slice(b"? "); bytes.extend_from_slice(path); bytes.push(0);
    }
    let (head, branch, mut changes) = parse_status(&bytes)?;
    let mut truncated = changes.len() > 2000;
    changes.truncate(2000);
    changes.retain(|change| {
        let valid = service.git_path(root, &change.path).is_ok() && change.original_path.as_ref().is_none_or(|old| service.git_path(root, old).is_ok());
        if !valid { truncated = true; } valid
    });
    for change in &mut changes {
        if !change.submodule && root.join(&change.path).is_dir() {
            change.selectable = false; change.reason = Some("嵌套仓库或目录，请将其作为独立仓库操作".into());
        }
    }
    let git_dir = text(&checked(root, &["rev-parse", "--absolute-git-dir"])?)?;
    let metadata = PathBuf::from(git_dir.trim());
    let blocked = ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply"].iter().find(|name| metadata.join(name).exists())
        .map(|_| "仓库处于合并、变基或拣选过程中，请先在 Git/IDE 中完成该操作".into());
    let mut version_bytes = bytes;
    for change in &changes {
        for path in change.original_path.iter().chain(std::iter::once(&change.path)) {
            if let Ok(metadata) = fs::symlink_metadata(root.join(path)) {
                version_bytes.extend_from_slice(format!("{path}\0{}:{:?}\0", metadata.len(), metadata.modified().ok()).as_bytes());
            }
        }
    }
    Ok(GitRepository { root: display_path(root), name: root.file_name().unwrap_or_default().to_string_lossy().into_owned(), branch, head,
        parent: None, submodule_path: None, version: fingerprint(&version_bytes), changes, blocked, truncated })
}

fn parse_status(bytes: &[u8]) -> Result<(Option<String>, String, Vec<GitChange>)> {
    let mut head = None; let mut branch = String::new(); let mut changes = Vec::new();
    let mut records = bytes.split(|b| *b == 0).filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        let line = text(record)?;
        if let Some(oid) = line.strip_prefix("# branch.oid ") { if oid != "(initial)" { head = Some(oid.to_owned()); } continue; }
        if let Some(name) = line.strip_prefix("# branch.head ") { branch = name.to_owned(); continue; }
        if line.starts_with('#') || line.starts_with('!') { continue; }
        if let Some(path) = line.strip_prefix("? ") {
            changes.push(GitChange { path: path.trim_end_matches('/').into(), original_path: None, status: "??".into(), kind: "untracked".into(), untracked: true,
                submodule: false, pointer_changed: false, submodule_dirty: false, selectable: true, reason: None, head_oid: String::new(), index_oid: String::new(), head_mode: "000000".into() }); continue;
        }
        let tag = line.as_bytes().first().copied();
        let count = match tag { Some(b'1') => 9, Some(b'2') => 10, Some(b'u') => 11, _ => continue };
        let fields: Vec<_> = line.splitn(count, ' ').collect();
        if fields.len() != count { return Err(error("git_failed", "无法解析 Git porcelain v2 状态")); }
        let original_path = if tag == Some(b'2') { Some(text(records.next().ok_or_else(|| error("git_failed", "Git 重命名缺少原始路径"))?)?) } else { None };
        let status = fields[1]; let sub = fields[2]; let submodule = sub.starts_with('S');
        let pointer_changed = submodule && (sub.as_bytes().get(1) == Some(&b'C') || status.as_bytes().first().is_some_and(|x| *x != b'.'));
        let dirty = submodule && sub.as_bytes().iter().skip(2).any(|b| *b != b'.');
        let conflicted = tag == Some(b'u');
        let kind = if conflicted { "conflict" } else if original_path.is_some() { "renamed" } else if status.contains('D') { "deleted" } else if fields[3] == "000000" { "added" } else { "modified" };
        changes.push(GitChange { path: fields[count - 1].into(), original_path, status: status.into(), kind: kind.into(), untracked: false, submodule, pointer_changed,
            submodule_dirty: dirty, selectable: !conflicted && (!submodule || pointer_changed),
            reason: if conflicted { Some("冲突文件需要先解决冲突".into()) } else if submodule && !pointer_changed { Some("子仓库只有未提交内容，请在下方子仓库中提交".into()) } else { None },
            head_oid: if conflicted { fields[7] } else { fields[6] }.into(), index_oid: if conflicted { fields[8] } else { fields[7] }.into(), head_mode: fields[3].into() });
    }
    Ok((head, branch, changes))
}

fn working_bytes(service: &WorkspaceService, root: &Path, change: &GitChange) -> Result<Option<Vec<u8>>> {
    let path = service.git_path(root, &change.path)?;
    if change.submodule {
        if !path.join(".git").exists() { return Err(error("invalid_path", "子模块未初始化，不可提交指针")); }
        return checked(&path, &["rev-parse", "--verify", "HEAD"]).map(Some);
    }
    match fs::File::open(&path) {
        Ok(file) => {
            if !file.metadata()?.is_file() || file.metadata()?.len() > 4 * 1024 * 1024 { return Err(error("limit_exceeded", "Git 操作仅支持 4 MiB 以内的普通文件")); }
            let mut bytes = Vec::new(); file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 4 * 1024 * 1024 { return Err(error("limit_exceeded", "文件超过 4 MiB")); } Ok(Some(bytes))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn baseline(root: &Path, repo: &GitRepository, change: &GitChange) -> Result<Vec<u8>> {
    if repo.head.is_none() || change.head_mode == "000000" || change.untracked { return Ok(Vec::new()); }
    let source = format!("{}:{}", repo.head.as_deref().unwrap_or("HEAD"), change.original_path.as_deref().unwrap_or(&change.path));
    let size = text(&checked(root, &["cat-file", "-s", &source])?)?.trim().parse::<u64>().map_err(|_| error("git_failed", "Git 文件大小无效"))?;
    if size > 1024 * 1024 { return Err(error("limit_exceeded", "Diff 原文超过 1 MiB")); }
    checked(root, &["cat-file", "blob", &source])
}
fn diff_for(service: &WorkspaceService, root: &Path, repo: &GitRepository, change: &GitChange) -> Result<GitDiff> {
    let mut diff = GitDiff { repository: repo.root.clone(), path: change.path.clone(), original_path: change.original_path.clone(), before: String::new(), after: String::new(), message: None, submodule: change.submodule };
    if change.submodule {
        diff.before = format!("Subproject commit {}\n", change.head_oid);
        let current = working_bytes(service, root, change)?;
        diff.after = format!("Subproject commit {}\n", current.as_deref().map(text).transpose()?.unwrap_or_default().trim());
        if change.submodule_dirty { diff.message = Some("这里只比较父仓库记录的指针；子仓库未提交内容在独立仓库分组查看。".into()); }
        return Ok(diff);
    }
    let before = match baseline(root, repo, change) { Ok(bytes) => bytes, Err(err) if err.code == "limit_exceeded" => { diff.message = Some(err.message); return Ok(diff); }, Err(err) => return Err(err) };
    let after = match working_bytes(service, root, change) { Ok(bytes) => bytes.unwrap_or_default(), Err(err) if err.code == "limit_exceeded" => { diff.message = Some(err.message); return Ok(diff); }, Err(err) => return Err(err) };
    if before.len() > 1024 * 1024 || after.len() > 1024 * 1024 || before.contains(&0) || after.contains(&0) {
        diff.message = Some("二进制或超过 1 MiB 的文件，仅展示状态，不展开文本差异。".into()); return Ok(diff);
    }
    match (String::from_utf8(before), String::from_utf8(after)) {
        (Ok(before), Ok(after)) => { diff.before = before.replace("\r\n", "\n"); diff.after = after.replace("\r\n", "\n"); }
        _ => diff.message = Some("文件不是 UTF-8 文本，仅展示状态。".into()),
    }
    Ok(diff)
}
