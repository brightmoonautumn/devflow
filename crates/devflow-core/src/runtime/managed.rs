//! Durable service host. The HTTP/MCP process is only a client of this host.
//! A separate executable owns launch sequencing and file-backed stdout/stderr.
use super::process::{Run, identity, kill_tree};
use crate::workspace::WorkspaceError;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, ffi::OsString, fs, io::{Read, Seek, SeekFrom, Write}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::{Arc, Mutex}, thread, time::Duration};

type Result<T> = std::result::Result<T, WorkspaceError>;
fn error(message: impl Into<String>) -> WorkspaceError { WorkspaceError { code: "io_error", message: message.into() } }

#[derive(Serialize, Deserialize)]
pub(crate) struct CommandSpec {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    env: Vec<(OsString, Option<OsString>)>,
}
impl CommandSpec {
    pub(crate) fn from(command: &Command) -> Self {
        Self { program: command.get_program().to_owned(), args: command.get_args().map(OsString::from).collect(), cwd: command.get_current_dir().map(Path::to_owned), env: command.get_envs().map(|(key, value)| (key.to_owned(), value.map(OsString::from))).collect() }
    }
    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if let Some(cwd) = &self.cwd { command.current_dir(cwd); }
        for (key, value) in &self.env {
            if let Some(value) = value { command.env(key, value); } else { command.env_remove(key); }
        }
        command
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) enum LaunchPlan {
    Node { command: CommandSpec },
    Java { build: CommandSpec, java: CommandSpec, root: PathBuf, classpath_file: PathBuf, main: String, args: Vec<String>, profiles: Vec<String> },
    JavaCached { maven: super::maven::MavenPlan, java: CommandSpec, root: PathBuf, main: String, args: Vec<String>, profiles: Vec<String> },
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u8,
    kind: String,
    key: String,
    id: String,
    created_ms: u64,
    worker: Option<(u32, u64)>,
    pid: Option<(u32, u64)>,
    state: String,
    exit: Option<i32>,
}
fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }

struct LaunchLock { _file: fs::File }
fn lock_launch(slot: &Path) -> Result<LaunchLock> {
    fs::create_dir_all(slot)?;
    // OS releases this lock even when DevFlow is force-terminated. Do not unlink
    // the file, which could let two processes lock different file objects.
    let file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(slot.join("launch.lock"))?;
    file.try_lock().map_err(|err| WorkspaceError { code: "conflict", message: format!("无法取得服务启动锁，可能正由另一个实例启动：{err}") })?;
    Ok(LaunchLock { _file: file })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 1024 * 1024 { return Err(error("运行记录过大")); }
    serde_json::from_reader(file).map_err(|err| error(format!("运行记录无效：{err}")))
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&temporary)?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; file.set_permissions(fs::Permissions::from_mode(0o600))?; }
    serde_json::to_writer(&mut file, value).map_err(|err| error(err.to_string()))?;
    file.flush()?;
    drop(file);
    fs::rename(temporary, path)?;
    Ok(())
}
fn root() -> Result<PathBuf> { Ok(std::env::current_dir()?.join(".devflow/runtime")) }
fn slot(root: &Path, kind: &str, key: &str) -> PathBuf {
    // Stable across process/compiler versions. Record key validation below is
    // still mandatory: this filename hash is not an authorization mechanism.
    let hash = key.bytes().fold(14695981039346656037u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(1099511628211));
    root.join(format!("{kind}-{hash:016x}"))
}

struct LogReader { offset: u64, pending: Vec<u8> }
pub(crate) struct ManagedRun {
    directory: PathBuf,
    id: String,
    kind: String,
    key: String,
    stdout: LogReader,
    stderr: LogReader,
}
impl ManagedRun {
    fn new(directory: PathBuf, record: &Record) -> Self {
        Self { directory, id: record.id.clone(), kind: record.kind.clone(), key: record.key.clone(), stdout: LogReader { offset: 0, pending: vec![] }, stderr: LogReader { offset: 0, pending: vec![] } }
    }
    fn record(&self) -> Result<Record> {
        let record: Record = read_json(&self.directory.join("state.json"))?;
        if record.version != 1 || record.id != self.id || record.kind != self.kind || record.key != self.key {
            return Err(error("运行记录与当前服务不匹配"));
        }
        Ok(record)
    }
}

