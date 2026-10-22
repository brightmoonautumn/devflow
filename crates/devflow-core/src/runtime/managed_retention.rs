//! Only verified, inactive UUID run directories are eligible for deletion.
use super::{Record, error, lock_launch, read_json, slot};
use crate::workspace::WorkspaceError;
use std::{fs, path::Path, sync::OnceLock, thread, time::Duration};
use crate::runtime::process::process_system;

type Result<T> = std::result::Result<T, WorkspaceError>;
const HISTORY_RUNS: usize = 5;
const HISTORY_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const HANDSHAKE_GRACE_MS: u64 = 60_000;

fn alive(record: &Record) -> bool {
    active_with(&process_system(), record)
}
fn active_with(processes: &sysinfo::System, record: &Record) -> bool {
    record.worker.into_iter().chain(record.pid).any(|(pid, started)| {
        processes.process(sysinfo::Pid::from_u32(pid)).is_some_and(|process|
            started == 0 || process.start_time() == 0 || process.start_time() == started)
    })
}
fn service_name(name: &str) -> bool {
    name.strip_prefix("java-").or_else(|| name.strip_prefix("node-"))
        .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
}
fn verified(directory: &Path, service: &Path) -> Result<Record> {
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() { return Err(error("历史目录不是普通目录")); }
    let canonical = directory.canonicalize()?;
    if canonical.parent() != Some(service) || canonical.file_name() != directory.file_name() { return Err(error("历史目录链接越界")); }
    let record: Record = read_json(&canonical.join("state.json"))?;
    if record.version != 1 || !["java", "node"].contains(&record.kind.as_str())
        || uuid::Uuid::parse_str(&record.id).is_err() || directory.file_name().and_then(|v| v.to_str()) != Some(record.id.as_str()) {
        return Err(error("历史记录身份无效"));
    }
    let root = service.parent().ok_or_else(|| error("历史目录缺少运行根"))?;
    if slot(root, &record.kind, &record.key) != service { return Err(error("历史记录与服务不匹配")); }
    Ok(record)
}
fn modified_ms(path: &Path) -> u64 {
    path.metadata().and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

/// Caller must own launch.lock. The current record remains available even when
/// it is older than seven days; if ended it counts toward the five-run budget.
pub(super) fn prune_locked(directory: &Path, now: u64) -> Result<()> {
    let service = directory.canonicalize()?;
    let current: String = match read_json(&service.join("current.json")) { Ok(value) => value, Err(_) => return Ok(()) };
    if uuid::Uuid::parse_str(&current).is_err() { return Ok(()); }
    let mut entries = fs::read_dir(&service)?.take(10_001).collect::<std::io::Result<Vec<_>>>()?;
    if entries.len() > 10_000 { return Err(error("运行历史条目超过清理预算")); }
    entries.sort_by_key(|entry| entry.file_name());
    let mut historical = Vec::new();
    let mut current_ended = false;
    let processes = process_system();
    for entry in entries {
        if entry.file_name().to_str().is_none_or(|name| uuid::Uuid::parse_str(name).is_err()) { continue; }
        let Ok(record) = verified(&entry.path(), &service) else { continue; };
        if active_with(&processes, &record) || (record.state == "starting" && now.saturating_sub(record.created_ms) < HANDSHAKE_GRACE_MS) { continue; }
        if record.id == current {
            current_ended = true;
            if let Err(err) = super::logs::compact_legacy(&entry.path()) { eprintln!("DevFlow 历史日志压缩失败: {err}"); }
            continue;
        }
        // Legacy records lack a finish timestamp; the last state/log write is
        // a more accurate fallback than the original process start time.
        let ended = record.finished_ms.unwrap_or_else(|| record.created_ms
            .max(modified_ms(&entry.path().join("state.json")))
            .max(modified_ms(&entry.path().join("stdout.log")))
            .max(modified_ms(&entry.path().join("stderr.log"))));
        historical.push((ended, entry.path()));
    }
    historical.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let keep = HISTORY_RUNS.saturating_sub(usize::from(current_ended));
    for (index, (ended, path)) in historical.into_iter().enumerate() {
        if index < keep && now.saturating_sub(ended) <= HISTORY_MS {
            if let Err(err) = super::logs::compact_legacy(&path) { eprintln!("DevFlow 历史日志压缩失败: {err}"); }
            continue;
        }
        // Recheck the resolved target and process identities immediately before
        // recursive deletion. Unknown/corrupt records are always preserved.
        let Ok(record) = verified(&path, &service) else { continue; };
        if record.id == current || alive(&record) { continue; }
        let target = path.canonicalize()?;
        if target.parent() != Some(service.as_path()) || !target.starts_with(&service) { continue; }
        if let Err(err) = fs::remove_dir_all(&target) { eprintln!("DevFlow 历史清理失败 {}: {err}", target.display()); }
    }
    Ok(())
}

pub(super) fn sweep(root: &Path, now: u64) -> Result<()> {
    if !root.is_dir() { return Ok(()); }
    let root = root.canonicalize()?;
    for entry in fs::read_dir(&root)?.take(4096) {
        let entry = entry?;
        if !service_name(&entry.file_name().to_string_lossy()) || !entry.file_type()?.is_dir() { continue; }
        let Ok(path) = entry.path().canonicalize() else { continue; };
        if path.parent() != Some(root.as_path()) || path.file_name() != Some(entry.file_name().as_os_str()) { continue; }
        // Contention is expected while another host is launching a service.
        let Ok(_guard) = lock_launch(&path) else { continue; };
        if let Err(err) = prune_locked(&path, now) { eprintln!("DevFlow 历史清理失败: {err}"); }
    }
    Ok(())
}

pub(super) fn start(root: std::path::PathBuf) -> Result<()> {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() { return Ok(()); }
    thread::Builder::new().name("devflow-runtime-retention".into()).spawn(move || loop {
        if let Err(err) = sweep(&root, super::now_ms()) { eprintln!("DevFlow 历史清理失败: {err}"); }
        thread::sleep(Duration::from_secs(60 * 60));
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::process::identity;
    use std::path::PathBuf;
    struct Fixture { root: PathBuf, service: PathBuf }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("devflow-history-中文 空格-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap(); let root = root.canonicalize().unwrap();
            let service = slot(&root, "java", "D:/中文 空格/app"); fs::create_dir_all(&service).unwrap();
            Self { root, service }
        }
        fn run(&self, ended: u64, live: bool) -> PathBuf {
            let id = uuid::Uuid::new_v4().to_string(); let path = self.service.join(&id); fs::create_dir_all(&path).unwrap();
            let record = Record { version: 1, kind: "java".into(), key: "D:/中文 空格/app".into(), id,
                created_ms: ended, finished_ms: Some(ended), worker: live.then(|| identity(std::process::id()).unwrap()), pid: None,
                state: if live { "running" } else { "stopped" }.into(), exit: Some(0) };
            super::super::write_json(&path.join("state.json"), &record).unwrap(); path
        }
        fn current(&self, path: &Path) { super::super::write_json(&self.service.join("current.json"), &path.file_name().unwrap().to_string_lossy()).unwrap(); }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }
    #[test]
    fn bounds_history_and_preserves_current_live_unknown_and_nonservice_dirs() {
        let f = Fixture::new(); let now = HISTORY_MS * 3;
        let current = f.run(1, true); f.current(&current);
        let live = f.run(1, true);
        let expired = f.run(now - HISTORY_MS - 1, false);
        let mut history = Vec::new();
        for age in 1..=7 { history.push(f.run(now - age * 1000, false)); }
        let unknown = f.service.join(uuid::Uuid::new_v4().to_string()); fs::create_dir(&unknown).unwrap();
        let tools = f.root.join("hosts"); fs::create_dir(&tools).unwrap(); fs::write(tools.join("keep.exe"), b"fixture").unwrap();
        let cache = f.root.join("maven"); fs::create_dir(&cache).unwrap();
        sweep(&f.root, now).unwrap();
        assert!(current.exists() && live.exists() && unknown.exists() && tools.exists() && cache.exists());
        assert!(!expired.exists());
        assert!(history[..5].iter().all(|path| path.exists()));
        assert!(history[5..].iter().all(|path| !path.exists()));
    }
    #[test]
    fn stopped_current_counts_toward_budget_but_is_never_expired() {
        let f = Fixture::new(); let now = HISTORY_MS * 3;
        let current = f.run(1, false); f.current(&current);
        let history: Vec<_> = (1..=5).map(|age| f.run(now - age * 1000, false)).collect();
        let guard = lock_launch(&f.service).unwrap();
        prune_locked(&f.service, now).unwrap(); drop(guard);
        assert!(current.exists());
        assert!(history[..4].iter().all(|path| path.exists()));
        assert!(!history[4].exists());
    }
    #[test]
    fn mismatched_record_and_launch_lock_contention_are_preserved() {
        let f = Fixture::new(); let current = f.run(1, false); f.current(&current);
        let expired = f.run(2, false);
        let corrupt = f.run(3, false);
        let mut record: Record = read_json(&corrupt.join("state.json")).unwrap(); record.key = "another-service".into();
        super::super::write_json(&corrupt.join("state.json"), &record).unwrap();
        let guard = lock_launch(&f.service).unwrap(); sweep(&f.root, HISTORY_MS * 3).unwrap();
        assert!(expired.exists()); drop(guard);
        sweep(&f.root, HISTORY_MS * 3).unwrap();
        assert!(!expired.exists() && corrupt.exists() && current.exists());
    }
}
