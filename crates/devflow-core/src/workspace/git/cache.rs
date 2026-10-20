//! Workspace-scoped presentation cache. Never used as a mutation preflight.
use super::*;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, OnceLock};

const CONTENT_LIMIT: usize = 32 * 1024 * 1024;
const STATUS_LIMIT: usize = 16 * 1024 * 1024;
const FALLBACK: Duration = Duration::from_secs(60);
const DEBOUNCE: Duration = Duration::from_millis(350);
type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Work { jobs: VecDeque<Job>, stopped: bool }
struct Pool { state: Arc<(Mutex<Work>, Condvar)> }
impl Pool {
    fn new() -> Self {
        let state = Arc::new((Mutex::new(Work::default()), Condvar::new()));
        let mut workers = 0;
        for index in 0..4 {
            let state = state.clone();
            let started = std::thread::Builder::new().name(format!("git-refresh-{index}")).spawn(move || loop {
                let job = {
                    let (lock, wake) = &*state;
                    let Ok(mut work) = lock.lock() else { return; };
                    while work.jobs.is_empty() && !work.stopped {
                        work = match wake.wait(work) { Ok(work) => work, Err(_) => return };
                    }
                    if work.stopped { return; }
                    work.jobs.pop_front()
                };
                if let Some(job) = job { job(); }
            }).is_ok();
            if started { workers += 1; }
        }
        if workers == 0 { if let Ok(mut work) = state.0.lock() { work.stopped = true; } }
        Self { state }
    }
    fn submit(&self, job: Job) -> bool {
        let (lock, wake) = &*self.state;
        let Ok(mut work) = lock.lock() else { return false; };
        if work.stopped || work.jobs.len() >= 128 { return false; }
        work.jobs.push_back(job); wake.notify_one(); true
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        if let Ok(mut work) = self.state.0.lock() { work.stopped = true; }
        self.state.1.notify_all();
    }
}
#[derive(Default)]
struct Content { entries: VecDeque<(String, Vec<u8>)>, bytes: usize }
impl Content {
    fn insert(&mut self, key: String, bytes: Vec<u8>) {
        if bytes.len() > CONTENT_LIMIT { return; }
        if let Some(index) = self.entries.iter().position(|(old, _)| old == &key) {
            if let Some((_, old)) = self.entries.remove(index) { self.bytes -= old.len(); }
        }
        while self.bytes + bytes.len() > CONTENT_LIMIT || self.entries.len() >= 512 {
            if let Some((_, old)) = self.entries.pop_front() { self.bytes -= old.len(); } else { break; }
        }
        self.bytes += bytes.len(); self.entries.push_back((key, bytes));
    }
    fn remove_scope(&mut self, prefix: &str) {
        self.entries.retain(|(key, _)| !key.starts_with(prefix));
        self.bytes = self.entries.iter().map(|(_, bytes)| bytes.len()).sum();
    }
}
#[derive(Default)]
pub(crate) struct Resources { content: Mutex<Content>, status_bytes: Mutex<usize>, pool: OnceLock<Pool> }
impl Resources {
    fn reserve_status(&self, previous: usize, requested: usize) -> usize {
        let Ok(mut bytes) = self.status_bytes.lock() else { return 0; };
        *bytes = bytes.saturating_sub(previous);
        let granted = requested.min(STATUS_LIMIT.saturating_sub(*bytes)); *bytes += granted; granted
    }
    fn release_status(&self, released: usize) { if let Ok(mut bytes) = self.status_bytes.lock() { *bytes = bytes.saturating_sub(released); } }
}