pub(crate) fn restore_into(runs: &mut HashMap<String, Arc<Mutex<Run>>>, kind: &str, key: &str) -> Result<()> {
    if runs.contains_key(key) { return Ok(()); }
    if let Some(run) = restore(&root()?, kind, key)? { runs.insert(key.to_owned(), Arc::new(Mutex::new(run))); }
    Ok(())
}
fn restore(root: &Path, kind: &str, key: &str) -> Result<Option<Run>> {
    let slot = slot(root, kind, key);
    let id: String = match read_json(&slot.join("current.json")) {
        Ok(id) => id,
        Err(_) if !slot.join("current.json").exists() => return Ok(None),
        Err(err) => return Err(err),
    };
    if uuid::Uuid::parse_str(&id).is_err() { return Err(error("运行标识无效")); }
    let directory = slot.join(&id);
    let record: Record = read_json(&directory.join("state.json"))?;
    let managed = ManagedRun::new(directory, &record);
    if record.id != id || record.key != key || record.kind != kind { return Err(error("服务运行记录不匹配")); }
    managed.record()?;
    let mut run = Run::new("starting");
    // A new presentation runId tells clients to reset their in-memory cursor.
    run.managed = Some(managed);
    refresh(&mut run)?;
    run.log("system", "已恢复独立运行记录；显示保留的文件日志。");
    Ok(Some(run))
}

pub(crate) fn refresh(run: &mut Run) -> Result<()> {
    let Some(mut managed) = run.managed.take() else { return Ok(()); };
    let result = (|| {
        let record = managed.record()?;
        run.worker = record.worker.filter(|target| identity(target.0) == Some(*target));
        run.pid = record.pid.filter(|target| identity(target.0) == Some(*target));
        run.exit = record.exit;
        let pending_handshake = record.state == "starting" && record.worker.is_none() && now_ms().saturating_sub(record.created_ms) < 10000;
        run.state = record.state;
        if managed.directory.join("stop").exists() { run.cancelled = true; }
        if matches!(run.state.as_str(), "starting" | "running") && run.worker.is_none() {
            run.state = if run.pid.is_some() { "running" } else if run.cancelled { "stopped" } else if pending_handshake { "starting" } else { "failed" }.into();
        }
        let finished = !matches!(run.state.as_str(), "starting" | "running");
        read_log(&managed.directory.join("stdout.log"), &mut managed.stdout, run, "stdout", finished)?;
        read_log(&managed.directory.join("stderr.log"), &mut managed.stderr, run, "stderr", finished)?;
        Ok(())
    })();
    run.managed = Some(managed);
    result
}

fn read_log(path: &Path, reader: &mut LogReader, run: &mut Run, stream: &str, finished: bool) -> Result<()> {
    let mut file = match fs::File::open(path) { Ok(file) => file, Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()), Err(err) => return Err(err.into()) };
    let length = file.metadata()?.len();
    if length < reader.offset { reader.offset = 0; reader.pending.clear(); }
    // Bounded catch-up even if a detached service has emitted gigabytes.
    if length.saturating_sub(reader.offset) > 256 * 1024 {
        reader.offset = length - 256 * 1024;
        reader.pending.clear();
        run.log("system", format!("{stream} 较早的日志已省略，完整日志保留在磁盘。"));
    }
    file.seek(SeekFrom::Start(reader.offset))?;
    let mut bytes = Vec::new();
    file.take(256 * 1024).read_to_end(&mut bytes)?;
    reader.offset += bytes.len() as u64;
    for byte in bytes {
        if byte == b'\n' || reader.pending.len() >= 16384 {
            run.log(stream, String::from_utf8_lossy(&reader.pending).trim_end_matches('\r').to_owned());
            reader.pending.clear();
        }
        if byte != b'\n' { reader.pending.push(byte); }
    }
    if finished && !reader.pending.is_empty() {
        run.log(stream, String::from_utf8_lossy(&reader.pending).trim_end_matches('\r').to_owned());
        reader.pending.clear();
    }
    Ok(())
}

