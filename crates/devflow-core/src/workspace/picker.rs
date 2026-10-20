//! Fixed native dialogs: clients choose only the dialog kind, never shell input.
use super::{WorkspaceError, WorkspacePickKind};

#[cfg(windows)]
pub(super) fn pick(kind: WorkspacePickKind) -> Result<Option<String>, WorkspaceError> {
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
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-WindowStyle", "Hidden", "-Command", SCRIPT])
        .env("DEVFLOW_PICK_KIND", match kind { WorkspacePickKind::WorkspaceFile => "workspace_file", WorkspacePickKind::Folder => "folder" })
        .creation_flags(0x08000000)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    // Drain concurrently: a long selected path must not fill the pipe and block exit.
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || -> std::io::Result<String> {
        let mut output = String::new();
        if let Some(stdout) = stdout { stdout.take(131_073).read_to_string(&mut output)?; }
        Ok(output)
    });
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() { return Err(failure("无法打开系统选择窗口，请确认 DevFlow 运行在本机桌面会话中")); }
            let output = reader.join().map_err(|_| failure("无法读取系统选择结果"))??;
            if output.len() > 131_072 { return Err(failure("选择路径过长")); }
            return serde_json::from_str(output.trim_start_matches('\u{feff}').trim())
                .map_err(|_| failure("无法读取系统选择结果"));
        }
        if started.elapsed() > Duration::from_secs(180) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failure("选择窗口已超时，请重新选择"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn failure(message: &str) -> WorkspaceError {
    WorkspaceError { code: "unavailable", message: message.into() }
}

#[cfg(not(windows))]
pub(super) fn pick(_: WorkspacePickKind) -> Result<Option<String>, WorkspaceError> {
    Err(failure("当前系统不支持原生选择窗口，请使用下方的位置浏览器"))
}
