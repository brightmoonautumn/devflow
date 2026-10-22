//! Java execution shared by HTTP/MCP. Arguments never pass through a shell.
pub use super::process::{LogLine, ProcessOutput as JavaOutput};
use super::process::{Run, configure, identity, kill_tree, process_system, reader};
use super::managed::{self, CommandSpec, LaunchPlan};
pub use super::maven::{BuildMode, BuildOptions};
use super::maven::{self, MavenPlan};
use crate::workspace::{DetectedProject, ProjectKind, WorkspaceError};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use sysinfo::Pid;

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
        #[serde(default)]
        maven_profiles: Vec<String>,
        #[serde(default)]
        build_mode: BuildMode,
        #[serde(default)]
        force_build: bool,
    },
    #[serde(rename_all = "camelCase")]
    Restart {
        #[serde(default)]
        jvm_args: Vec<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        profiles: Vec<String>,
        main_class: Option<String>,
        #[serde(default)]
        maven_profiles: Vec<String>,
        #[serde(default)]
        build_mode: BuildMode,
        #[serde(default)]
        force_build: bool,
        pid: Option<u32>,
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
fn maven_plan(root: &Path, boundary: &Path, options: BuildOptions) -> Result<MavenPlan> {
    if options.maven_profiles.len() > 16 || options.maven_profiles.iter().any(|p| !valid_profile(p)) {
        return Err(error("invalid_arguments", "Maven Profile 仅支持字母、数字、点、横线和下划线"));
    }
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
    let home = home.canonicalize()?;
    if !home.join("lib").is_dir() || !home.join("boot").is_dir() {
        return Err(error("maven_missing", "Maven 安装缺少 lib/boot 目录"));
    }
    let boundary = boundary.canonicalize()?;
    if !root.starts_with(&boundary) { return Err(error("outside_workspace", "启动模块超出工作区目录")); }
    let reactor = maven::reactor_root(root, &boundary)?;
    let configuration = reactor.ancestors().take_while(|path| path.starts_with(&boundary))
        .find(|path| path.join(".mvn").is_dir()).unwrap_or(&reactor).to_owned();
    Ok(MavenPlan { reactor, configuration, boundary, target: root.to_owned(), home, java: CommandSpec::from(&java_command()?),
        profiles: options.maven_profiles, mode: options.build_mode, force: options.force_build })
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
            .any(|arg| maven::is_launcher_arg(&arg.to_string_lossy()))
    {
        return Err(error(
            "invalid_target",
            "请选择应用 Java 进程，不能附加或终止 Maven 构建进程",
        ));
    }
    Ok((pid, process.start_time()))
}
impl JavaService {
    pub(crate) fn managed_identity(&self, project: &DetectedProject) -> Option<(u32, u64)> {
        let mut runs = self.runs.lock().unwrap();
        managed::restore_into(&mut runs, "java", &key(project)).ok()?;
        let mut run = runs.get(&key(project))?.lock().unwrap();
        managed::refresh(&mut run).ok()?;
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
        let mut runs = self.runs.lock().unwrap();
        managed::restore_into(&mut runs, "java", &key(project))?;
        runs.get(&key(project)).map_or_else(
            || Ok(Run::new("external").output(0)),
            |run| {
                let mut run = run.lock().unwrap();
                managed::refresh(&mut run)?;
                Ok(run.output(if run_id == Some(run.id.as_str()) {
                    after
                } else {
                    0
                }))
            },
        )
    }
    pub fn start(
        &self,
        project: DetectedProject,
        jvm: Vec<String>,
        args: Vec<String>,
        profiles: Vec<String>,
        main: Option<String>,
    ) -> Result<JavaOutput> {
        let boundary = PathBuf::from(&project.path);
        self.launch(project, jvm, args, profiles, main, None, boundary, BuildOptions::default())
    }
    pub fn restart(
        &self,
        project: DetectedProject,
        jvm: Vec<String>,
        args: Vec<String>,
        profiles: Vec<String>,
        main: Option<String>,
        external: Option<(u32, u64)>,
    ) -> Result<JavaOutput> {
        let boundary = PathBuf::from(&project.path);
        self.launch(project, jvm, args, profiles, main, Some(external), boundary, BuildOptions::default())
    }
    pub(crate) fn start_with_build(&self, project: DetectedProject, jvm: Vec<String>, args: Vec<String>, profiles: Vec<String>, main: Option<String>, boundary: PathBuf, build: BuildOptions) -> Result<JavaOutput> {
        self.launch(project, jvm, args, profiles, main, None, boundary, build)
    }
    pub(crate) fn restart_with_build(&self, project: DetectedProject, jvm: Vec<String>, args: Vec<String>, profiles: Vec<String>, main: Option<String>, external: Option<(u32, u64)>, boundary: PathBuf, build: BuildOptions) -> Result<JavaOutput> {
        self.launch(project, jvm, args, profiles, main, Some(external), boundary, build)
    }
    fn launch(
        &self,
        project: DetectedProject,
        jvm: Vec<String>,
        args: Vec<String>,
        profiles: Vec<String>,
        main: Option<String>,
        restart: Option<Option<(u32, u64)>>,
        boundary: PathBuf,
        build: BuildOptions,
    ) -> Result<JavaOutput> {
        java_project(&project)?;
        validate_args(&jvm, &args, &profiles)?;
        let main = main
            .or_else(|| (project.main_classes.len() == 1).then(|| project.main_classes[0].clone()))
            .filter(|m| project.main_classes.contains(m))
            .ok_or_else(|| error("main_class_required", "请选择项目启动主类"))?;
        let root = Path::new(&project.path).canonicalize()?;
        let maven = maven_plan(&root, &boundary, build)?;
        let mut java = java_command()?;
        java.args(jvm).arg("-Dfile.encoding=UTF-8");
        if let Some(external) = restart {
            self.stop(&project, external)?;
        }
        let mut runs = self.runs.lock().unwrap();
        managed::restore_into(&mut runs, "java", &key(&project))?;
        if let Some(run) = runs.get(&key(&project)) { managed::refresh(&mut run.lock().unwrap())?; }
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
        let run = Arc::new(Mutex::new(managed::launch("java", &key(&project), LaunchPlan::JavaCached {
            maven, java: CommandSpec::from(&java), root, main, args, profiles,
        })?));
        run.lock().unwrap().log("system", "独立服务宿主正在检查构建缓存，需要时按依赖链构建…");
        let result = run.lock().unwrap().output(0);
        runs.insert(key(&project), run);
        Ok(result)
    }
    pub fn stop(
        &self,
        project: &DetectedProject,
        external_pid: Option<(u32, u64)>,
    ) -> Result<JavaOutput> {
        java_project(project)?;
        let mut runs = self.runs.lock().unwrap();
        managed::restore_into(&mut runs, "java", &key(project))?;
        let run = runs
            .entry(key(project))
            .or_insert_with(|| Arc::new(Mutex::new(Run::new("external"))))
            .clone();
        let mut state = run.lock().unwrap();
        managed::refresh(&mut state)?;
        if state.managed.is_some() && matches!(state.state.as_str(), "starting" | "running") {
            managed::stop(&mut state)?;
            state.arthas_url = None;
            state.log("system", "已停止独立运行的 Java 服务进程树。");
            return Ok(state.output(0));
        }
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
        managed::restore_into(&mut runs, "java", &key(project))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_rejects_invalid_options_without_stopping_current_run() {
        let project = DetectedProject {
            name: "中文服务".into(),
            kind: crate::workspace::ProjectKind::Maven,
            path: r"D:\工作区 空格\服务".into(),
            markers: vec![], main_classes: vec!["example.Application".into()],
            launch_scripts: vec!["dev".into()], node_script: None,
            node_selector: None,
        };
        let service = JavaService::default();
        let run = Arc::new(Mutex::new(Run::new("running")));
        let runtime_key = key(&project);
        service.runs.lock().unwrap().insert(runtime_key, run.clone());
        let result = service.restart(project.clone(), vec!["-jar".into()], vec![], vec![], None, None);
        assert_eq!(result.err().unwrap().code, "invalid_arguments");
        let result = service.restart(project, vec![], vec![], vec![], Some("removed.Main".into()), None);
        assert_eq!(result.err().unwrap().code, "main_class_required");
        let state = run.lock().unwrap();
        assert_eq!(state.state, "running");
        assert!(!state.cancelled);
    }
    #[test]
    fn restart_request_keeps_launch_options_and_optional_pid() {
        let request: JavaRequest = serde_json::from_value(serde_json::json!({
            "projectPath": "D:/工作区 空格/服务", "expectedRevision": 7,
            "operation": { "type": "restart", "mainClass": "example.Application", "jvmArgs": ["-Xmx512m"], "args": ["--name=中文 空格"], "profiles": ["dev"], "pid": 42 }
        })).unwrap();
        assert!(matches!(request.operation, JavaOperation::Restart { pid: Some(42), jvm_args, args, profiles, main_class: Some(_), .. } if jvm_args == vec!["-Xmx512m"] && args == vec!["--name=中文 空格"] && profiles == vec!["dev"]));
    }

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
    fn launch_build_options_are_optional_and_maven_profiles_are_separate() {
        let legacy: JavaRequest = serde_json::from_value(serde_json::json!({
            "projectPath": "D:/中文/app", "expectedRevision": 1, "operation": {"type": "start"}
        })).unwrap();
        assert!(matches!(legacy.operation, JavaOperation::Start { build_mode: BuildMode::Workspace, force_build: false, maven_profiles, .. } if maven_profiles.is_empty()));
        let request: JavaRequest = serde_json::from_value(serde_json::json!({
            "projectPath": "D:/中文/app", "expectedRevision": 1,
            "operation": {"type": "start", "profiles": ["dev"], "mavenProfiles": ["build-local"], "buildMode": "install", "forceBuild": true}
        })).unwrap();
        assert!(matches!(request.operation, JavaOperation::Start { build_mode: BuildMode::Install, force_build: true, profiles, maven_profiles, .. } if profiles == ["dev"] && maven_profiles == ["build-local"]));
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