/// Copy the host so rebuilding/restarting devflow.exe neither locks nor targets
/// the running worker executable. Each host version has an immutable filename.
fn host(root: &Path) -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let metadata = executable.metadata()?;
    let stamp = metadata.modified()?.duration_since(std::time::UNIX_EPOCH).map_err(|err| error(err.to_string()))?.as_nanos();
    let directory = root.join("hosts");
    fs::create_dir_all(&directory)?;
    let extension = if cfg!(windows) { ".exe" } else { "" };
    let target = directory.join(format!("devflow-runtime-worker-{}-{stamp}{extension}", metadata.len()));
    if !target.exists() {
        let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        fs::copy(&executable, &temporary)?;
        if let Err(err) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            if !target.exists() { return Err(err.into()); }
        }
    }
    Ok(target)
}

pub(crate) fn launch(kind: &str, key: &str, plan: LaunchPlan) -> Result<Run> {
    let root = root()?;
    let executable = host(&root)?;
    let id = uuid::Uuid::new_v4().to_string();
    let slot = slot(&root, kind, key);
    let _launch_lock = lock_launch(&slot)?;
    if restore(&root, kind, key)?.is_some_and(|run| matches!(run.state.as_str(), "starting" | "running")) {
        return Err(WorkspaceError { code: "conflict", message: "该服务已在独立运行，请刷新后查看".into() });
    }
    let directory = slot.join(&id);
    fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?; }
    let record = Record { version: 1, kind: kind.into(), key: key.into(), id: id.clone(), created_ms: now_ms(), worker: None, pid: None, state: "starting".into(), exit: None };
    write_json(&directory.join("plan.json"), &plan)?;
    write_json(&directory.join("state.json"), &record)?;
    write_json(&slot.join("current.json"), &id)?;
    let mut command = Command::new(executable);
    command.arg("--devflow-runtime-worker").arg(&directory).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x00000008 | 0x00000200); }
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.spawn()?;
    let mut run = Run::new("starting");
    run.id = id;
    run.managed = Some(ManagedRun::new(directory.clone(), &record));
    // Wait only for the host handshake, never for Maven or application startup.
    for _ in 0..150 {
        let record: Record = read_json(&directory.join("state.json"))?;
        if record.worker.is_some_and(|worker| worker.0 == child.id()) {
            refresh(&mut run)?;
            thread::spawn(move || { let _ = child.wait(); });
            return Ok(run);
        }
        if let Some(status) = child.try_wait()? { return Err(error(format!("独立服务宿主启动失败：{status}"))); }
        thread::sleep(Duration::from_millis(20));
    }
    // Cancel before any eventual late-starting host is allowed to launch code.
    fs::write(directory.join("stop"), [])?;
    let _ = child.kill();
    let _ = child.wait();
    Err(error("独立服务宿主启动超时"))
}

pub(crate) fn stop(run: &mut Run) -> Result<()> {
    if let Some(managed) = &run.managed {
        fs::write(managed.directory.join("stop"), [])?;
    }
    run.cancelled = true;
    // The durable host owns startup transitions. Request cancellation first,
    // allowing it to stop a process spawned concurrently with this request.
    for _ in 0..30 {
        refresh(run)?;
        if run.worker.is_none() { break; }
        thread::sleep(Duration::from_millis(50));
    }
    if let Some(worker) = run.worker { kill_tree(worker)?; }
    if let Some(pid) = run.pid { kill_tree(pid)?; }
    run.pid = None;
    run.worker = None;
    run.state = "stopped".into();
    Ok(())
}

