//! Fixed native dialogs: clients choose only the dialog kind, never shell input.
use super::{WorkspaceError, WorkspacePickKind};
use std::sync::{Arc, Mutex};

type Result<T> = std::result::Result<T, WorkspaceError>;

#[derive(Default)]
pub(super) struct PickerRegistry {
    active: Mutex<Option<Arc<Mutex<PickerState>>>>,
}

struct PickerState {
    request_id: String,
    cancelled: bool,
    child: Option<std::process::Child>,
}

pub(super) struct PickerSession<'a> {
    registry: &'a PickerRegistry,
    state: Arc<Mutex<PickerState>>,
}

fn conflict(message: &str) -> WorkspaceError {
    WorkspaceError { code: "conflict", message: message.into() }
}

impl PickerRegistry {
    pub(super) fn start(&self, request_id: Option<String>) -> Result<PickerSession<'_>> {
        let mut active = self.active.lock().map_err(|_| failure("选择窗口状态不可用"))?;
        if active.is_some() {
            return Err(conflict("已有一个系统选择窗口，请先完成选择"));
        }
        let state = Arc::new(Mutex::new(PickerState {
            request_id: request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            cancelled: false,
            child: None,
        }));
        *active = Some(state.clone());
        Ok(PickerSession { registry: self, state })
    }

    pub(super) fn cancel(&self, request_id: &str) -> Result<()> {
        let active = self.active.lock().map_err(|_| failure("选择窗口状态不可用"))?;
        let state = active.as_ref().ok_or_else(|| conflict("选择请求已结束"))?;
        let mut state = state.lock().map_err(|_| failure("选择窗口状态不可用"))?;
        if state.request_id != request_id {
            return Err(conflict("选择请求不匹配"));
        }
        state.cancelled = true;
        stop_child(&mut state)?;
        Ok(())
    }
}

fn stop_child(state: &mut PickerState) -> Result<()> {
    if let Some(child) = state.child.as_mut() {
        if child.try_wait()?.is_none() {
            if let Err(error) = child.kill() {
                if child.try_wait()?.is_none() { return Err(error.into()); }
            }
        }
        child.wait()?;
    }
    state.child = None;
    Ok(())
}

impl PickerSession<'_> {
    pub(super) fn pick(&self, kind: WorkspacePickKind) -> Result<Option<String>> {
        pick_dialog(self, kind)
    }

    pub(super) fn finish<T>(
        &self,
        accept: impl FnOnce() -> Result<T>,
        cancelled: impl FnOnce() -> T,
    ) -> Result<T> {
        // Keep cancellation and granting access mutually exclusive.
        let state = self.state.lock().map_err(|_| failure("选择窗口状态不可用"))?;
        if state.cancelled { Ok(cancelled()) } else { accept() }
    }
}

impl Drop for PickerSession<'_> {
    fn drop(&mut self) {
        // Match cancel's lock order and reap the exact owned child, never a PID lookup.
        let mut active = self.registry.active.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let _ = stop_child(&mut state);
        if active.as_ref().is_some_and(|current| Arc::ptr_eq(current, &self.state)) {
            *active = None;
        }
    }
}

