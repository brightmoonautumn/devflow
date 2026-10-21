$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.dll,System.Core.dll,System.Windows.Forms.dll,System.Drawing.dll -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows.Forms;

public static class DevFlowPickerWindow {
    private delegate bool EnumWindow(IntPtr window, IntPtr parameter);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] private static extern bool EnumThreadWindows(uint thread, EnumWindow callback, IntPtr parameter);
    [DllImport("user32.dll")] private static extern IntPtr GetAncestor(IntPtr window, uint flags);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder name, int length);
    [DllImport("user32.dll")] private static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr window);

    public static DialogResult Show(CommonDialog dialog, int backendPid) {
        using (var backend = Process.GetProcessById(backendPid))
        using (var owner = new Form())
        using (var timer = new Timer()) {
            // Pin this exact backend instance before starting the modal message loop.
            if (backend.Handle == IntPtr.Zero) { throw new InvalidOperationException("Backend process handle is unavailable."); }
            owner.Text = "DevFlow — 选择本机位置";
            owner.ShowInTaskbar = false;
            owner.StartPosition = FormStartPosition.Manual;
            owner.FormBorderStyle = FormBorderStyle.None;
            var area = Screen.FromPoint(Cursor.Position).WorkingArea;
            owner.Bounds = new Rectangle(area.Left + area.Width / 2, area.Top + area.Height / 2, 1, 1);
            // Create only the owner handle. Never show an auxiliary window behind
            // the chooser; the actual native dialog receives its own z-order fix.
            IntPtr ownerHandle = owner.Handle;

            uint thread = GetCurrentThreadId();
            var promoted = new HashSet<IntPtr>();
            timer.Interval = 100;
            timer.Tick += delegate {
                if (backend.HasExited) { Environment.Exit(0); }
                // ShowDialog runs a nested message loop. Wait until its actual native
                // window exists; making an invisible surrogate topmost is insufficient.
                EnumThreadWindows(thread, delegate(IntPtr window, IntPtr parameter) {
                    if (!IsWindowVisible(window) || GetAncestor(window, 3) != ownerHandle) { return true; }
                    var name = new StringBuilder(64);
                    GetClassName(window, name, name.Capacity);
                    if (name.ToString() != "#32770" || promoted.Contains(window)) { return true; }
                    // Keep only our modal chooser above ordinary browser windows until
                    // it closes. NOACTIVATE avoids repeatedly taking keyboard focus.
                    if (SetWindowPos(window, new IntPtr(-1), 0, 0, 0, 0, 0x0053)) {
                        promoted.Add(window);
                        // A best-effort, one-time request: Windows may reject activation,
                        // but the actual chooser remains visible above ordinary windows.
                        SetForegroundWindow(window);
                    }
                    return true;
                }, IntPtr.Zero);
            };

            timer.Start();
            try {
                return dialog.ShowDialog(owner);
            } finally {
                timer.Stop();
                foreach (var window in promoted) {
                    if (GetAncestor(window, 3) == ownerHandle) {
                        SetWindowPos(window, new IntPtr(-2), 0, 0, 0, 0, 0x0013);
                    }
                }
            }
        }
    }
}
'@

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
    $result = [DevFlowPickerWindow]::Show($dialog, [int]$env:DEVFLOW_PICK_PARENT)
    if ($result -eq [System.Windows.Forms.DialogResult]::OK) {
        if ($env:DEVFLOW_PICK_KIND -eq 'workspace_file') { $path = $dialog.FileName } else { $path = $dialog.SelectedPath }
        ConvertTo-Json -Compress -InputObject $path
    } else { 'null' }
} finally { $dialog.Dispose() }