/// Private executable mode, not exposed as an HTTP/MCP arbitrary-command API.
pub fn run_worker(directory: &Path) -> Result<()> {
    let mut record: Record = read_json(&directory.join("state.json"))?;
    if record.version != 1 || directory.file_name().and_then(|name| name.to_str()) != Some(record.id.as_str()) {
        return Err(error("独立服务宿主记录无效"));
    }
    let plan: LaunchPlan = read_json(&directory.join("plan.json"))?;
    fs::remove_file(directory.join("plan.json"))?;
    record.worker = Some(identity(std::process::id()).ok_or_else(|| error("无法校验服务宿主 PID"))?);
    write_json(&directory.join("state.json"), &record)?;
    let outcome = (|| -> Result<Option<i32>> {
        match plan {
            LaunchPlan::Node { command } => execute(command.command(), directory, &mut record, "running"),
            LaunchPlan::Java { build, java, root, classpath_file, main, args, profiles } => {
                let result = (|| {
                    let code = execute(build.command(), directory, &mut record, "starting")?;
                    if code != Some(0) || directory.join("stop").exists() { return Ok(code); }
                    let classes = root.join("target/classes").canonicalize()?;
                    if !classes.starts_with(&root) { return Err(error("编译输出超出项目目录")); }
                    let dependencies = fs::read_to_string(&classpath_file)?;
                    let mut command = java.command();
                    command.arg("-cp").arg(format!("{}{}{}", crate::workspace::display_path(&classes), if cfg!(windows) { ";" } else { ":" }, dependencies.trim())).arg(main).args(args).current_dir(root);
                    if !profiles.is_empty() { command.arg(format!("--spring.profiles.active={}", profiles.join(","))); }
                    execute(command, directory, &mut record, "running")
                })();
                let _ = fs::remove_file(classpath_file);
                result
            },
            LaunchPlan::JavaCached { maven, java, root, main, args, profiles } => {
                let classpath = super::maven::build(&maven, directory, |command, tick| {
                    execute_with(command, directory, &mut record, "starting", tick)
                })?;
                if let Some(classpath) = classpath {
                    let mut command = java.command();
                    command.arg("-cp").arg(classpath).arg(main).args(args).current_dir(root);
                    if !profiles.is_empty() { command.arg(format!("--spring.profiles.active={}", profiles.join(","))); }
                    execute(command, directory, &mut record, "running")
                } else { Ok(None) }
            },
        }
    })();
    record.pid = record.pid.filter(|pid| identity(pid.0) == Some(*pid));
    match outcome {
        Ok(code) => { record.exit = code; record.state = if directory.join("stop").exists() || code == Some(0) { "stopped" } else { "failed" }.into(); },
        Err(err) => {
            record.state = if record.pid.is_some() { "running" } else if directory.join("stop").exists() { "stopped" } else { "failed" }.into();
            if let Ok(mut log) = fs::OpenOptions::new().append(true).create(true).open(directory.join("stderr.log")) { let _ = writeln!(log, "{}", err.message); }
        },
    }
    write_json(&directory.join("state.json"), &record)
}

fn execute(command: Command, directory: &Path, record: &mut Record, phase: &str) -> Result<Option<i32>> {
    execute_with(command, directory, record, phase, &mut || Ok(()))
}