#[derive(Clone)]
pub(super) struct Metadata { pub git_dir: PathBuf, pub common_dir: PathBuf, pub marker: String }
#[derive(Clone)]
pub(super) struct Repository { pub root: PathBuf, pub name: String, pub parent: Option<String>, pub submodule_path: Option<String>, pub metadata: Metadata }
impl Repository {
    fn empty(&self) -> GitRepository {
        GitRepository { root: display_path(&self.root), name: self.name.clone(), parent: self.parent.clone(), submodule_path: self.submodule_path.clone(),
            branch: String::new(), head: None, version: String::new(), changes: Vec::new(), blocked: Some("正在读取 Git 状态".into()), truncated: false }
    }
}
#[derive(Default)]
struct Entry {
    snapshot: Option<GitRepository>, sampled: Option<Instant>, attempted: Option<Instant>,
    dirty: u64, clean: u64, dirty_at: Option<Instant>, refreshing: bool, error: Option<String>, bytes: usize,
}
#[derive(Default)]
struct State {
    scope: String, generation: u64, topology: Option<(Instant, Vec<Repository>, Vec<String>)>, topology_dirty: bool,
    metadata: HashMap<PathBuf, Metadata>, entries: HashMap<PathBuf, Entry>, watcher_error: Option<String>,
}
pub(crate) struct GitCache {
    id: String, state: Mutex<State>, changed: Condvar, pub topology_gate: Mutex<()>,
    watcher: Mutex<Option<RecommendedWatcher>>, pub resources: Arc<Resources>,
    created: Instant,
}
impl Default for GitCache {
    fn default() -> Self { Self::with_resources(Arc::new(Resources::default())) }
}
impl GitCache {
    pub fn with_resources(resources: Arc<Resources>) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), state: Mutex::new(State::default()), changed: Condvar::new(), topology_gate: Mutex::new(()), watcher: Mutex::new(None), resources, created: Instant::now() }
    }
    fn prefix(&self) -> String { format!("{}:", self.id) }
    pub fn clear_scope(&self) {
        if let Ok(mut state) = self.state.lock() {
            self.resources.release_status(state.entries.values().map(|entry| entry.bytes).sum());
            let generation = state.generation + 1; *state = State { generation, ..State::default() };
        }
        if let Ok(mut watcher) = self.watcher.lock() { *watcher = None; }
        if let Ok(mut content) = self.resources.content.lock() { content.remove_scope(&self.prefix()); }
        self.changed.notify_all();
    }
    pub fn scope(&self, revision: u64, roots: &[PathBuf]) {
        let scope = format!("{revision}:{roots:?}");
        let different = self.state.lock().is_ok_and(|state| state.scope != scope);
        if different {
            // Callers hold the workspace state guard while changing cache scope.
            self.clear_scope();
            if let Ok(mut state) = self.state.lock() { state.scope = scope; }
        }
    }
    pub(super) fn metadata(&self, root: &Path) -> Option<Metadata> {
        self.state.lock().ok()?.metadata.get(root).filter(|meta| meta.marker == metadata_marker(root, &meta.git_dir, &meta.common_dir)).cloned()
    }
    pub(super) fn remember_metadata(&self, root: PathBuf, metadata: Metadata) {
        if let Ok(mut state) = self.state.lock() {
            if state.metadata.len() >= 128 { state.metadata.clear(); }
            state.metadata.insert(root, metadata);
        }
    }
    pub(super) fn topology(&self) -> Option<(Vec<Repository>, Vec<String>)> {
        let state = self.state.lock().ok()?;
        let (sampled, repositories, warnings) = state.topology.as_ref()?;
        if state.topology_dirty || sampled.elapsed() >= FALLBACK { return None; }
        Some((repositories.clone(), warnings.clone()))
    }
    pub fn generation(&self) -> u64 { self.state.lock().map(|state| state.generation).unwrap_or(0) }
    pub(super) fn install(self: &Arc<Self>, generation: u64, repositories: Vec<Repository>, warnings: Vec<String>) {
        if let Ok(mut state) = self.state.lock() {
            if state.generation != generation { return; }
            let previous: usize = state.entries.values().map(|entry| entry.bytes).sum();
            state.entries.retain(|root, _| repositories.iter().any(|repo| &repo.root == root));
            let retained: usize = state.entries.values().map(|entry| entry.bytes).sum();
            self.resources.release_status(previous.saturating_sub(retained));
            state.topology = Some((Instant::now(), repositories.clone(), warnings)); state.topology_dirty = false; state.watcher_error = None;
        }
        // Windows uses an OS recursive subscription, without enumerating dependency
        // directories. Other platforms register only bounded, pruned source dirs.
        let weak = Arc::downgrade(self);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Some(cache) = weak.upgrade() {
                match event {
                    Ok(event) if !matches!(event.kind, notify::EventKind::Access(_)) => for path in event.paths { cache.invalidate_path(&path); },
                    Ok(_) => {},
                    Err(_) => cache.watcher_failed(),
                }
            }
        });
        match watcher {
            Ok(mut watcher) => {
                let mut watched = HashSet::new(); let mut budget = 4096usize;
                for repo in &repositories {
                    #[cfg(windows)]
                    if watched.insert(repo.root.clone()) && watcher.watch(&repo.root, RecursiveMode::Recursive).is_err() { self.watcher_failed(); }
                    #[cfg(not(windows))]
                    {
                        let mut queue = vec![repo.root.clone()];
                        while let Some(path) = queue.pop() {
                            if budget == 0 { self.watcher_failed(); break; }
                            if !watched.insert(path.clone()) { continue; } budget -= 1;
                            if watcher.watch(&path, RecursiveMode::NonRecursive).is_err() { self.watcher_failed(); }
                            if let Ok(entries) = fs::read_dir(&path) {
                                for entry in entries.flatten() {
                                    if queue.len() >= budget { self.watcher_failed(); break; }
                                    if entry.file_type().is_ok_and(|kind| kind.is_dir() && !kind.is_symlink()) && !excluded_directory(&entry.file_name().to_string_lossy()) && entry.file_name() != ".git" { queue.push(entry.path()); }
                                }
                            }
                        }
                    }
                    for directory in [&repo.metadata.git_dir, &repo.metadata.common_dir] {
                        if watched.insert(directory.to_path_buf()) && watcher.watch(directory, RecursiveMode::NonRecursive).is_err() { self.watcher_failed(); }
                        let refs = directory.join("refs");
                        if refs.is_dir() && watched.insert(refs.clone()) && watcher.watch(&refs, RecursiveMode::Recursive).is_err() { self.watcher_failed(); }
                    }
                }
                let _ = &mut budget;
                let mut previous = None;
                if let Ok(state) = self.state.lock() {
                    if state.generation == generation {
                        if let Ok(mut slot) = self.watcher.lock() { previous = slot.replace(watcher); }
                    }
                }
                // A watcher may join its callback thread during Drop; callbacks lock state.
                drop(previous);
            }
            Err(_) => self.watcher_failed(),
        }
    }
    fn watcher_failed(&self) {
        if let Ok(mut state) = self.state.lock() { state.watcher_error = Some("文件监听不可用或达到预算，使用低频完整校验兜底。".into()); }
    }
    pub fn invalidate_path(&self, path: &Path) {
        if path.components().any(|part| { let name = part.as_os_str().to_string_lossy(); !name.eq_ignore_ascii_case(".git") && excluded_directory(&name) }) { return; }
        let Ok(mut state) = self.state.lock() else { return; };
        let Some((_, repositories, _)) = &state.topology else { return; };
        if path.file_name().is_some_and(|name| name == "index.lock" || name.to_string_lossy().ends_with(".lock")) { return; }
        let metadata_event = repositories.iter().any(|repo| within(path, &repo.metadata.git_dir) || within(path, &repo.metadata.common_dir));
        if !metadata_event && path.components().any(|part| excluded_directory(&part.as_os_str().to_string_lossy())) { return; }
        let owner = repositories.iter().filter(|repo| within(path, &repo.root)).max_by_key(|repo| repo.root.components().count());
        let mut roots: Vec<_> = if metadata_event {
            repositories.iter().filter(|repo| within(path, &repo.metadata.git_dir) || within(path, &repo.metadata.common_dir)).map(|repo| repo.root.clone()).collect()
        } else { owner.into_iter().map(|repo| repo.root.clone()).collect() };
        if metadata_event {
            for repo in repositories.iter().filter(|repo| roots.contains(&repo.root)).collect::<Vec<_>>() {
                let mut parent = repo.parent.clone();
                while let Some(value) = parent {
                    if let Some(repo) = repositories.iter().find(|repo| display_path(&repo.root) == value) { roots.push(repo.root.clone()); parent = repo.parent.clone(); } else { break; }
                }
            }
        }
        let topology_dirty = path.file_name().is_some_and(|name| name == ".gitmodules" || name == ".git" || name == "index" || name == "config" || name == "config.worktree") || (!metadata_event && path.is_dir());
        let config_changed = path.file_name().is_some_and(|name| name == "config" || name == "config.worktree" || name == ".git");
        state.topology_dirty |= topology_dirty;
        for root in roots {
            if config_changed { state.metadata.remove(&root); }
            let entry = state.entries.entry(root).or_default(); entry.dirty = entry.dirty.wrapping_add(1); entry.dirty_at = Some(Instant::now());
        }
    }
    pub(super) fn request(self: &Arc<Self>, service: &WorkspaceService, revision: u64, repositories: &[Repository], force: bool) {
        for repo in repositories {
            let (generation, dirty, budget) = {
                let Ok(mut state) = self.state.lock() else { return; };
                let generation = state.generation;
                let entry = state.entries.entry(repo.root.clone()).or_default();
                let changed = entry.dirty != entry.clean && entry.dirty_at.is_none_or(|time| time.elapsed() >= DEBOUNCE);
                let expired = entry.sampled.is_none_or(|time| time.elapsed() >= FALLBACK);
                let retry = entry.attempted.is_none_or(|time| time.elapsed() >= Duration::from_secs(5));
                if entry.refreshing || !(force || ((entry.snapshot.is_none() || changed || expired) && retry)) { continue; }
                entry.refreshing = true;
                (generation, entry.dirty, STATUS_LIMIT / repositories.len().max(1))
            };
            let cache = self.clone(); let service = service.clone(); let repo = repo.clone(); let root = repo.root.clone();
            let submitted = self.resources.pool.get_or_init(Pool::new).submit(Box::new(move || {
                if !cache.state.lock().is_ok_and(|state| state.generation == generation) { return; }
                let started = Instant::now();
                let result: Result<GitRepository> = (|| {
                    service.git_roots(revision, true)?;
                    let _operations = service.git_operations.read().map_err(|_| error("unavailable", "Git 操作暂不可用"))?;
                    let root = service.git_repository(&display_path(&repo.root), revision)?;
                    let metadata = service.git_cache.metadata(&root).ok_or_else(|| error("conflict", "Git 元数据位置已变化，请重试"))?;
                    let mut value = repository_status_display(&service, &root, &metadata.git_dir)?;
                    value.parent = repo.parent.clone(); value.submodule_path = repo.submodule_path.clone();
                    let mut size = 1024usize;
                    value.changes.retain(|file| { size += file.path.len() + file.original_path.as_ref().map_or(0, String::len) + 384; size <= budget });
                    value.changes.shrink_to_fit();
                    if size > budget { value.truncated = true; value.blocked = Some("变更列表达到内存预算，请缩小范围后操作".into()); }
                    service.git_roots(revision, true)?;
                    Ok(value)
                })();
                if let Ok(mut state) = cache.state.lock() {
                    if state.generation != generation { return; }
                    let entry = state.entries.entry(repo.root.clone()).or_default();
                    entry.refreshing = false; entry.attempted = Some(Instant::now());
                    match result {
                        Ok(mut value) => {
                            let base = value.root.len() + value.name.len() + value.branch.len() + 1024;
                            let requested = base + value.changes.iter().map(|file| file.path.len() + file.original_path.as_ref().map_or(0, String::len) + 384).sum::<usize>();
                            let granted = cache.resources.reserve_status(entry.bytes, requested);
                            if granted < requested {
                                let mut size = base;
                                value.changes.retain(|file| { size += file.path.len() + file.original_path.as_ref().map_or(0, String::len) + 384; size <= granted });
                                value.changes.shrink_to_fit();
                                value.truncated = true; value.blocked = Some("共享 Git 状态缓存达到内存预算，请缩小工作区范围".into());
                            }
                            entry.bytes = granted; entry.snapshot = Some(value); entry.sampled = Some(Instant::now()); entry.clean = dirty; entry.error = None;
                        }
                        Err(error) => entry.error = Some(error.message),
                    }
                }
                cache.changed.notify_all();
                if started.elapsed() >= Duration::from_secs(1) { eprintln!("[git-cache] status {}: {}ms", display_path(&repo.root), started.elapsed().as_millis()); }
            }));
            if !submitted {
                if let Ok(mut state) = self.state.lock() { if let Some(entry) = state.entries.get_mut(&root) { entry.refreshing = false; } }
            }
        }
    }
    pub(super) fn result(&self, revision: u64, repositories: &[Repository], mut warnings: Vec<String>, wait: Duration) -> Result<GitStatus> {
        let started = Instant::now();
        let mut state = self.state.lock().map_err(|_| error("unavailable", "Git 缓存暂不可用"))?;
        while started.elapsed() < wait && repositories.iter().any(|repo| state.entries.get(&repo.root).is_some_and(|entry| entry.refreshing)) {
            let timeout = wait.saturating_sub(started.elapsed());
            state = self.changed.wait_timeout(state, timeout).map_err(|_| error("unavailable", "Git 缓存暂不可用"))?.0;
        }
        if let Some(error) = &state.watcher_error { warnings.push(error.clone()); }
        let refreshing = repositories.iter().any(|repo| state.entries.get(&repo.root).is_some_and(|entry| entry.refreshing));
        let stale = repositories.iter().any(|repo| state.entries.get(&repo.root).is_none_or(|entry| entry.snapshot.is_none() || entry.clean != entry.dirty || entry.sampled.is_none_or(|time| time.elapsed() >= FALLBACK)));
        let repositories = repositories.iter().map(|repo| {
            if let Some(entry) = state.entries.get(&repo.root) {
                if let Some(error) = &entry.error { warnings.push(format!("{}：{error}", repo.name)); }
                if let Some(value) = &entry.snapshot { return value.clone(); }
            }
            repo.empty()
        }).collect();
        Ok(GitStatus { revision, repositories, warnings, refreshing, stale })
    }
    pub fn content_get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_slice(&self.raw_get(key)?).ok()
    }
    pub fn raw_get(&self, key: &str) -> Option<Vec<u8>> {
        let key = format!("{}{key}", self.prefix());
        let mut content = self.resources.content.lock().ok()?;
        let index = content.entries.iter().position(|(old, _)| old == &key)?;
        let entry = content.entries.remove(index)?;
        let value = entry.1.clone(); content.entries.push_back(entry); Some(value)
    }
    pub fn content_put<T: Serialize>(&self, key: &str, value: &T) {
        if let Ok(bytes) = serde_json::to_vec(value) {
            self.raw_put(key, bytes);
        }
    }
    pub fn raw_put(&self, key: &str, bytes: Vec<u8>) {
        if let Ok(mut content) = self.resources.content.lock() { content.insert(format!("{}{key}", self.prefix()), bytes); }
    }
    pub fn content_epoch(&self, root: &Path) -> String {
        let dirty = self.state.lock().ok().and_then(|state| state.entries.get(root).map(|entry| entry.dirty)).unwrap_or(0);
        format!("{dirty}:{}", self.created.elapsed().as_secs() / 60)
    }
    pub fn invalidate_repositories(&self, roots: &[PathBuf]) {
        let Ok(mut state) = self.state.lock() else { return; };
        let mut affected = Vec::new();
        if let Some((_, repositories, _)) = &state.topology {
            for root in roots {
                let Some(repo) = repositories.iter().find(|repo| comparison_path(&repo.root) == comparison_path(root)) else { continue; };
                affected.push(repo.root.clone());
                let mut parent = repo.parent.clone();
                while let Some(path) = parent {
                    if let Some(repo) = repositories.iter().find(|repo| display_path(&repo.root) == path) { affected.push(repo.root.clone()); parent = repo.parent.clone(); } else { break; }
                }
            }
        }
        for root in affected {
            let entry = state.entries.entry(root).or_default(); entry.dirty = entry.dirty.wrapping_add(1); entry.dirty_at = Some(Instant::now() - DEBOUNCE);
        }
    }
}
pub(super) struct Invalidate<'a> { pub cache: &'a GitCache, pub roots: Vec<PathBuf> }
impl Drop for Invalidate<'_> { fn drop(&mut self) { self.cache.invalidate_repositories(&self.roots); } }
impl Drop for GitCache {
    fn drop(&mut self) {
        if let Ok(state) = self.state.lock() { self.resources.release_status(state.entries.values().map(|entry| entry.bytes).sum()); }
        if let Ok(mut content) = self.resources.content.lock() { content.remove_scope(&self.prefix()); }
    }
}
pub(super) fn marker(path: &Path) -> String {
    fs::symlink_metadata(path).map(|meta| format!("{}:{:?}", meta.len(), meta.modified().ok())).unwrap_or_default()
}
pub(super) fn metadata_marker(root: &Path, git_dir: &Path, common_dir: &Path) -> String {
    format!("{}:{}:{}:{}", marker(&root.join(".git")), marker(&git_dir.join("config")), marker(&common_dir.join("config")), marker(&git_dir.join("config.worktree")))
}
fn within(path: &Path, root: &Path) -> bool {
    let path = comparison_path(path); let root = comparison_path(root);
    path == root || path.starts_with(&format!("{root}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_lru_is_bounded_and_recent_reads_can_be_retained() {
        let mut cache = Content::default();
        cache.insert("a".into(), vec![0; CONTENT_LIMIT / 2]);
        cache.insert("b".into(), vec![0; CONTENT_LIMIT / 2]);
        cache.insert("c".into(), vec![0; 1]);
        assert!(cache.bytes <= CONTENT_LIMIT); assert!(!cache.entries.iter().any(|(key, _)| key == "a"));
        cache.remove_scope("b"); assert_eq!(cache.bytes, 1);
    }
    #[test]
    fn cache_scope_separates_windows_roots_and_drops_old_content() {
        let cache = GitCache::default(); cache.scope(1, &[PathBuf::from(r"D:\work\a")]);
        cache.content_put("commit", &"old"); assert_eq!(cache.content_get::<String>("commit").as_deref(), Some("old"));
        cache.scope(2, &[PathBuf::from(r"D:\work\b")]); assert!(cache.content_get::<String>("commit").is_none());
    }
    #[test]
    fn worktree_edits_only_dirty_the_child_but_child_heads_dirty_parent_pointers() {
        let cache = GitCache::default();
        let root = PathBuf::from("C:/source/parent"); let child = root.join("modules/child");
        let git_dir = root.join(".git"); let child_git = git_dir.join("modules/child");
        let parent = Repository { root: root.clone(), name: "parent".into(), parent: None, submodule_path: None,
            metadata: Metadata { git_dir: git_dir.clone(), common_dir: git_dir.clone(), marker: String::new() } };
        let child_repo = Repository { root: child.clone(), name: "child".into(), parent: Some(display_path(&root)), submodule_path: Some("modules/child".into()),
            metadata: Metadata { git_dir: child_git.clone(), common_dir: child_git.clone(), marker: String::new() } };
        {
            let mut state = cache.state.lock().unwrap(); state.topology = Some((Instant::now(), vec![parent, child_repo], vec![]));
            state.entries.insert(root.clone(), Entry::default()); state.entries.insert(child.clone(), Entry::default());
        }
        cache.invalidate_path(&child.join("src/Main.java"));
        assert_eq!(cache.state.lock().unwrap().entries[&root].dirty, 0);
        assert_eq!(cache.state.lock().unwrap().entries[&child].dirty, 1);
        cache.invalidate_path(&child_git.join("HEAD"));
        assert!(cache.state.lock().unwrap().entries[&root].dirty > 0);
        cache.invalidate_path(&child.join("node_modules/dependency/index.js"));
        assert_eq!(cache.state.lock().unwrap().entries[&child].dirty, 2);
    }
    #[test]
    fn invitations_share_one_status_and_history_budget_but_not_cache_keys() {
        let resources = Arc::new(Resources::default());
        let owner = GitCache::with_resources(resources.clone()); let guest = GitCache::with_resources(resources.clone());
        owner.content_put("blob", &"owner"); guest.content_put("blob", &"guest");
        assert_eq!(owner.content_get::<String>("blob").as_deref(), Some("owner"));
        assert_eq!(guest.content_get::<String>("blob").as_deref(), Some("guest"));
        assert_eq!(resources.reserve_status(0, STATUS_LIMIT - 10), STATUS_LIMIT - 10);
        assert_eq!(resources.reserve_status(0, 100), 10);
        resources.release_status(STATUS_LIMIT);
        assert_eq!(*resources.status_bytes.lock().unwrap(), 0);
    }
}
