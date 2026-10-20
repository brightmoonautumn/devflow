//! Bounded, local-only commit history. No checkout, fetch or working-tree writes.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitHistoryRequest {
    pub expected_revision: u64, pub repository: String,
    #[serde(default)] pub skip: usize,
    pub limit: Option<usize>, pub reference: Option<String>, pub query: Option<String>, pub version: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitHistoryCommitRequest {
    pub expected_revision: u64, pub repository: String, pub commit: String, pub parent: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitHistoryDiffRequest {
    pub expected_revision: u64, pub repository: String, pub commit: String, pub parent: Option<String>, pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCommit {
    pub id: String, pub parents: Vec<String>, pub author: String, pub email: String,
    pub timestamp: i64, pub subject: String, pub refs: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistory {
    pub repository: String, pub version: String, pub references: Vec<String>,
    pub commits: Vec<HistoryCommit>, pub next_skip: Option<usize>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFile {
    pub path: String, pub status: String, pub submodule: bool, pub available: bool,
    pub reason: Option<String>,
    #[serde(skip)] old_oid: String, #[serde(skip)] new_oid: String,
    #[serde(skip)] old_mode: String, #[serde(skip)] new_mode: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryCommit {
    pub repository: String, pub commit: HistoryCommit, pub parent: Option<String>,
    pub message: String, pub files: Vec<HistoryFile>, pub truncated: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryDiff {
    #[serde(flatten)] pub diff: GitDiff,
    pub historical: bool, pub commit: String, pub parent: Option<String>,
}

fn validate_oid(value: &str) -> Result<()> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error("invalid_query", "历史记录需要完整的 commit SHA"));
    }
    Ok(())
}
fn parse_commits(bytes: &[u8]) -> Result<Vec<HistoryCommit>> {
    if bytes.is_empty() { return Ok(Vec::new()); }
    let mut fields: Vec<_> = bytes.split(|b| *b == 0).collect();
    if fields.last().is_some_and(|f| f.is_empty()) { fields.pop(); }
    if fields.len() % 7 != 0 { return Err(error("git_failed", "Git 历史记录格式无效")); }
    fields.chunks_exact(7).map(|row| {
        let id = text(row[0])?.to_owned(); validate_oid(&id)?;
        Ok(HistoryCommit { id, parents: text(row[1])?.split_whitespace().map(str::to_owned).collect(),
            author: text(row[2])?.to_owned(), email: text(row[3])?.to_owned(),
            timestamp: text(row[4])?.parse().map_err(|_| error("git_failed", "Git 提交时间无效"))?,
            subject: text(row[5])?.to_owned(), refs: text(row[6])?.split(", ").filter(|s| !s.is_empty()).map(str::to_owned).collect() })
    }).collect()
}
const FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%s%x00%D";

fn history_refs(root: &Path) -> Result<(String, Vec<String>, bool)> {
    // --all also visits stash/custom refs. Fingerprint every ref so those
    // changes cannot silently shift an in-progress pagination window.
    let refs = checked(root, &["for-each-ref", "--format=%(objectname) %(refname)"])?;
    let references: Vec<_> = text(&refs)?.lines().filter_map(|line| line.split_once(' ').map(|(_, name)| name)).filter(|name| name.starts_with("refs/heads/") || name.starts_with("refs/remotes/") || name.starts_with("refs/tags/")).map(str::to_owned).collect();
    if references.len() > 4096 { return Err(error("limit_exceeded", "分支和标签超过 4096 项，请缩小仓库范围")); }
    let head = run(root, &["rev-parse", "--verify", "HEAD"], None, false)?;
    let has_head = head.code == 0;
    let mut bytes = refs; if has_head { bytes.extend_from_slice(&head.stdout); }
    let symbolic = run(root, &["symbolic-ref", "--quiet", "HEAD"], None, false)?;
    if symbolic.code == 0 { bytes.extend_from_slice(&symbolic.stdout); }
    Ok((fingerprint(&bytes), references, has_head))
}
fn commit_metadata(service: &WorkspaceService, root: &Path, oid: &str) -> Result<(HistoryCommit, String)> {
    validate_oid(oid)?;
    let key = format!("metadata:{root:?}:{oid}:{}", service.git_cache.content_epoch(root));
    if let Some(value) = service.git_cache.content_get::<(HistoryCommit, String)>(&key) { return Ok(value); }
    if text(&checked(root, &["cat-file", "-t", oid])?)?.trim() != "commit" { return Err(error("invalid_query", "所选对象不是提交")); }
    let bytes = checked(root, &["show", "-s", "-z", "--no-show-signature", FORMAT, oid, "--"])?;
    let commit = parse_commits(&bytes)?.into_iter().next().ok_or_else(|| error("not_found", "提交不存在"))?;
    let mut message = text(&checked(root, &["show", "-s", "--no-show-signature", "--format=%B", oid, "--"])?)?.to_owned();
    if message.len() > 256 * 1024 {
        let end = message.char_indices().take_while(|(i, _)| *i <= 256 * 1024).last().map_or(0, |(i, _)| i);
        message.truncate(end); message.push_str("\n…提交说明超过 256 KiB，已截断。\n");
    }
    service.git_cache.content_put(&key, &(commit.clone(), message.clone()));
    Ok((commit, message))
}
fn choose_parent(commit: &HistoryCommit, requested: Option<&str>) -> Result<Option<String>> {
    if let Some(parent) = requested {
        validate_oid(parent)?;
        if !commit.parents.iter().any(|oid| oid == parent) { return Err(error("invalid_query", "只能比较该提交的直接父提交")); }
        Ok(Some(parent.to_owned()))
    } else { Ok(commit.parents.first().cloned()) }
}
fn parse_files(bytes: &[u8]) -> Result<(Vec<HistoryFile>, bool)> {
    let mut records = bytes.split(|b| *b == 0).filter(|r| !r.is_empty());
    let mut files = Vec::new();
    while let Some(meta) = records.next() {
        let metadata = text(meta)?;
        let meta: Vec<_> = metadata.split_whitespace().collect();
        let path = text(records.next().ok_or_else(|| error("git_failed", "历史文件路径缺失"))?)?.to_owned();
        if meta.len() != 5 || !meta[0].starts_with(':') { return Err(error("git_failed", "历史文件状态格式无效")); }
        if files.len() >= 2000 { return Ok((files, true)); }
        let old_mode = meta[0].trim_start_matches(':').to_owned(); let new_mode = meta[1].to_owned();
        let available = [&old_mode, &new_mode].iter().all(|mode| matches!(mode.as_str(), "000000" | "100644" | "100755" | "160000"));
        files.push(HistoryFile { path, status: meta[4].into(), submodule: old_mode == "160000" || new_mode == "160000", available,
            reason: (!available).then(|| "符号链接或特殊对象不展开内容".into()),
            old_oid: meta[2].into(), new_oid: meta[3].into(), old_mode, new_mode });
    }
    Ok((files, false))
}
fn commit_files(service: &WorkspaceService, root: &Path, commit: &str, parent: Option<&str>) -> Result<(Vec<HistoryFile>, bool)> {
    let mut args = vec!["diff-tree", "--no-commit-id", "--raw", "--no-abbrev", "-z", "-r", "--no-renames", "--no-ext-diff"];
    if let Some(parent) = parent { args.push(parent); } else { args.push("--root"); }
    args.extend([commit, "--"]);
    let key = format!("files:{root:?}:{commit}:{parent:?}");
    let bytes = if let Some(bytes) = service.git_cache.raw_get(&key) { bytes } else {
        let bytes = checked(root, &args)?; service.git_cache.raw_put(&key, bytes.clone()); bytes
    };
    let (mut files, truncated) = parse_files(&bytes)?;
    for file in &mut files {
        if let Err(err) = service.git_path(root, &file.path) { file.available = false; file.reason = Some(err.message); }
    }
    Ok((files, truncated))
}
fn object_text(service: &WorkspaceService, root: &Path, oid: &str, mode: &str) -> Result<String> {
    if mode == "000000" { return Ok(String::new()); }
    validate_oid(oid)?;
    if mode == "160000" { return Ok(format!("Subproject commit {oid}\n")); }
    let key = format!("blob:{root:?}:{oid}:{mode}");
    if let Some(value) = service.git_cache.content_get::<String>(&key) { return Ok(value); }
    let size = text(&checked(root, &["cat-file", "-s", oid])?)?.trim().parse::<u64>().map_err(|_| error("git_failed", "历史文件大小无效"))?;
    if size > 1024 * 1024 { return Err(error("limit_exceeded", "历史文件超过 1 MiB，仅展示状态")); }
    let bytes = checked(root, &["cat-file", "blob", oid])?;
    if bytes.contains(&0) { return Err(error("unsupported_file", "二进制文件仅展示状态")); }
    let value = String::from_utf8(bytes).map(|s| s.replace("\r\n", "\n")).map_err(|_| error("unsupported_file", "非 UTF-8 文件仅展示状态"))?;
    service.git_cache.content_put(&key, &value);
    Ok(value)
}

impl WorkspaceService {
    pub fn git_history(&self, request: GitHistoryRequest, authenticated: bool) -> Result<GitHistory> {
        self.git_roots(request.expected_revision, authenticated)?;
        let limit = request.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) || request.skip > 10000 || request.query.as_ref().is_some_and(|q| q.len() > 512 || q.contains('\0')) { return Err(error("invalid_query", "每页 1–100 条，跳过条数最多 10000，搜索文本最多 512 字节")); }
        if request.skip > 0 && request.version.is_none() { return Err(error("invalid_query", "继续翻页需要上一页的 version")); }
        let _operations = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let (version, references, has_head) = history_refs(&root)?;
        if request.version.as_ref().is_some_and(|v| v != &version) { return Err(error("conflict", "分支引用已变化，请刷新历史后继续翻页")); }
        let reference = request.reference.as_deref().unwrap_or("all");
        if reference != "all" && reference != "HEAD" && !references.iter().any(|r| r == reference) { return Err(error("invalid_query", "请选择仓库已有分支或标签")); }
        let count = format!("--max-count={}", limit + 1); let skip = format!("--skip={}", request.skip);
        let grep = format!("--grep={}", request.query.as_deref().unwrap_or(""));
        let mut args = vec!["log", "-z", "--topo-order", "--no-show-signature", FORMAT, &count, &skip, "--fixed-strings", "--regexp-ignore-case", &grep];
        if reference == "all" { args.push("--all"); if has_head { args.push("HEAD"); } } else { args.push(reference); }
        args.push("--");
        let mut commits = if !has_head && (reference == "HEAD" || references.is_empty()) { Vec::new() } else { parse_commits(&checked(&root, &args)?)? };
        let next_skip = (commits.len() > limit && request.skip + limit <= 10000).then_some(request.skip + limit);
        commits.truncate(limit);
        if history_refs(&root)?.0 != version { return Err(error("conflict", "读取期间分支引用已变化，请刷新历史")); }
        self.git_roots(request.expected_revision, true)?;
        Ok(GitHistory { repository: display_path(&root), version, references, commits, next_skip })
    }
    pub fn git_history_commit(&self, request: GitHistoryCommitRequest, authenticated: bool) -> Result<GitHistoryCommit> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _operations = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let (commit, message) = commit_metadata(self, &root, &request.commit)?;
        let parent = choose_parent(&commit, request.parent.as_deref())?;
        let (files, truncated) = commit_files(self, &root, &commit.id, parent.as_deref())?;
        self.git_roots(request.expected_revision, true)?;
        Ok(GitHistoryCommit { repository: display_path(&root), commit, parent, message, files, truncated })
    }
    pub fn git_history_diff(&self, request: GitHistoryDiffRequest, authenticated: bool) -> Result<GitHistoryDiff> {
        self.git_roots(request.expected_revision, authenticated)?;
        let _operations = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let root = self.git_repository(&request.repository, request.expected_revision)?;
        let (commit, _) = commit_metadata(self, &root, &request.commit)?;
        let parent = choose_parent(&commit, request.parent.as_deref())?;
        let (files, _) = commit_files(self, &root, &commit.id, parent.as_deref())?;
        let file = files.iter().find(|f| f.path == request.path).ok_or_else(|| error("not_found", "文件不在该提交的变更列表中"))?;
        if !file.available { return Err(error("invalid_path", file.reason.clone().unwrap_or_default())); }
        let mut diff = GitDiff { repository: display_path(&root), path: file.path.clone(), original_path: None, before: String::new(), after: String::new(), message: None, submodule: file.submodule };
        match (object_text(self, &root, &file.old_oid, &file.old_mode), object_text(self, &root, &file.new_oid, &file.new_mode)) {
            (Ok(before), Ok(after)) => { diff.before = before; diff.after = after; }
            (Err(err), _) | (_, Err(err)) if matches!(err.code, "limit_exceeded" | "unsupported_file") => diff.message = Some(err.message),
            (Err(err), _) | (_, Err(err)) => return Err(err),
        }
        self.git_roots(request.expected_revision, true)?;
        Ok(GitHistoryDiff { diff, historical: true, commit: commit.id, parent })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_parsing_preserves_unicode_and_separates_nul_records() {
        let id = "a".repeat(40);
        let bytes = format!("{id}\0\0中文作者\0a@example.test\01760800000\0更新 权限\0HEAD -> main, tag: v1\0");
        let commits = parse_commits(bytes.as_bytes()).unwrap();
        assert_eq!(commits.len(), 1); assert_eq!(commits[0].subject, "更新 权限");
        assert_eq!(commits[0].refs.len(), 2); assert!(commits[0].parents.is_empty());
        assert!(validate_oid("--all").is_err()); assert!(validate_oid("../HEAD").is_err());
        assert!(parse_commits(b"invalid\0").is_err());
    }
    #[test]
    fn history_paths_and_gitlinks_keep_exact_status_and_modes() {
        let id = "b".repeat(40); let zero = "0".repeat(40);
        let bytes = format!(":000000 100644 {zero} {id} A\0目录/a b.txt\0:160000 160000 {id} {id} M\0modules/ai\0");
        let (files, truncated) = parse_files(bytes.as_bytes()).unwrap();
        assert!(!truncated); assert_eq!(files[0].path, "目录/a b.txt"); assert!(files[1].submodule);
        assert!(parse_files(b":invalid\0path\0").is_err());
    }
}