#[cfg(windows)]
fn pick_dialog(session: &PickerSession<'_>, kind: WorkspacePickKind) -> Result<Option<String>> {
    use std::{io::Read, os::windows::process::CommandExt, process::{Command, Stdio}, time::{Duration, Instant}};
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Add-Type -AssemblyName System.Windows.Forms
$owner = New-Object System.Windows.Forms.Form
$owner.TopMost = $true
$owner.ShowInTaskbar = $false
$owner.Opacity = 0
$owner.Show()
try {
  if ($env:DEVFLOW_PICK_KIND -eq 'workspace_file') {
    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = '选择工作区文件'
    $dialog.Filter = 'VS Code 工作区 (*.code-workspace)|*.code-workspace'
    $dialog.CheckFileExists = $true
    $dialog.Multiselect = $false
    $dialog.DereferenceLinks = $true
  } else {
    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.Description = '选择本机文件夹'
    $dialog.RootFolder = [System.Environment+SpecialFolder]::MyComputer
    $dialog.ShowNewFolderButton = $true
  }
  try {
    if ($dialog.ShowDialog($owner) -eq [System.Windows.Forms.DialogResult]::OK) {
      if ($env:DEVFLOW_PICK_KIND -eq 'workspace_file') { $path = $dialog.FileName } else { $path = $dialog.SelectedPath }
      ConvertTo-Json -Compress -InputObject $path
    } else { 'null' }
  } finally { $dialog.Dispose() }
} finally { $owner.Dispose() }
"#;
    // Rust passes Windows process arguments as UTF-16, preserving the dialog labels.
    let mut state = session.state.lock().map_err(|_| failure("选择窗口状态不可用"))?;
    if state.cancelled { return Ok(None); }
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-WindowStyle", "Hidden", "-Command", SCRIPT])
        .env("DEVFLOW_PICK_KIND", match kind { WorkspacePickKind::WorkspaceFile => "workspace_file", WorkspacePickKind::Folder => "folder" })
        .creation_flags(0x08000000)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    // Drain concurrently: a long selected path must not fill the pipe and block exit.
    let stdout = child.stdout.take();
    state.child = Some(child);
    drop(state);
    let reader = std::thread::spawn(move || -> std::io::Result<String> {
        let mut output = String::new();
        if let Some(stdout) = stdout { stdout.take(131_073).read_to_string(&mut output)?; }
        Ok(output)
    });
    let started = Instant::now();
    loop {
        let status = {
            let mut state = session.state.lock().map_err(|_| failure("选择窗口状态不可用"))?;
            if state.cancelled { return Ok(None); }
            state.child.as_mut().ok_or_else(|| failure("选择窗口进程不存在"))?.try_wait()?
        };
        if let Some(status) = status {
            if !status.success() { return Err(failure("无法打开系统选择窗口，请确认 DevFlow 运行在本机桌面会话中")); }
            let output = reader.join().map_err(|_| failure("无法读取系统选择结果"))??;
            if output.len() > 131_072 { return Err(failure("选择路径过长")); }
            return serde_json::from_str(output.trim_start_matches('\u{feff}').trim())
                .map_err(|_| failure("无法读取系统选择结果"));
        }
        if started.elapsed() > Duration::from_secs(180) {
            let mut state = session.state.lock().map_err(|_| failure("选择窗口状态不可用"))?;
            stop_child(&mut state)?;
            return Err(failure("选择窗口已超时，请重新选择"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn failure(message: &str) -> WorkspaceError {
    WorkspaceError { code: "unavailable", message: message.into() }
}

#[cfg(not(windows))]
fn pick_dialog(session: &PickerSession<'_>, _: WorkspacePickKind) -> Result<Option<String>> {
    if session.state.lock().map_err(|_| failure("选择窗口状态不可用"))?.cancelled { return Ok(None); }
    Err(failure("当前系统不支持原生选择窗口，请使用下方的位置浏览器"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_is_exclusive_and_drop_releases_the_slot() {
        let registry = PickerRegistry::default();
        let session = registry.start(Some("first".into())).unwrap();
        assert!(matches!(registry.start(None), Err(error) if error.code == "conflict"));
        assert_eq!(registry.cancel("other").unwrap_err().code, "conflict");
        assert!(!session.state.lock().unwrap().cancelled);
        drop(session);
        assert!(registry.start(None).is_ok());
    }

    #[test]
    fn cancellation_prevents_selection_acceptance() {
        let registry = PickerRegistry::default();
        let session = registry.start(Some("request".into())).unwrap();
        registry.cancel("request").unwrap();
        let result = session.finish(|| panic!("cancelled selection must not grant access"), || 7).unwrap();
        assert_eq!(result, 7);
        drop(session);
        assert_eq!(registry.cancel("request").unwrap_err().code, "conflict");
    }

    #[cfg(windows)]
    #[test]
    fn cancellation_reaps_the_owned_dialog_process() {
        use std::{os::windows::process::CommandExt, process::{Command, Stdio}};
        let registry = PickerRegistry::default();
        let session = registry.start(Some("child".into())).unwrap();
        // A harmless stand-in exercises process cleanup without opening a dialog.
        let child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 60"])
            .creation_flags(0x08000000)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().unwrap();
        session.state.lock().unwrap().child = Some(child);
        registry.cancel("child").unwrap();
        assert!(session.state.lock().unwrap().child.is_none());
    }
}