fn execute_with(mut command: Command, directory: &Path, record: &mut Record, phase: &str, tick: &mut dyn FnMut() -> Result<()>) -> Result<Option<i32>> {
    if directory.join("stop").exists() { return Ok(None); }
    let stdout = fs::OpenOptions::new().append(true).create(true).open(directory.join("stdout.log"))?;
    let stderr = fs::OpenOptions::new().append(true).create(true).open(directory.join("stderr.log"))?;
    command.stdin(Stdio::null()).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut child = command.spawn()?;
    let Some(pid) = identity(child.id()) else { let _ = child.kill(); let _ = child.wait(); return Err(error("无法校验服务 PID")); };
    record.pid = Some(pid);
    record.state = phase.into();
    if let Err(err) = write_json(&directory.join("state.json"), record) { let _ = kill_tree(pid); let _ = child.wait(); return Err(err); }
    loop {
        if directory.join("stop").exists() { kill_tree(pid)?; }
        if let Err(err) = tick() {
            let _ = kill_tree(pid);
            let _ = child.kill();
            let _ = child.wait();
            return Err(err);
        }
        if let Some(status) = child.try_wait()? { return Ok(status.code()); }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("devflow-runtime-中文 空格-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn record(&self, kind: &str, key: &str) -> (PathBuf, Record) {
            let id = uuid::Uuid::new_v4().to_string();
            let slot = slot(&self.0, kind, key);
            let directory = slot.join(&id);
            fs::create_dir_all(&directory).unwrap();
            let current = identity(std::process::id()).unwrap();
            let stale = (current.0, current.1 + 1);
            let record = Record { version: 1, kind: kind.into(), key: key.into(), id: id.clone(), created_ms: 0, worker: Some(stale), pid: Some(stale), state: "running".into(), exit: None };
            write_json(&directory.join("state.json"), &record).unwrap();
            write_json(&slot.join("current.json"), &id).unwrap();
            (directory, record)
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn recovery_rejects_reused_pids_and_preserves_explicit_stop_state() {
        let fixture = Fixture::new();
        let (directory, record) = fixture.record("node", "node:C:/项目#dev");
        let run = restore(&fixture.0, "node", &record.key).unwrap().unwrap();
        assert!(run.pid.is_none()); assert!(run.worker.is_none());
        assert_eq!(run.state, "failed");
        assert_ne!(run.id, record.id);
        fs::write(directory.join("stop"), []).unwrap();
        let run = restore(&fixture.0, "node", &record.key).unwrap().unwrap();
        assert_eq!(run.state, "stopped");
        // Disk metadata alone never authorizes a different project.
        let mut wrong = record;
        wrong.key = "another-project".into();
        write_json(&directory.join("state.json"), &wrong).unwrap();
        assert!(restore(&fixture.0, "node", "node:C:/项目#dev").is_err());
    }

    #[test]
    fn logfile_reader_preserves_utf8_fragments_and_bounds_catch_up() {
        let fixture = Fixture::new();
        let path = fixture.0.join("stdout.log");
        let mut run = Run::new("running");
        let mut reader = LogReader { offset: 0, pending: vec![] };
        let data = "中文输出\n".as_bytes();
        fs::write(&path, &data[..2]).unwrap();
        read_log(&path, &mut reader, &mut run, "stdout", false).unwrap();
        assert!(run.logs.is_empty());
        fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&data[2..]).unwrap();
        read_log(&path, &mut reader, &mut run, "stdout", false).unwrap();
        assert_eq!(run.logs.back().unwrap().text, "中文输出");
        fs::write(&path, "line\n".repeat(100000)).unwrap();
        read_log(&path, &mut reader, &mut run, "stdout", true).unwrap();
        assert!(run.logs.len() <= 2000);
        assert_eq!(reader.offset, path.metadata().unwrap().len());
    }

    #[test]
    fn launch_lock_releases_on_drop_and_service_slots_are_distinct() {
        let fixture = Fixture::new();
        let guard = lock_launch(&fixture.0).unwrap();
        assert!(lock_launch(&fixture.0).is_err());
        drop(guard);
        assert!(lock_launch(&fixture.0).is_ok());
        assert_ne!(slot(&fixture.0, "node", "node:C:/项目#dev"), slot(&fixture.0, "node", "node:C:/项目#docs"));
        assert_ne!(slot(&fixture.0, "java", "C:/项目"), slot(&fixture.0, "node", "C:/项目"));
    }

    #[test]
    fn command_manifest_round_trips_arguments_without_shell_interpretation() {
        let mut command = Command::new("C:/工具/node.exe");
        command.args(["C:/项目 空格/pnpm.cjs", "run", "dev", "a&b", "中文"]).env("EXAMPLE", "a b");
        let bytes = serde_json::to_vec(&CommandSpec::from(&command)).unwrap();
        let restored: CommandSpec = serde_json::from_slice(&bytes).unwrap();
        let restored = restored.command();
        assert_eq!(restored.get_program(), command.get_program());
        assert_eq!(restored.get_args().collect::<Vec<_>>(), command.get_args().collect::<Vec<_>>());
        assert_eq!(restored.get_envs().collect::<Vec<_>>(), command.get_envs().collect::<Vec<_>>());
    }
}
