Option Explicit
Dim shell, files, root, script, command
Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
root = files.GetParentFolderName(WScript.ScriptFullName)
script = files.BuildPath(root, "scripts\launch.ps1")
If Not files.FileExists(script) Then
    MsgBox "Missing scripts\launch.ps1. Please extract the complete DevFlow package.", vbCritical, "DevFlow"
    WScript.Quit 1
End If
command = """" & shell.ExpandEnvironmentStrings("%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe") & """ -NoProfile -STA -ExecutionPolicy Bypass -File """ & script & """"
shell.Run command, 0, False
