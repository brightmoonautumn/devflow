//! Java execution shared by HTTP/MCP. Arguments never pass through a shell.
use crate::workspace::{DetectedProject, ProjectKind, WorkspaceError};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::Read,
    path::{Path, PathBuf},
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
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JavaRequest {
    pub project_path: String,
    pub expected_revision: u64,
    pub operation: JavaOperation,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum JavaOperation {
    #[serde(rename_all = "camelCase")]
    Start {
        #[serde(default)]
        jvm_args: Vec<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        profiles: Vec<String>,
        main_class: Option<String>,
    },
    Stop {
        pid: Option<u32>,
    },
    Attach {
        pid: u32,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JavaQuery {
    pub project_path: String,
    #[serde(default)]
    pub after: u64,
    pub run_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaOptions {
    pub main_classes: Vec<String>,
    pub profiles: Vec<String>,
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
pub struct JavaOutput {
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
struct Run {
    id: String,
    state: String,
    pid: Option<(u32, u64)>,
    cancelled: bool,
    exit: Option<i32>,
    logs: VecDeque<LogLine>,
    sequence: u64,
    arthas_url: Option<String>,
    attaching: bool,
}
impl Run {
    fn new(state: &str) -> Self {
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
    fn log(&mut self, stream: &str, text: impl Into<String>) {
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
    fn output(&self, after: u64) -> JavaOutput {
        JavaOutput {
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
#[derive(Default)]
pub struct JavaService {
    runs: Mutex<HashMap<String, Arc<Mutex<Run>>>>,
}
fn key(project: &DetectedProject) -> String {
    project.path.to_lowercase()
}
fn java_project(project: &DetectedProject) -> Result<()> {
    if matches!(project.kind, ProjectKind::Node) {
        Err(error("unsupported", "当前仅支持 Java 服务运行"))
    } else {
        Ok(())
    }
}
pub fn options(project: &DetectedProject) -> Result<JavaOptions> {
    java_project(project)?;
    let root = Path::new(&project.path).canonicalize()?;
    let mut profiles = Vec::new();
    let resources = root.join("src/main/resources");
    if resources
        .canonicalize()
        .is_ok_and(|path| path.starts_with(&root))
    {
        if let Ok(entries) = fs::read_dir(resources) {
            for entry in entries.take(512).flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(name) = name.strip_prefix("application-") {
                    if let Some((profile, extension)) = name.rsplit_once('.') {
                        if ["yml", "yaml", "properties"].contains(&extension)
                            && valid_profile(profile)
                        {
                            profiles.push(profile.to_owned());
                        }
                    }
                }
            }
        }
    }
    profiles.sort();
    profiles.dedup();
    Ok(JavaOptions {
        main_classes: project.main_classes.clone(),
        profiles,
    })
}
fn valid_profile(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}
fn validate_args(jvm: &[String], args: &[String], profiles: &[String]) -> Result<()> {
    if jvm.len() + args.len() > 256
        || jvm
            .iter()
            .chain(args)
            .any(|arg| arg.contains('\0') || arg.len() > 8192)
        || jvm.iter().chain(args).map(String::len).sum::<usize>() > 24000
    {
        return Err(error("invalid_arguments", "参数过长或包含无效字符"));
    }
    if jvm.iter().any(|arg| {
        !arg.starts_with('-')
            || [
                "-jar",
                "-cp",
                "-classpath",
                "--class-path",
                "-m",
                "--module",
            ]
            .contains(&arg.as_str())
            || arg.starts_with("--class-path=")
            || arg.starts_with("--module=")
    }) {
        return Err(error(
            "invalid_arguments",
            "JVM 参数不能替换项目主类或 classpath；每行填写一个完整参数",
        ));
    }
    if profiles.len() > 16 || profiles.iter().any(|p| !valid_profile(p)) {
        return Err(error(
            "invalid_arguments",
            "Profile 仅支持字母、数字、点、横线和下划线",
        ));
    }
    Ok(())
}
pub fn java_executable() -> PathBuf {
    std::env::var_os("DEVFLOW_JAVA_HOME")
        .or_else(|| std::env::var_os("JAVA_HOME"))
        .map(|home| {
            PathBuf::from(home).join(if cfg!(windows) {
                "bin/java.exe"
            } else {
                "bin/java"
            })
        })
        .unwrap_or_else(|| PathBuf::from("java"))
}
fn java_command() -> Result<Command> {
    let mut command = Command::new(java_executable());
    command.arg("-Dfile.encoding=UTF-8");
    // Windows JDK NIO wakeup sockets need a native path, not an inherited
    // short-name TEMP path that may fail with Invalid argument: connect.
    #[cfg(windows)]
    {
        let sockets = std::env::var_os("DEVFLOW_JAVA_SOCKET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".devflow/sockets"));
        fs::create_dir_all(&sockets)?;
        command.arg(format!(
            "-Djdk.net.unixdomain.tmpdir={}",
            crate::workspace::display_path(&sockets.canonicalize()?)
        ));
    }
    Ok(command)
}
fn maven_command(root: &Path, classpath: &Path) -> Result<Command> {
    let home = std::env::var_os("MAVEN_HOME")
        .or_else(|| std::env::var_os("M2_HOME"))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .find(|path| {
                        path.join(if cfg!(windows) { "mvn.cmd" } else { "mvn" })
                            .is_file()
                    })
                    .and_then(|bin| bin.parent().map(Path::to_owned))
            })
        })
        .ok_or_else(|| {
            error(
                "maven_missing",
                "找不到 Maven，请配置 MAVEN_HOME 或将 Maven bin 加入 PATH",
            )
        })?;
    let boot = fs::read_dir(home.join("boot"))?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name().is_some_and(|n| {
                n.to_string_lossy().starts_with("plexus-classworlds-")
                    && n.to_string_lossy().ends_with(".jar")
            })
        })
        .ok_or_else(|| error("maven_missing", "找不到 Maven ClassWorlds 启动器"))?;
    let multi = root
        .ancestors()
        .find(|p| p.join(".mvn").is_dir())
        .unwrap_or(root);
    let mut command = java_command()?;
    command
        .arg(format!("-Dmaven.home={}", home.display()))
        .arg(format!(
            "-Dclassworlds.conf={}",
            home.join("bin/m2.conf").display()
        ))
        .arg(format!(
            "-Dmaven.multiModuleProjectDirectory={}",
            multi.display()
        ))
        .arg("-Dfile.encoding=UTF-8")
        .arg("-classpath")
        .arg(boot)
        .arg("org.codehaus.plexus.classworlds.launcher.Launcher")
        .args([
            "-B",
            "-Dstyle.color=never",
            "-DskipTests",
            "compile",
            "dependency:build-classpath",
            "-Dmdep.includeScope=runtime",
        ])
        .arg(format!("-Dmdep.outputFile={}", classpath.display()))
        .current_dir(root);
    Ok(command)
}
fn process_system() -> System {
    let mut s = System::new();
    s.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything().without_tasks(),
    );
    s
}
fn identity(pid: u32) -> Option<(u32, u64)> {
    process_system()
        .process(Pid::from_u32(pid))
        .map(|p| (pid, p.start_time()))
}
pub fn java_identity(pid: u32) -> Result<(u32, u64)> {
    let system = process_system();
    let process = system
        .process(Pid::from_u32(pid))
        .ok_or_else(|| error("conflict", "目标进程已退出"))?;
    if !["java", "java.exe", "javaw", "javaw.exe"]
        .contains(&process.name().to_string_lossy().to_lowercase().as_str())
        || process
            .cmd()
            .iter()
            .any(|arg| arg.to_string_lossy().contains("org.codehaus.plexus"))
    {
        return Err(error(
            "invalid_target",
            "请选择应用 Java 进程，不能附加或终止 Maven 构建进程",
        ));
    }
    Ok((pid, process.start_time()))
}
fn kill_tree(root: (u32, u64)) -> Result<()> {
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
fn configure(command: &mut Command) {
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
fn reader(
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
fn execute(mut command: Command, run: &Arc<Mutex<Run>>, phase: &str) -> Result<Option<i32>> {
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
impl JavaService {
    pub(crate) fn managed_identity(&self, project: &DetectedProject) -> Option<(u32, u64)> {
        let runs = self.runs.lock().unwrap();
        let run = runs.get(&key(project))?.lock().unwrap();
        if matches!(run.state.as_str(), "starting" | "running") {
            run.pid
        } else {
            None
        }
    }

    pub fn output(
        &self,
        project: &DetectedProject,
        after: u64,
        run_id: Option<&str>,
    ) -> Result<JavaOutput> {
        java_project(project)?;
        let runs = self.runs.lock().unwrap();
        Ok(runs.get(&key(project)).map_or_else(
            || Run::new("external").output(0),
            |run| {
                let run = run.lock().unwrap();
                run.output(if run_id == Some(run.id.as_str()) {
                    after
                } else {
                    0
                })
            },
        ))
    }
    pub fn start(
        &self,
        project: DetectedProject,
        jvm: Vec<String>,
        args: Vec<String>,
        profiles: Vec<String>,
        main: Option<String>,
    ) -> Result<JavaOutput> {
        java_project(&project)?;
        validate_args(&jvm, &args, &profiles)?;
        let main = main
            .or_else(|| (project.main_classes.len() == 1).then(|| project.main_classes[0].clone()))
            .filter(|m| project.main_classes.contains(m))
            .ok_or_else(|| error("main_class_required", "请选择项目启动主类"))?;
        let mut runs = self.runs.lock().unwrap();
        if runs
            .get(&key(&project))
            .is_some_and(|run| matches!(run.lock().unwrap().state.as_str(), "starting" | "running"))
        {
            return Err(error("conflict", "该项目正在启动或运行中"));
        }
        if runs.len() >= 128 && !runs.contains_key(&key(&project)) {
            return Err(error(
                "unavailable",
                "运行记录已达上限，请重启 DevFlow 清理历史记录",
            ));
        }
        let run = Arc::new(Mutex::new(Run::new("starting")));
        runs.insert(key(&project), run.clone());
        run.lock()
            .unwrap()
            .log("system", "正在准备 Maven 编译和运行依赖…");
        let result = run.lock().unwrap().output(0);
        thread::spawn(move || {
            let cp =
                std::env::temp_dir().join(format!("devflow-{}.classpath", uuid::Uuid::new_v4()));
            let outcome = (|| -> Result<Option<i32>> {
                let root = Path::new(&project.path).canonicalize()?;
                let code = execute(maven_command(&root, &cp)?, &run, "starting")?;
                if code != Some(0) {
                    return Err(error(
                        "build_failed",
                        format!("Maven 准备失败，退出码 {code:?}；请查看上方输出"),
                    ));
                }
                let dependencies = fs::read_to_string(&cp)?;
                let classes = root.join("target/classes").canonicalize()?;
                if !classes.starts_with(&root) {
                    return Err(error("outside_workspace", "编译输出超出项目目录"));
                }
                let classpath = format!(
                    "{}{}{}",
                    crate::workspace::display_path(&classes),
                    if cfg!(windows) { ";" } else { ":" },
                    dependencies.trim()
                );
                let mut command = java_command()?;
                command
                    .args(jvm)
                    .arg("-Dfile.encoding=UTF-8")
                    .arg("-cp")
                    .arg(classpath)
                    .arg(main)
                    .args(args)
                    .current_dir(&root);
                if !profiles.is_empty() {
                    command.arg(format!("--spring.profiles.active={}", profiles.join(",")));
                }
                run.lock()
                    .unwrap()
                    .log("system", "编译完成，正在启动 Java 进程…");
                execute(command, &run, "running")
            })();
            let _ = fs::remove_file(cp);
            let mut state = run.lock().unwrap();
            match outcome {
                Ok(code) => {
                    state.exit = code;
                    state.state = if state.cancelled || code == Some(0) {
                        "stopped"
                    } else {
                        "failed"
                    }
                    .into();
                    state.log("system", format!("进程退出：{code:?}"));
                }
                Err(err) => {
                    state.state = if state.cancelled { "stopped" } else { "failed" }.into();
                    state.log("system", err.message);
                }
            }
            state.pid = None;
            state.arthas_url = None;
        });
        Ok(result)
    }
    pub fn stop(
        &self,
        project: &DetectedProject,
        external_pid: Option<(u32, u64)>,
    ) -> Result<JavaOutput> {
        java_project(project)?;
        let mut runs = self.runs.lock().unwrap();
        let run = runs
            .entry(key(project))
            .or_insert_with(|| Arc::new(Mutex::new(Run::new("external"))))
            .clone();
        let mut state = run.lock().unwrap();
        let target = if matches!(state.state.as_str(), "starting" | "running") {
            state.pid
        } else {
            external_pid
        };
        if target.is_none() && !matches!(state.state.as_str(), "starting" | "running") {
            return Err(error("conflict", "未找到可停止的 Java 进程"));
        }
        state.cancelled = true;
        if let Some(target) = target {
            if let Err(err) = kill_tree(target) {
                state.cancelled = false;
                return Err(err);
            }
        }
        state.arthas_url = None;
        state.state = "stopped".into();
        state.log("system", "已请求停止项目进程树。");
        Ok(state.output(0))
    }
    pub fn attach(&self, project: &DetectedProject, target: (u32, u64)) -> Result<JavaOutput> {
        java_project(project)?;
        let pid = target.0;
        let boot = std::env::var_os("DEVFLOW_ARTHAS_BOOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".devflow/tools/arthas-boot.jar"));
        if !boot.is_file() {
            return Err(error(
                "arthas_missing",
                "缺少官方 arthas-boot.jar，请将其放到 .devflow/tools/arthas-boot.jar，或配置 DEVFLOW_ARTHAS_BOOT 后重启",
            ));
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let telnet_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let telnet_port = telnet_listener.local_addr()?.port();
        drop(listener);
        drop(telnet_listener);
        let mut runs = self.runs.lock().unwrap();
        let run = runs
            .entry(key(project))
            .or_insert_with(|| Arc::new(Mutex::new(Run::new("external"))))
            .clone();
        {
            let mut state = run.lock().unwrap();
            if state.attaching || state.arthas_url.is_some() {
                return Err(error("conflict", "Arthas 已附加或正在附加，请查看诊断输出"));
            }
            state.attaching = true;
            state.log("arthas", format!("正在向 Java PID {pid} 附加 Arthas…"));
        }
        let result = run.lock().unwrap().output(0);
        thread::spawn(move || {
            let outcome = (|| -> Result<()> {
                let mut command = java_command()?;
                command
                    .arg("-Dfile.encoding=UTF-8")
                    .arg("-jar")
                    .arg(boot)
                    .arg(pid.to_string())
                    .args(["--attach-only", "--target-ip", "127.0.0.1", "--telnet-port"])
                    .arg(telnet_port.to_string())
                    .arg("--http-port")
                    .arg(port.to_string());
                configure(&mut command);
                if java_identity(pid)? != target {
                    return Err(error("conflict", "目标 Java 进程已变化，请刷新后重试"));
                }
                let mut child = command.spawn()?;
                let out = reader(child.stdout.take().unwrap(), run.clone(), "arthas");
                let err = reader(child.stderr.take().unwrap(), run.clone(), "arthas");
                let deadline = std::time::Instant::now() + Duration::from_secs(90);
                let status = loop {
                    if let Some(status) = child.try_wait()? {
                        break status;
                    }
                    if run.lock().unwrap().cancelled || std::time::Instant::now() >= deadline {
                        let _ = kill_tree(identity(child.id()).unwrap_or((child.id(), 0)));
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error("timeout", "Arthas 附加超时，请查看启动器输出"));
                    }
                    thread::sleep(Duration::from_millis(100));
                };
                let _ = out.join();
                let _ = err.join();
                if !status.success() {
                    return Err(error("attach_failed", format!("Arthas 附加失败：{status}")));
                }
                if java_identity(pid)? != target {
                    return Err(error("conflict", "目标 Java 进程在附加期间已变化"));
                }
                let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
                let ready = std::time::Instant::now() + Duration::from_secs(10);
                loop {
                    if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(250))
                        .is_ok()
                    {
                        break;
                    }
                    if std::time::Instant::now() >= ready {
                        return Err(error(
                            "attach_failed",
                            "Arthas 未监听预期端口；请查看目标 JVM 的 logs/arthas/arthas.log",
                        ));
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Ok(())
            })();
            let mut state = run.lock().unwrap();
            state.attaching = false;
            match outcome {
                Ok(()) => {
                    state.arthas_url = Some(format!("http://127.0.0.1:{port}"));
                    state.log("arthas", format!("Arthas 已附加，HTTP 端口 {port}"));
                }
                Err(err) => state.log("arthas", err.message),
            }
        });
        Ok(result)
    }
}

impl Drop for JavaService {
    fn drop(&mut self) {
        if let Ok(runs) = self.runs.lock() {
            for run in runs.values() {
                if let Ok(mut state) = run.lock() {
                    if matches!(state.state.as_str(), "starting" | "running") {
                        state.cancelled = true;
                        if let Some(pid) = state.pid {
                            let _ = kill_tree(pid);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_launch_options() {
        assert!(
            validate_args(
                &["-Xmx512m".into(), "-Dname=中文 空格".into()],
                &["--name=a b".into()],
                &["dev".into()]
            )
            .is_ok()
        );
        assert!(validate_args(&["-jar".into()], &[], &[]).is_err());
        assert!(validate_args(&[], &["bad\0arg".into()], &[]).is_err());
        assert!(validate_args(&[], &[], &["../prod".into()]).is_err());
    }
    #[test]
    fn bounds_logs_and_detects_cursor_gaps() {
        let mut run = Run::new("running");
        for i in 0..2200 {
            run.log("stdout", format!("行{i}"));
        }
        let output = run.output(0);
        assert!(output.truncated);
        assert_eq!(output.lines.len(), 500);
        assert_eq!(output.lines[0].sequence, 201);
        assert_eq!(output.next_cursor, 700);
    }
    #[test]
    fn refuses_pid_reuse() {
        assert!(kill_tree((std::process::id(), 0)).is_ok());
    }
}
