//! Bounded process output and PID-identity-checked process cleanup shared by Java and Node.
use crate::workspace::WorkspaceError;
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::Read,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
type Result<T> = std::result::Result<T, WorkspaceError>;
fn error(code: &'static str, message: impl Into<String>) -> WorkspaceError {
    WorkspaceError {
        code,
        message: message.into(),
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub sequence: u64,
    pub stream: String,
    pub text: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOutput {
    pub run_id: String,
    pub state: String,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub lines: Vec<LogLine>,
    pub next_cursor: u64,
    pub truncated: bool,
    pub arthas_url: Option<String>,
    pub attaching: bool,
}
pub(crate) struct Run {
    pub(crate) id: String,
    pub(crate) state: String,
    pub(crate) pid: Option<(u32, u64)>,
    pub(crate) worker: Option<(u32, u64)>,
    pub(crate) managed: Option<super::managed::ManagedRun>,
    pub(crate) cancelled: bool,
    pub(crate) exit: Option<i32>,
    pub(crate) logs: VecDeque<LogLine>,
    pub(crate) sequence: u64,
    pub(crate) arthas_url: Option<String>,
    pub(crate) arthas_target: Option<(u32, u64)>,
    pub(crate) attaching: bool,
}
impl Run {
    pub(crate) fn new(state: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            state: state.into(),
            pid: None,
            worker: None,
            managed: None,
            cancelled: false,
            exit: None,
            logs: VecDeque::new(),
            sequence: 0,
            arthas_url: None,
            arthas_target: None,
            attaching: false,
        }
    }
    pub(crate) fn log(&mut self, stream: &str, text: impl Into<String>) {
        self.sequence += 1;
        self.logs.push_back(LogLine {
            sequence: self.sequence,
            stream: stream.into(),
            text: text.into(),
        });
        while self.logs.len() > 2000 {
            self.logs.pop_front();
        }
    }
    pub(crate) fn output(&self, after: u64) -> ProcessOutput {
        ProcessOutput {
            run_id: self.id.clone(),
            state: self.state.clone(),
            pid: self.pid.map(|p| p.0),
            exit_code: self.exit,
            lines: self
                .logs
                .iter()
                .filter(|line| line.sequence > after)
                .take(500)
                .cloned()
                .collect(),
            next_cursor: self
                .logs
                .iter()
                .filter(|line| line.sequence > after)
                .take(500)
                .last()
                .map_or(after.min(self.sequence), |line| line.sequence),
            truncated: self
                .logs
                .front()
                .is_some_and(|line| after.saturating_add(1) < line.sequence),
            arthas_url: self.arthas_url.clone(),
            attaching: self.attaching,
        }
    }
}
pub(crate) fn process_system() -> System {
    let mut s = System::new();
    s.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything().without_tasks(),
    );
    s
}
pub(crate) fn identity(pid: u32) -> Option<(u32, u64)> {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
    system.process(Pid::from_u32(pid))
        .map(|p| (pid, p.start_time()))
}
pub(crate) fn kill_tree(root: (u32, u64)) -> Result<()> {
    let system = process_system();
    if system
        .process(Pid::from_u32(root.0))
        .is_none_or(|p| p.start_time() != root.1)
    {
        return Ok(());
    }
    let mut tree = vec![root];
    for _ in 0..32 {
        let mut added = false;
        for (pid, p) in system.processes() {
            if p.parent()
                .is_some_and(|parent| tree.iter().any(|(id, _)| *id == parent.as_u32()))
                && !tree.iter().any(|(id, _)| *id == pid.as_u32())
            {
                tree.push((pid.as_u32(), p.start_time()));
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    // Stop the root first so a build/launcher cannot create more children.
    let mut fresh = System::new();
    for &(pid, started) in &tree {
        let pid = Pid::from_u32(pid);
        fresh.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        if let Some(p) = fresh.process(pid) {
            if p.start_time() == started && !p.kill() {
                // TerminateProcess may report access denied when a process exits
                // between the identity check and kill. Recheck before reporting it.
                for _ in 0..5 {
                    thread::sleep(Duration::from_millis(40));
                    fresh.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                    if fresh.process(pid).is_none_or(|p| p.start_time() != started) {
                        break;
                    }
                }
                if fresh
                    .process(pid)
                    .is_some_and(|p| p.start_time() == started)
                {
                    return Err(error("permission_denied", format!("无法停止 PID {pid}")));
                }
            }
        }
    }
    // Termination is asynchronous on Windows. Do not release ports to a new
    // launch until every captured process has exited (or its PID was reused).
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let remaining = tree.iter().find(|(pid, started)| {
            let pid = Pid::from_u32(*pid);
            fresh.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
            fresh.process(pid).is_some_and(|p| p.start_time() == *started
                && p.status() != sysinfo::ProcessStatus::Zombie)
        });
        let Some((pid, _)) = remaining else { return Ok(()); };
        if std::time::Instant::now() >= deadline {
            return Err(error("conflict", format!("PID {pid} 尚未退出，请稍后重试")));
        }
        thread::sleep(Duration::from_millis(40));
    }
}
pub(crate) fn configure(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
}
pub(crate) fn reader(
    input: impl Read + Send + 'static,
    run: Arc<Mutex<Run>>,
    stream: &'static str,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut input = input;
        let mut chunk = [0u8; 4096];
        let mut line = Vec::new();
        loop {
            match input.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for byte in &chunk[..n] {
                        if *byte == b'\n' || line.len() >= 16384 {
                            let text = String::from_utf8_lossy(&line)
                                .trim_end_matches('\r')
                                .to_owned();
                            run.lock().unwrap().log(stream, text);
                            line.clear();
                        }
                        if *byte != b'\n' {
                            line.push(*byte);
                        }
                    }
                }
            }
        }
        if !line.is_empty() {
            run.lock()
                .unwrap()
                .log(stream, String::from_utf8_lossy(&line).into_owned());
        }
    })
}

#[cfg(all(test, windows))]
mod restart_cleanup_tests {
    use super::*;

    #[test]
    fn stopping_waits_for_windows_launcher_and_child_exit() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "ping -n 60 127.0.0.1 >nul"]);
        configure(&mut command);
        let mut child = command.spawn().unwrap();
        let root = identity(child.id()).unwrap();
        let mut descendants = Vec::new();
        for _ in 0..50 {
            let system = process_system();
            descendants = system.processes().iter()
                .filter(|(_, p)| p.parent() == Some(Pid::from_u32(root.0)))
                .map(|(pid, p)| (pid.as_u32(), p.start_time())).collect();
            if !descendants.is_empty() { break; }
            thread::sleep(Duration::from_millis(20));
        }
        let result = kill_tree(root);
        let _ = child.wait();
        // Cleanup takes place before assertions, including on a slow test host.
        assert!(result.is_ok());
        assert!(!descendants.is_empty(), "expected ping child process");
        assert_ne!(identity(root.0), Some(root));
        for pid in descendants { assert_ne!(identity(pid.0), Some(pid)); }
    }
}
