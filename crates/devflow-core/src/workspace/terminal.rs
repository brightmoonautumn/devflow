//! Interactive owner-only terminals. Follows VS Code's xterm / PTY split;
//! uses portable-pty (ConPTY on Windows), not VS Code's Electron/node-pty host.
use super::*;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{collections::VecDeque, sync::Arc, thread};

const CAPACITY: usize = 1024 * 1024;
const PAGE: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalRequest {
    pub expected_revision: u64,
    pub operation: TerminalOperation,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalOperation {
    List,
    Create { folder: String, cols: u16, rows: u16 },
    Read { id: String, after: u64 },
    Input { id: String, data: String },
    Resize { id: String, cols: u16, rows: u16 },
    Close { id: String },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    id: String,
    folder: String,
    shell: String,
    pid: Option<u32>,
    exited: bool,
    exit_code: Option<u32>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalResponse {
    sessions: Vec<TerminalInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    data: Vec<u8>,
    next_cursor: u64,
    truncated: bool,
    exited: bool,
    exit_code: Option<u32>,
}
struct Output {
    bytes: VecDeque<u8>,
    start: u64,
    eof: bool,
    changed: tokio::sync::watch::Sender<()>,
}
impl Default for Output {
    fn default() -> Self {
        Self { bytes: VecDeque::new(), start: 0, eof: false, changed: tokio::sync::watch::channel(()).0 }
    }
}
impl Output {
    fn append(&mut self, bytes: &[u8]) {
        self.bytes.extend(bytes);
        let excess = self.bytes.len().saturating_sub(CAPACITY);
        self.bytes.drain(..excess);
        self.start += excess as u64;
        self.changed.send_replace(());
    }
    fn read(&self, after: u64) -> TerminalResponse {
        let end = self.start + self.bytes.len() as u64;
        let cursor = after.clamp(self.start, end);
        let data: Vec<_> = self.bytes.iter().skip((cursor - self.start) as usize).take(PAGE).copied().collect();
        TerminalResponse { next_cursor: cursor + data.len() as u64, truncated: after < self.start || after > end, data, ..Default::default() }
    }
}

/// Raw bounded page for protocol adapters. The cursor always counts PTY bytes.
pub struct TerminalStreamPage {
    pub data: Vec<u8>,
    pub next_cursor: u64,
    pub truncated: bool,
    pub exited: bool,
    pub exit_code: Option<u32>,
    pub eof: bool,
}
struct Session {
    id: String,
    folder: PathBuf,
    shell: String,
    identity: Option<(u32, u64)>,
    child: Box<dyn Child + Send + Sync>,
    input: std::sync::mpsc::SyncSender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    output: Arc<Mutex<Output>>,
    exit: Option<u32>,
}
impl Session {
    fn info(&mut self) -> Result<TerminalInfo> {
        if let Some(status) = self.child.try_wait()? { self.exit = Some(status.exit_code()); }
        Ok(TerminalInfo { id: self.id.clone(), folder: display_path(&self.folder), shell: self.shell.clone(), pid: self.child.process_id(), exited: self.exit.is_some(), exit_code: self.exit })
    }
    fn stop(&mut self) -> Result<()> {
        let tree = self.identity.map(crate::runtime::process::kill_tree).transpose();
        if self.child.try_wait()?.is_none() { self.child.kill()?; }
        self.child.wait()?;
        tree.map(|_| ())
    }
}
impl Drop for Session {
    fn drop(&mut self) { let _ = self.stop(); }
}
#[derive(Default)]
pub(super) struct TerminalRegistry { sessions: Vec<Session> }
impl TerminalRegistry {
    pub(super) fn clear(&mut self) { self.sessions.clear(); }
}
fn pty_error(err: impl std::fmt::Display) -> WorkspaceError { error("io_error", format!("终端操作失败：{err}")) }
fn size(cols: u16, rows: u16) -> Result<PtySize> {
    if !(2..=500).contains(&cols) || !(1..=300).contains(&rows) { return Err(error("invalid_query", "终端尺寸超出范围")); }
    Ok(PtySize { cols, rows, pixel_width: 0, pixel_height: 0 })
}

impl WorkspaceService {
    /// Subscribe before reading to avoid losing a wakeup between read and wait.
    /// Uses the same authorization and membership checks as HTTP/MCP operations.
    pub fn terminal_subscribe(&self, revision: u64, id: String, authenticated: bool) -> Result<tokio::sync::watch::Receiver<()>> {
        self.terminal_execute_inner(TerminalRequest { expected_revision: revision, operation: TerminalOperation::Read { id: id.clone(), after: 0 } }, authenticated, false)?;
        let state = self.lock()?;
        if state.revision != revision { return Err(error("conflict", "工作区已改变，请刷新终端")); }
        let registry = self.terminals.lock().map_err(|_| error("unavailable", "终端服务暂不可用"))?;
        let session = registry.sessions.iter().find(|session| session.id == id).ok_or_else(|| error("not_found", "终端已关闭"))?;
        let output = session.output.lock().map_err(|_| error("unavailable", "终端输出暂不可用"))?;
        Ok(output.changed.subscribe())
    }

    /// A subscription is authorized on attach; each page checks revision and live
    /// registry membership under the workspace lock. Workspace mutations reconcile
    /// this registry under that same lock. No filesystem work on the output path.
    pub fn terminal_stream_read(&self, revision: u64, id: &str, after: u64, authenticated: bool) -> Result<TerminalStreamPage> {
        if !authenticated { return Err(error("unauthorized", "终端需要本机管理令牌")); }
        let state = self.lock()?;
        if state.revision != revision { return Err(error("conflict", "工作区已改变，请刷新终端")); }
        let mut registry = self.terminals.lock().map_err(|_| error("unavailable", "终端服务暂不可用"))?;
        let session = registry.sessions.iter_mut().find(|session| session.id == id).ok_or_else(|| error("not_found", "终端已关闭"))?;
        let (page, eof) = {
            let output = session.output.lock().map_err(|_| error("unavailable", "终端输出暂不可用"))?;
            (output.read(after), output.eof)
        };
        let info = session.info()?;
        Ok(TerminalStreamPage { data: page.data, next_cursor: page.next_cursor, truncated: page.truncated, exited: info.exited, exit_code: info.exit_code, eof })
    }

    /// WebSocket mutations keep the shared core checks without collecting every
    /// session's process status on each keystroke.
    pub fn terminal_stream_execute(&self, request: TerminalRequest, authenticated: bool) -> Result<()> {
        if !matches!(&request.operation, TerminalOperation::Input { .. } | TerminalOperation::Resize { .. }) {
            return Err(error("invalid_query", "实时终端仅接受输入或尺寸更新"));
        }
        self.terminal_execute_inner(request, authenticated, false).map(|_| ())
    }
    /// Explicit shutdown also covers an engine retained by the read-only listener.
    pub fn close_terminals(&self) -> Result<()> {
        self.terminals.lock().map_err(|_| error("unavailable", "终端服务暂不可用"))?.clear();
        Ok(())
    }
    fn terminal_folders(&self, state: &State) -> Result<Vec<PathBuf>> {
        let Some(session) = &state.session else { return Ok(Vec::new()); };
        let document = parse_document(&session.source)?;
        Ok(folder_entries(&document)?.iter().filter_map(|entry| resolve_folder(entry, &session.base).ok().and_then(|p| self.authorize(&p).ok())).filter(|p| p.is_dir()).collect())
    }
    pub(super) fn reconcile_terminals(&self, state: &State, reset: bool) -> Result<()> {
        let mut registry = self.terminals.lock().map_err(|_| error("unavailable", "终端服务暂不可用"))?;
        if reset { registry.clear(); }
        else {
            let folders = self.terminal_folders(state)?;
            registry.sessions.retain(|session| folders.contains(&session.folder));
        }
        Ok(())
    }
    pub fn terminal_execute(&self, request: TerminalRequest, authenticated: bool) -> Result<TerminalResponse> {
        self.terminal_execute_inner(request, authenticated, true)
    }
    fn terminal_execute_inner(&self, request: TerminalRequest, authenticated: bool, include_sessions: bool) -> Result<TerminalResponse> {
        if !authenticated { return Err(error("unauthorized", "终端需要本机管理令牌")); }
        // Keep the workspace lock through the operation to prevent a switch/input race.
        let state = self.lock()?;
        if state.revision != request.expected_revision { return Err(error("conflict", "工作区已改变，请刷新终端")); }
        // The registry is reconciled by every workspace mutation under this
        // same state lock. Streaming mutations only need to re-authorize their
        // own canonical directory, not reparse and stat all workspace folders.
        let streaming_mutation = !include_sessions && matches!(&request.operation, TerminalOperation::Input { .. } | TerminalOperation::Resize { .. });
        let folders = if streaming_mutation { None } else { Some(self.terminal_folders(&state)?) };
        let mut registry = self.terminals.lock().map_err(|_| error("unavailable", "终端服务暂不可用"))?;
        if let Some(folders) = &folders { registry.sessions.retain(|session| folders.contains(&session.folder)); }
        let mut response = TerminalResponse::default();
        match request.operation {
            TerminalOperation::List => {},
            TerminalOperation::Create { folder, cols, rows } => {
                let dimensions = size(cols, rows)?;
                if registry.sessions.len() >= 12 { return Err(error("conflict", "最多打开 12 个终端，请先关闭不用的终端")); }
                let path = self.authorize(Path::new(&folder))?;
                if !folders.as_ref().is_some_and(|folders| folders.contains(&path)) { return Err(error("outside_workspace", "只能从当前工作区的文件夹创建终端")); }
                let pair = native_pty_system().openpty(dimensions).map_err(pty_error)?;
                let mut reader = pair.master.try_clone_reader().map_err(pty_error)?;
                let mut writer = pair.master.take_writer().map_err(pty_error)?;
                // Only a server-selected interactive shell; no executable/args from clients.
                #[cfg(windows)]
                let shell = {
                    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
                    PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe")
                };
                #[cfg(not(windows))]
                let shell = PathBuf::from(std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into()));
                let mut command = CommandBuilder::new(&shell);
                #[cfg(windows)]
                command.args(["-NoLogo", "-NoExit", "-Command", "[Console]::InputEncoding = [Console]::OutputEncoding = $OutputEncoding = [System.Text.UTF8Encoding]::new($false)"]);
                #[cfg(not(windows))]
                command.arg("-i");
                // Keep canonical paths for authorization/session identity, but
                // pass a normal drive/UNC path to the interactive shell. Windows
                // PowerShell otherwise exposes its provider-qualified \\?\ path
                // in the prompt (Microsoft.PowerShell.Core\FileSystem::...).
                #[cfg(windows)]
                command.cwd(PathBuf::from(display_path(&path)));
                #[cfg(not(windows))]
                command.cwd(&path);
                command.env("TERM", "xterm-256color");
                command.env("COLORTERM", "truecolor");
                command.env("TERM_PROGRAM", "DevFlow");
                let child = pair.slave.spawn_command(command).map_err(pty_error)?;
                drop(pair.slave);
                let id = uuid::Uuid::new_v4().to_string();
                let output = Arc::new(Mutex::new(Output::default()));
                let (input, receive) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
                let session = Session { id: id.clone(), folder: path, shell: shell.file_stem().unwrap_or_default().to_string_lossy().into_owned(), identity: child.process_id().and_then(crate::runtime::process::identity), child, input, master: pair.master, output: output.clone(), exit: None };
                let input_output = output.clone();
                thread::Builder::new().name("workspace-terminal-input".into()).spawn(move || {
                    while let Ok(data) = receive.recv() {
                        if let Err(err) = writer.write_all(&data).and_then(|_| writer.flush()) {
                            if let Ok(mut output) = input_output.lock() { output.append(format!("\r\n终端输入失败：{err}\r\n").as_bytes()); }
                            break;
                        }
                    }
                })?;
                // Raw bytes preserve ANSI and UTF-8 characters split across PTY reads.
                thread::Builder::new().name("workspace-terminal-output".into()).spawn(move || {
                    let mut bytes = [0u8; 8192];
                    loop {
                        match reader.read(&mut bytes) {
                            Ok(0) => break,
                            Ok(n) => { if let Ok(mut output) = output.lock() { output.append(&bytes[..n]); } else { break; } },
                            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(_) => break,
                        }
                    }
                    if let Ok(mut output) = output.lock() { output.eof = true; output.changed.send_replace(()); }
                })?;
                response.id = Some(id);
                registry.sessions.push(session);
            },
            operation => {
                let id = match &operation {
                    TerminalOperation::Read { id, .. } | TerminalOperation::Input { id, .. } | TerminalOperation::Resize { id, .. } | TerminalOperation::Close { id } => id,
                    _ => unreachable!(),
                };
                let index = registry.sessions.iter().position(|session| &session.id == id).ok_or_else(|| error("not_found", "终端已关闭"))?;
                let session = &mut registry.sessions[index];
                if streaming_mutation && (self.authorize(&session.folder)? != session.folder || !session.folder.is_dir()) {
                    return Err(error("outside_workspace", "终端工作目录已失效"));
                }
                match operation {
                    TerminalOperation::Read { after, .. } => {
                        response = session.output.lock().map_err(|_| error("unavailable", "终端输出暂不可用"))?.read(after);
                        let info = session.info()?;
                        response.exited = info.exited;
                        response.exit_code = info.exit_code;
                    },
                    TerminalOperation::Input { data, .. } => {
                        if data.len() > 64 * 1024 { return Err(error("invalid_query", "单次终端输入不能超过 64 KiB")); }
                        if session.info()?.exited { return Err(error("conflict", "终端进程已退出")); }
                        session.input.try_send(data.into_bytes()).map_err(|_| error("conflict", "终端输入队列已满或关闭，请稍后重试"))?;
                    },
                    TerminalOperation::Resize { cols, rows, .. } => session.master.resize(size(cols, rows)?).map_err(pty_error)?,
                    TerminalOperation::Close { .. } => { session.stop()?; registry.sessions.remove(index); },
                    _ => unreachable!(),
                }
            },
        }
        if include_sessions { response.sessions = registry.sessions.iter_mut().map(Session::info).collect::<Result<_>>()?; }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stream_notifications_survive_read_wait_races_and_coalesce_without_copying_output() {
        let mut output = Output::default();
        let mut changes = output.changed.subscribe();
        assert!(output.read(0).data.is_empty());
        // Data arrives after the consumer's empty read but before it waits.
        output.append("中".as_bytes());
        output.append("文\x1b[0m".as_bytes());
        tokio::time::timeout(Duration::from_secs(1), changes.changed()).await.unwrap().unwrap();
        let page = output.read(0);
        assert_eq!(page.data, "中文\x1b[0m".as_bytes());
        assert!(!changes.has_changed().unwrap());
        output.eof = true;
        output.changed.send_replace(());
        tokio::time::timeout(Duration::from_secs(1), changes.changed()).await.unwrap().unwrap();
        assert!(output.read(page.next_cursor).data.is_empty());
        assert!(output.eof);
    }
    #[test]
    fn raw_output_preserves_split_chinese_and_ansi() {
        let mut output = Output::default();
        let data = "\u{1b}[31m中文\u{1b}[0m".as_bytes();
        output.append(&data[..7]); output.append(&data[7..]);
        assert_eq!(output.read(0).data, data);
        assert!(!output.read(0).truncated);
    }
    #[test]
    fn output_is_bounded_and_cursors_page_without_duplicates() {
        let mut output = Output::default();
        output.append(&vec![b'x'; CAPACITY + 10]);
        let first = output.read(0);
        assert!(first.truncated); assert_eq!(first.data.len(), PAGE);
        assert_eq!(first.next_cursor, 10 + PAGE as u64);
        assert!(!output.read(first.next_cursor).truncated);
        assert_eq!(output.bytes.len(), CAPACITY);
    }
    #[test]
    fn requires_auth_revision_and_exact_workspace_folder() {
        let root = std::env::temp_dir().join(format!("devflow-terminal-中文-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).unwrap();
        let workspace = WorkspaceService::new(vec![root.clone()], Some(root.clone())).unwrap();
        let request = || TerminalRequest { expected_revision: 0, operation: TerminalOperation::List };
        assert_eq!(workspace.terminal_execute(request(), false).err().unwrap().code, "unauthorized");
        assert_eq!(workspace.terminal_subscribe(0, "missing".into(), false).err().unwrap().code, "unauthorized");
        assert_eq!(workspace.terminal_stream_read(0, "missing", 0, false).err().unwrap().code, "unauthorized");
        assert_eq!(workspace.terminal_stream_read(99, "missing", 0, true).err().unwrap().code, "conflict");
        assert_eq!(workspace.terminal_stream_read(0, "missing", 0, true).err().unwrap().code, "not_found");
        assert_eq!(workspace.terminal_stream_execute(request(), true).err().unwrap().code, "invalid_query");
        assert_eq!(workspace.terminal_execute(TerminalRequest { expected_revision: 99, operation: TerminalOperation::List }, true).err().unwrap().code, "conflict");
        let request = TerminalRequest { expected_revision: 0, operation: TerminalOperation::Create { folder: display_path(&root.join("nested")), cols: 80, rows: 24 } };
        assert_eq!(workspace.terminal_execute(request, true).err().unwrap().code, "outside_workspace");
        assert!(size(0, 24).is_err()); assert!(size(80, 301).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_terminal_uses_chinese_directory_resizes_and_cleans_up_on_workspace_close() {
        let root = std::env::temp_dir().join(format!("devflow-terminal-中文 空格-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let workspace = WorkspaceService::new(vec![root.clone()], Some(root.clone())).unwrap();
        let execute = |operation| workspace.terminal_execute(TerminalRequest { expected_revision: 0, operation }, true).unwrap();
        let created = execute(TerminalOperation::Create { folder: display_path(&root), cols: 80, rows: 24 });
        let id = created.id.unwrap();
        let changes = workspace.terminal_subscribe(0, id.clone(), true).unwrap();
        let identity = workspace.terminals.lock().unwrap().sessions[0].identity;
        assert_eq!(workspace.terminals.lock().unwrap().sessions[0].folder, root.canonicalize().unwrap());
        execute(TerminalOperation::Resize { id: id.clone(), cols: 100, rows: 30 });
        assert_eq!(workspace.terminals.lock().unwrap().sessions[0].master.get_size().unwrap().cols, 100);
        #[cfg(windows)]
        let command = "echo ('DEVFLOW_TERMINAL_' + 'READY')\r";
        #[cfg(not(windows))]
        let command = "printf '%s%s\\n' DEVFLOW_TERMINAL_ READY\r";
        workspace.terminal_stream_execute(TerminalRequest { expected_revision: 0, operation: TerminalOperation::Input { id: id.clone(), data: command.into() } }, true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut bytes = Vec::new();
        let mut cursor = 0;
        while Instant::now() < deadline {
            let response = execute(TerminalOperation::Read { id: id.clone(), after: cursor });
            cursor = response.next_cursor;
            bytes.extend(response.data);
            if String::from_utf8_lossy(&bytes).contains("DEVFLOW_TERMINAL_READY") { break; }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(String::from_utf8_lossy(&bytes).contains("DEVFLOW_TERMINAL_READY"));
        assert!(changes.has_changed().unwrap());
        let replay = workspace.terminal_stream_read(0, &id, 0, true).unwrap();
        assert!(!replay.truncated);
        assert!(String::from_utf8_lossy(&replay.data).contains("DEVFLOW_TERMINAL_READY"));
        #[cfg(windows)]
        let command = "$devflowChild = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32/ping.exe') -ArgumentList @('-t', '127.0.0.1') -WindowStyle Hidden -PassThru; Write-Output ('DEVFLOW_CHILD_' + $devflowChild.Id)\r";
        #[cfg(not(windows))]
        let command = "sleep 120 & printf 'DEVFLOW_CHILD_%s\\n' \"$!\"\r";
        execute(TerminalOperation::Input { id: id.clone(), data: command.into() });
        let marker = regex::Regex::new(r"DEVFLOW_CHILD_(\d+)").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut child_identity = None;
        while Instant::now() < deadline {
            let response = execute(TerminalOperation::Read { id: id.clone(), after: cursor });
            cursor = response.next_cursor;
            bytes.extend(response.data);
            if let Some(found) = marker.captures(&String::from_utf8_lossy(&bytes)) {
                child_identity = crate::runtime::process::identity(found[1].parse().unwrap());
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let child_identity = child_identity.expect("shell should report a live child process");
        workspace.execute(WorkspaceRequest { expected_revision: 0, discard_changes: true, operation: WorkspaceOperation::Close }, true).unwrap();
        assert!(workspace.terminals.lock().unwrap().sessions.is_empty());
        assert_eq!(workspace.terminal_stream_read(0, &id, cursor, true).err().unwrap().code, "conflict");
        if let Some(identity) = identity { assert_ne!(crate::runtime::process::identity(identity.0), Some(identity)); }
        let deadline = Instant::now() + Duration::from_secs(3);
        while crate::runtime::process::identity(child_identity.0) == Some(child_identity) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        assert_ne!(crate::runtime::process::identity(child_identity.0), Some(child_identity));
        fs::remove_dir_all(root).unwrap();
    }
}
