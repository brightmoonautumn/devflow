//! On-demand attribution for one bounded editor snapshot; no background index/cache.
use super::*;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitBlameRequest {
    pub expected_revision: u64, pub path: String, pub content: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameCommit { pub id: String, pub author: String, pub timestamp: i64, pub timezone: String }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameRange { pub from: usize, pub to: usize, pub commit: Option<String> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBlame { pub content_version: String, pub repository: String, pub commits: Vec<BlameCommit>, pub ranges: Vec<BlameRange> }

fn parse_blame(bytes: &[u8], expected_lines: usize) -> Result<(Vec<BlameCommit>, Vec<BlameRange>)> {
    let output = text(bytes)?;
    let mut commits: HashMap<String, BlameCommit> = HashMap::new();
    let mut ranges: Vec<BlameRange> = Vec::new();
    let mut lines = output.lines();
    let invalid = || error("git_failed", "Git 行批注格式无效");
    let mut next = 1;
    while let Some(header) = lines.next() {
        let fields: Vec<_> = header.split_whitespace().collect();
        if !(3..=4).contains(&fields.len()) { return Err(invalid()); }
        let id = fields[0];
        if !matches!(id.len(), 40 | 64) || !id.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(invalid()); }
        let line = fields[2].parse::<usize>().map_err(|_| invalid())?;
        if line != next || line > expected_lines { return Err(invalid()); }
        let uncommitted = id.bytes().all(|b| b == b'0');
        let mut author = None; let mut timestamp = None; let mut timezone = None; let mut source = false;
        for metadata in lines.by_ref() {
            if metadata.starts_with('\t') { source = true; break; }
            if let Some(value) = metadata.strip_prefix("author ") { author = Some(value.to_owned()); }
            if let Some(value) = metadata.strip_prefix("author-time ") { timestamp = Some(value.parse::<i64>().map_err(|_| invalid())?); }
            if let Some(value) = metadata.strip_prefix("author-tz ") { timezone = Some(value.to_owned()); }
        }
        if !source { return Err(invalid()); }
        if !uncommitted && !commits.contains_key(id) {
            let timezone = timezone.ok_or_else(invalid)?;
            if timezone.len() != 5 || !matches!(timezone.as_bytes()[0], b'+' | b'-') || !timezone.as_bytes()[1..].iter().all(u8::is_ascii_digit) { return Err(invalid()); }
            commits.insert(id.into(), BlameCommit { id: id.into(), author: author.ok_or_else(invalid)?, timestamp: timestamp.ok_or_else(invalid)?, timezone });
        }
        let commit = (!uncommitted).then(|| id.to_owned());
        if let Some(last) = ranges.last_mut().filter(|last| last.commit == commit) { last.to = line; }
        else { ranges.push(BlameRange { from: line, to: line, commit }); }
        next += 1;
    }
    if next != expected_lines + 1 { return Err(invalid()); }
    let mut commits: Vec<_> = commits.into_values().collect(); commits.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((commits, ranges))
}

impl WorkspaceService {
    pub fn git_blame(&self, request: GitBlameRequest, authenticated: bool) -> Result<GitBlame> {
        self.git_roots(request.expected_revision, authenticated)?;
        let snapshot = self.read_file(WorkspaceFileReadRequest { expected_revision: request.expected_revision, path: request.path.clone() }, true)?;
        let content = request.content.unwrap_or_else(|| snapshot.content.clone()).trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
        let count = content.lines().count();
        if content.len() > 1024 * 1024 || count > 10000 || content.contains('\0') {
            return Err(error("limit_exceeded", "行批注仅支持 1 MiB、10000 行以内的 UTF-8 文本"));
        }
        let content_version = fingerprint(content.as_bytes());
        let _operations = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
        let path = self.authorize(Path::new(&snapshot.path))?;
        // Ask Git for the nearest repository (including submodules/worktrees),
        // then apply the same complete-root authorization as History/Diff.
        let top = text(&checked(path.parent().ok_or_else(|| error("invalid_path", "文件没有父目录"))?, &["rev-parse", "--show-toplevel"])?)?;
        let root = self.git_repository(top.trim(), request.expected_revision)?;
        let relative = path.strip_prefix(&root).map_err(|_| error("outside_workspace", "文件不属于此仓库"))?.to_string_lossy().replace('\\', "/");
        self.git_path(&root, &relative)?;
        let head = checked(&root, &["rev-parse", "--verify", "HEAD"])?;
        // Only tracked text is eligible. No shell, network, hooks or checkout.
        checked(&root, &["ls-files", "--error-unmatch", "--", &literal(&relative)])?;
        let (commits, ranges) = if count == 0 { (Vec::new(), Vec::new()) } else {
            let mut input = match snapshot.line_ending {
                "CRLF" => content.replace('\n', "\r\n"),
                "CR" => content.replace('\n', "\r"),
                _ => content,
            };
            if snapshot.bom { input.insert(0, '\u{feff}'); }
            let output = run(&root, &["-c", "blame.ignoreRevsFile=", "blame", "--root", "--porcelain", "--encoding=UTF-8", "--contents", "-", "--", &relative], Some(input.into_bytes()), false)?.checked()?;
            parse_blame(&output, count)?
        };
        let current = self.read_file(WorkspaceFileReadRequest { expected_revision: request.expected_revision, path: request.path }, true)?;
        if current.content != snapshot.content || checked(&root, &["rev-parse", "--verify", "HEAD"])? != head {
            return Err(error("conflict", "查询期间文件或 HEAD 已改变，请重新开启行批注"));
        }
        Ok(GitBlame { content_version, repository: display_path(&root), commits, ranges })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_groups_reused_commits_unicode_and_uncommitted_lines() {
        let id = "a".repeat(40); let zero = "0".repeat(40);
        let output = format!("{id} 1 1 2\nauthor 中文 Fly\nauthor-time 1792108800\nauthor-tz +0800\nfilename 空格 文件.java\n\t第一行\n{id} 2 2\n\t第二行\n{zero} 3 3 1\nauthor Not Committed Yet\n\t草稿\n{id} 4 4 1\n\t第四行\n");
        let (commits, ranges) = parse_blame(output.as_bytes(), 4).unwrap();
        assert_eq!(commits.len(), 1); assert_eq!(commits[0].author, "中文 Fly");
        assert_eq!(commits[0].timezone, "+0800"); assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].to, 2); assert!(ranges[1].commit.is_none());
        assert!(parse_blame(output.as_bytes(), 5).is_err());
        assert!(parse_blame(b"invalid", 1).is_err());
        assert!(parse_blame(format!("{id} 1 2\n\tx\n").as_bytes(), 1).is_err());
    }
}
