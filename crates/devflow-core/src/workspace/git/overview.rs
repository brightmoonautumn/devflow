//! A bounded k-way merge of independent repository logs. Cursor positions are per root.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitHistoryOverviewRequest {
    pub expected_revision: u64,
    #[serde(default)] pub repositories: Vec<String>,
    pub reference: Option<String>, pub query: Option<String>, pub limit: Option<usize>, pub cursor: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Position { repository: String, version: String, skip: usize }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor { scope: String, positions: Vec<Position> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewCommit { pub repository: String, #[serde(flatten)] pub commit: HistoryCommit }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryOverview { pub commits: Vec<OverviewCommit>, pub next_cursor: Option<String>, pub references: Vec<String>, pub warnings: Vec<String> }

// At most four scoped readers, no detached work or recursive project scans.
pub(super) fn parallel_roots<T: Send>(roots: &[PathBuf], task: impl Fn(&Path) -> Result<T> + Sync) -> Result<Vec<T>> {
    let mut values = Vec::new(); let started = Instant::now();
    for chunk in roots.chunks(4) {
        if started.elapsed() > Duration::from_secs(90) { return Err(error("git_timeout", "仓库读取达到时间预算，请减少所选仓库后重试")); }
        let batch = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk.iter().map(|root| { let task = &task; scope.spawn(move || task(root)) }).collect();
            handles.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Err(error("unavailable", "Git 历史读取任务失败")))).collect::<Vec<_>>()
        });
        for value in batch { values.push(value?); }
    }
    Ok(values)
}

impl WorkspaceService {
    pub(super) fn git_selected_roots(&self, revision: u64, requested: &[String]) -> Result<(Vec<PathBuf>, Vec<String>)> {
        if requested.len() > 64 { return Err(error("limit_exceeded", "最多选择 64 个仓库")); }
        let (catalog, warnings) = self.git_catalog(revision, false)?;
        let mut roots = Vec::new();
        if requested.is_empty() { roots.extend(catalog.iter().map(|repo| repo.root.clone())); }
        else {
            for name in requested {
                let root = self.git_repository(name, revision)?;
                if !catalog.iter().any(|repo| repo.root == root) { return Err(error("outside_workspace", "仓库不在当前工作区发现范围内")); }
                if roots.contains(&root) { return Err(error("invalid_query", "仓库筛选不能包含重复项")); }
                roots.push(root);
            }
        }
        roots.sort(); Ok((roots, warnings))
    }
    pub fn git_history_overview(&self, request: GitHistoryOverviewRequest, authenticated: bool) -> Result<GitHistoryOverview> {
        self.git_roots(request.expected_revision, authenticated)?;
        let limit = request.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) || request.cursor.as_ref().is_some_and(|value| value.len() > 32768) {
            return Err(error("limit_exceeded", "每页 1–100 条，分页游标不能超过 32 KiB"));
        }
        if request.query.as_ref().is_some_and(|value| value.len() > 512 || value.contains('\0')) { return Err(error("invalid_query", "搜索文本最多 512 字节且不能包含 NUL")); }
        let (roots, mut warnings) = self.git_selected_roots(request.expected_revision, &request.repositories)?;
        let reference = request.reference.as_deref().unwrap_or("all");
        if roots.len() != 1 && !matches!(reference, "all" | "HEAD") { return Err(error("invalid_query", "具体分支/标签筛选仅支持单仓库，请先聚焦仓库")); }
        let scope = fingerprint(&serde_json::to_vec(&(request.expected_revision, &roots, reference, &request.query)).map_err(|_| error("invalid_query", "无效历史筛选"))?);
        let cursor: Option<Cursor> = request.cursor.as_ref().map(|value| serde_json::from_str::<Cursor>(value).map_err(|_| error("invalid_query", "无效历史游标"))).transpose()?;
        if let Some(cursor) = &cursor {
            if cursor.scope != scope || cursor.positions.len() != roots.len() || cursor.positions.iter().zip(&roots).any(|(position, root)| position.repository != display_path(root) || position.skip > 10000 || position.version.is_empty()) {
                return Err(error("conflict", "历史筛选或仓库集合已变化，请刷新后重新翻页"));
            }
        }
        let memory = std::sync::atomic::AtomicUsize::new(0);
        let pages = parallel_roots(&roots, |root| {
            let position = cursor.as_ref().and_then(|cursor| cursor.positions.iter().find(|position| position.repository == display_path(root)));
            let page = self.git_history(GitHistoryRequest { expected_revision: request.expected_revision, repository: display_path(root),
                skip: position.map_or(0, |position| position.skip), limit: Some(limit), reference: Some(reference.into()), query: request.query.clone(), version: position.map(|position| position.version.clone()) }, true)?;
            let bytes = serde_json::to_vec(&page).map_err(|_| error("unavailable", "无法读取历史记录"))?.len();
            if memory.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed) + bytes > 32 * 1024 * 1024 { return Err(error("limit_exceeded", "聚合历史超过 32 MiB，请减少仓库或缩小搜索范围")); }
            Ok(page)
        })?;
        let mut positions: Vec<_> = pages.iter().map(|page| Position { repository: page.repository.clone(), version: page.version.clone(), skip: cursor.as_ref().and_then(|cursor| cursor.positions.iter().find(|position| position.repository == page.repository)).map_or(0, |position| position.skip) }).collect();
        let mut consumed = vec![0usize; pages.len()]; let mut commits = Vec::new();
        for _ in 0..limit {
            // Only compare each root's next row, preserving its topological order.
            let next = pages.iter().enumerate().filter_map(|(index, page)| page.commits.get(consumed[index]).map(|commit| (index, commit.committed_timestamp)))
                .max_by(|(left, time), (right, other)| time.cmp(other).then_with(|| right.cmp(left)));
            let Some((index, _)) = next else { break; };
            commits.push(OverviewCommit { repository: pages[index].repository.clone(), commit: pages[index].commits[consumed[index]].clone() });
            consumed[index] += 1; positions[index].skip += 1;
        }
        let more = pages.iter().enumerate().any(|(index, page)| positions[index].skip <= 10000 && (consumed[index] < page.commits.len() || page.next_skip.is_some()));
        if positions.iter().any(|position| position.skip > 10000) { warnings.push("部分仓库达到 10000 条翻页上限，请使用搜索缩小范围。".into()); }
        // Recheck every root, including roots that contributed no rows this page.
        parallel_roots(&roots, |root| {
            self.git_roots(request.expected_revision, true)?;
            let _gate = self.git_operations.read().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
            let expected = positions.iter().find(|position| position.repository == display_path(root)).unwrap();
            if self.git_repository(&expected.repository, request.expected_revision)?.as_path() != root { return Err(error("conflict", "仓库路径已变化")); }
            if history::history_refs(root)?.0 != expected.version { return Err(error("conflict", "仓库引用已变化，请刷新聚合历史")); }
            Ok(())
        })?;
        self.git_roots(request.expected_revision, true)?;
        let references = if pages.len() == 1 { pages[0].references.clone() } else { Vec::new() };
        let next_cursor = if more && positions.iter().all(|position| position.skip <= 10000) {
            let value = serde_json::to_string(&Cursor { scope, positions }).map_err(|_| error("unavailable", "无法生成历史游标"))?;
            if value.len() > 32768 { return Err(error("limit_exceeded", "仓库路径过长，分页游标超过 32 KiB，请减少所选仓库")); }
            Some(value)
        } else { None };
        Ok(GitHistoryOverview { commits, next_cursor, references, warnings })
    }
}
