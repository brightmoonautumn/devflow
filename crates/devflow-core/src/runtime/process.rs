//! Bounded process output and PID-identity-checked process cleanup shared by Java and Node.
use crate::workspace::WorkspaceError;
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::Read,
    process::{Child, Command, Stdio},
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
    pub(crate) cancelled: bool,
    pub(crate) exit: Option<i32>,
    pub(crate) logs: VecDeque<LogLine>,
    pub(crate) sequence: u64,
    pub(crate) arthas_url: Option<String>,
    pub(crate) attaching: bool,
}
impl Run {
    pub(crate) fn new(state: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            state: state.into(),
            pid: None,
            cancelled: false,
            exit: None,
            logs: VecDeque::new(),
            sequence: 0,
            arthas_url: None,
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
    process_system()
        .process(Pid::from_u32(pid))
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
    for (pid, started) in tree {
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
    Ok(())
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
pub(crate) fn execute(
    mut command: Command,
    run: &Arc<Mutex<Run>>,
    phase: &str,
) -> Result<Option<i32>> {
    configure(&mut command);
    let mut state = run.lock().unwrap();
    if state.cancelled {
        return Err(error("cancelled", "运行已取消"));
    }
    let mut child: Child = command.spawn()?;
    state.pid = identity(child.id());
    state.state = phase.into();
    drop(state);
    let out = reader(child.stdout.take().unwrap(), run.clone(), "stdout");
    let err = reader(child.stderr.take().unwrap(), run.clone(), "stderr");
    let status = child.wait()?;
    let _ = out.join();
    let _ = err.join();
    run.lock().unwrap().pid = None;
    Ok(status.code())
}
