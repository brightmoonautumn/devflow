use super::*;
use std::process::{Child, Command, Stdio};

const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}
pub(super) struct Output { pub code: i32, pub stdout: Vec<u8>, pub stderr: String }
impl Output {
    pub fn checked(self) -> Result<Vec<u8>> {
        if self.code == 0 { Ok(self.stdout) }
        else { Err(error("git_failed", format!("Git 操作失败（{}）：{}", self.code, self.stderr.trim()))) }
    }
}

pub(super) fn executable() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DEVFLOW_GIT_PATH") {
        let path = PathBuf::from(path);
        if path.is_absolute() && path.is_file() { return Ok(path); }
        return Err(error("git_unavailable", "DEVFLOW_GIT_PATH 必须指向已有 Git 可执行文件的绝对路径"));
    }
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths).filter(|p| p.is_absolute()) {
            let candidate = directory.join(name);
            if candidate.is_file() { return Ok(candidate); }
        }
    }
    #[cfg(windows)]
    for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
        if let Some(base) = std::env::var_os(variable) {
            for relative in ["Git/cmd/git.exe", "Programs/Git/cmd/git.exe"] {
                let candidate = PathBuf::from(&base).join(relative);
                if candidate.is_absolute() && candidate.is_file() { return Ok(candidate); }
            }
        }
    }
    Err(error("git_unavailable", "未找到 Git，请安装 Git for Windows；已有安装会自动检测，无需修改项目配置"))
}

// All arguments originate from fixed core operations. User paths are literal
// pathspecs, never command fragments. Read, write and hooks have bounded output.
pub(super) fn run(root: &Path, args: &[&str], input: Option<Vec<u8>>, commit: bool) -> Result<Output> {
    let mut command = Command::new(executable()?);
    command.current_dir(root).args(["--no-pager", "-c", "color.ui=false", "-c", "core.quotePath=false", "-c", "core.fsmonitor=false"])
        .args(args).env("GIT_TERMINAL_PROMPT", "0").env("GIT_EDITOR", "true").env("GIT_OPTIONAL_LOCKS", if commit { "1" } else { "0" })
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    for key in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_COMMON_DIR", "GIT_NAMESPACE", "GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS", "GIT_LITERAL_PATHSPECS", "GIT_GLOB_PATHSPECS", "GIT_NOGLOB_PATHSPECS", "GIT_ICASE_PATHSPECS"] { command.env_remove(key); }
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut process = Process(command.spawn().map_err(|e| error("git_unavailable", format!("无法启动 Git：{e}")))?);
    let identity = crate::runtime::process::identity(process.0.id());
    let stdout = process.0.stdout.take().ok_or_else(|| error("io_error", "无法读取 Git 输出"))?;
    let stderr = process.0.stderr.take().ok_or_else(|| error("io_error", "无法读取 Git 错误"))?;
    let output = std::thread::spawn(move || { let mut bytes = Vec::new(); stdout.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes).map(|_| bytes) });
    let errors = std::thread::spawn(move || { let mut bytes = Vec::new(); stderr.take(128 * 1024).read_to_end(&mut bytes).map(|_| bytes) });
    let writer = input.and_then(|bytes| process.0.stdin.take().map(|mut stdin| std::thread::spawn(move || stdin.write_all(&bytes))));
    let started = Instant::now();
    let timeout = Duration::from_secs(if commit { 120 } else { 15 });
    let mut completed = None;
    let status = loop {
        if completed.is_none() { completed = process.0.try_wait()?; }
        if completed.is_some() && output.is_finished() && errors.is_finished() && writer.as_ref().is_none_or(|writer| writer.is_finished()) { break completed; }
        if started.elapsed() >= timeout {
            if let Some(identity) = identity { let _ = crate::runtime::process::kill_tree(identity); }
            let _ = process.0.kill(); let _ = process.0.wait(); break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Do not block a core worker forever if an unusual hook leaves descendants
    // holding inherited pipes after the Git parent has already exited.
    let Some(status) = status else { return Err(error("git_timeout", "Git 操作或 hook 输出超时，已清理仍属于该 Git 进程的进程树；请刷新状态确认是否已产生提交")); };
    if let Some(writer) = writer { let _ = writer.join(); }
    let bytes = output.join().map_err(|_| error("io_error", "Git 输出读取失败"))??;
    let stderr = errors.join().map_err(|_| error("io_error", "Git 错误读取失败"))??;
    if bytes.len() as u64 > OUTPUT_LIMIT { return Err(error("limit_exceeded", "Git 输出超过 16 MiB，请缩小工作区范围")); }
    Ok(Output { code: status.code().unwrap_or(-1), stdout: bytes, stderr: String::from_utf8_lossy(&stderr).into_owned() })
}

pub(super) fn checked(root: &Path, args: &[&str]) -> Result<Vec<u8>> { run(root, args, None, false)?.checked() }
pub(super) fn text(bytes: &[u8]) -> Result<String> { String::from_utf8(bytes.to_vec()).map_err(|_| error("unsupported_file", "Git 路径或内容不是 UTF-8")) }
pub(super) fn literal(path: &str) -> String { format!(":(literal){path}") }
pub(super) fn pathspecs(paths: &[String]) -> Vec<u8> { paths.iter().flat_map(|path| literal(path).into_bytes().into_iter().chain(std::iter::once(0))).collect() }
