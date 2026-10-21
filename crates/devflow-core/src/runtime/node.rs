//! Run only discovered package scripts in authorized workspace roots.
pub use super::process::ProcessOutput as NodeOutput;
use super::process::{Run, kill_tree};
use super::managed::{self, CommandSpec, LaunchPlan};
use crate::workspace::{DetectedProject, WorkspaceError, node_launch_scripts};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

type Result<T> = std::result::Result<T, WorkspaceError>;
fn error(code: &'static str, message: impl Into<String>) -> WorkspaceError {
    WorkspaceError {
        code,
        message: message.into(),
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}
impl PackageManager {
    fn name(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeRequest {
    pub project_path: String,
    pub script: Option<String>,
    pub expected_revision: u64,
    pub operation: NodeOperation,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeOperation {
    #[serde(rename_all = "camelCase")]
    Start {
        package_manager: Option<PackageManager>,
        #[serde(default)]
        args: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Restart {
        package_manager: Option<PackageManager>,
        #[serde(default)]
        args: Vec<String>,
        pid: Option<u32>,
    },
    Stop {
        pid: Option<u32>,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeQuery {
    pub project_path: String,
    pub script: Option<String>,
    #[serde(default)]
    pub after: u64,
    pub run_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeOptions {
    pub scripts: Vec<String>,
    pub package_manager: PackageManager,
}

fn descriptor(project: &DetectedProject) -> Result<serde_json::Value> {
    let root = Path::new(&project.path).canonicalize()?;
    let file = root.join("package.json").canonicalize()?;
    if !file.starts_with(&root) {
        return Err(error("outside_workspace", "package.json 超出项目目录"));
    }
    let file = fs::File::open(file)?;
    if file.metadata()?.len() > 2 * 1024 * 1024 {
        return Err(error("invalid_arguments", "package.json 超过读取限制"));
    }
    let mut text = String::new();
    file.take(2 * 1024 * 1024 + 1).read_to_string(&mut text)?;
    if text.len() > 2 * 1024 * 1024 {
        return Err(error("invalid_arguments", "package.json 超过读取限制"));
    }
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| error("invalid_arguments", format!("无法读取 package.json：{e}")))
}
fn package_manager(value: &serde_json::Value, root: &Path) -> PackageManager {
    match value
        .get("packageManager")
        .and_then(|v| v.as_str())
        .and_then(|v| v.split('@').next())
    {
        Some("pnpm") => PackageManager::Pnpm,
        Some("yarn") => PackageManager::Yarn,
        Some("npm") => PackageManager::Npm,
        _ if root.join("pnpm-lock.yaml").is_file() => PackageManager::Pnpm,
        _ if root.join("yarn.lock").is_file() => PackageManager::Yarn,
        _ => PackageManager::Npm,
    }
}
pub fn options(project: &DetectedProject) -> Result<NodeOptions> {
    let value = descriptor(project)?;
    let scripts = node_launch_scripts(&value)
        .into_iter()
        .filter(|s| project.launch_scripts.contains(s))
        .collect();
    Ok(NodeOptions {
        scripts,
        package_manager: package_manager(&value, Path::new(&project.path)),
    })
}
fn find_node() -> Result<PathBuf> {
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    if let Some(home) =
        std::env::var_os("DEVFLOW_NODE_HOME").or_else(|| std::env::var_os("NODE_HOME"))
    {
        let home = PathBuf::from(home);
        for path in [home.join(exe), home.join("bin").join(exe)] {
            if path.is_file() {
                return Ok(path);
            }
        }
        return Err(error(
            "node_missing",
            "配置的 Node.js 目录中找不到 node，请检查 DEVFLOW_NODE_HOME / NODE_HOME",
        ));
    }
    std::env::var_os("PATH")
        .and_then(|v| {
            std::env::split_paths(&v)
                .map(|p| p.join(exe))
                .find(|p| p.is_file())
        })
        .ok_or_else(|| {
            error(
                "node_missing",
                "找不到 Node.js，请将 node 加入 PATH 或设置 DEVFLOW_NODE_HOME",
            )
        })
}
fn manager_candidates(dir: &Path, manager: PackageManager) -> Vec<PathBuf> {
    let entry = match manager {
        PackageManager::Npm => "npm/bin/npm-cli.js",
        PackageManager::Pnpm => "pnpm/bin/pnpm.cjs",
        PackageManager::Yarn => "yarn/bin/yarn.js",
    };
    let mut result = vec![
        dir.join("node_modules").join(entry),
        dir.join("../lib/node_modules").join(entry),
    ];
    if matches!(manager, PackageManager::Pnpm) {
        result.push(dir.join("node_modules/pnpm/bin/pnpm.mjs"));
        result.push(dir.join("../lib/node_modules/pnpm/bin/pnpm.mjs"));
    }
    result
}
fn command(root: &Path, manager: PackageManager, script: &str, args: &[String]) -> Result<Command> {
    let node = find_node()?;
    let mut dirs = vec![node.parent().unwrap_or(root).to_owned()];
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    if let Some(home) =
        std::env::var_os("PNPM_HOME").filter(|_| matches!(manager, PackageManager::Pnpm))
    {
        dirs.insert(0, PathBuf::from(home));
    }
    let override_path = std::env::var_os(format!("DEVFLOW_{}_CLI", manager.name().to_uppercase()))
        .map(PathBuf::from);
    if override_path.as_ref().is_some_and(|p| !p.is_file()) {
        return Err(error(
            "package_manager_missing",
            "配置的包管理器 CLI 文件不存在",
        ));
    }
    let executable = override_path.or_else(|| {
        dirs.iter()
            .flat_map(|dir| manager_candidates(dir, manager))
            .find(|p| p.is_file())
    });
    let standalone = dirs
        .iter()
        .map(|dir| dir.join(format!("{}.exe", manager.name())))
        .find(|p| p.is_file());
    let corepack = || {
        dirs.iter()
            .flat_map(|dir| {
                [
                    dir.join(format!("node_modules/corepack/dist/{}.js", manager.name())),
                    dir.join(format!(
                        "../lib/node_modules/corepack/dist/{}.js",
                        manager.name()
                    )),
                ]
            })
            .find(|p| p.is_file())
    };
    let mut result = if let Some(cli) = executable {
        let mut command = Command::new(&node);
        command.arg(cli);
        command
    } else if let Some(exe) = standalone {
        Command::new(exe)
    } else if let Some(cli) = corepack() {
        let mut command = Command::new(&node);
        command.arg(cli);
        command
    } else {
        return Err(error(
            "package_manager_missing",
            format!(
                "找不到 {} 启动器，请在本机安装并加入 PATH；DevFlow 不会自动安装依赖",
                manager.name()
            ),
        ));
    };
    result.arg("run").arg(script);
    if !args.is_empty() {
        if matches!(manager, PackageManager::Npm) {
            result.arg("--");
        }
        result.args(args);
    }
    result
        .current_dir(root)
        .env("COREPACK_ENABLE_NETWORK", "0")
        .env("COREPACK_ENABLE_AUTO_PIN", "0")
        .env("npm_config_manage_package_manager_versions", "false")
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("NO_COLOR", "1");
    // Nested package scripts must use the same selected Node installation.
    let mut paths = vec![node.parent().unwrap().to_owned()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    if let Ok(paths) = std::env::join_paths(paths) {
        result.env("PATH", paths);
    }
    Ok(result)
}
fn validate_args(args: &[String]) -> Result<()> {
    if args.len() > 128
        || args.iter().any(|v| v.contains('\0') || v.len() > 8192)
        || args.iter().map(String::len).sum::<usize>() > 24000
    {
        return Err(error("invalid_arguments", "参数过长或包含无效字符"));
    }
    Ok(())
}
pub fn node_identity(pid: u32) -> Result<(u32, u64)> {
    let system = super::process::process_system();
    let process = system
        .process(sysinfo::Pid::from_u32(pid))
        .ok_or_else(|| error("conflict", "目标进程已退出"))?;
    if !["node", "node.exe", "pnpm", "pnpm.exe"].contains(
        &process
            .name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str(),
    ) {
        return Err(error("invalid_target", "请选择属于该服务的 Node.js 进程"));
    }
    Ok((pid, process.start_time()))
}
#[derive(Default)]
pub struct NodeService {
    runs: Mutex<HashMap<String, Arc<Mutex<Run>>>>,
}
impl NodeService {
    pub(crate) fn managed_identity(&self, project: &DetectedProject) -> Option<(u32, u64)> {
        let mut runs = self.runs.lock().unwrap();
        managed::restore_into(&mut runs, "node", &project.runtime_key()).ok()?;
        let mut run = runs.get(&project.runtime_key())?.lock().unwrap();
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
    ) -> Result<NodeOutput> {
        let mut runs = self.runs.lock().unwrap();
        managed::restore_into(&mut runs, "node", &project.runtime_key())?;
        runs.get(&project.runtime_key()).map_or_else(
            || Ok(Run::new("external").output(0)),
            |run| {
                let mut run = run.lock().unwrap();
                managed::refresh(&mut run)?;
                Ok(run.output(if run_id == Some(&run.id) { after } else { 0 }))
            },
        )
    }
    pub fn start(
        &self,
        project: DetectedProject,
        script: Option<String>,
        manager: Option<PackageManager>,
        args: Vec<String>,
    ) -> Result<NodeOutput> {
        self.launch(project, script, manager, args, None)
    }
    pub fn restart(
        &self,
        project: DetectedProject,
        script: Option<String>,
        manager: Option<PackageManager>,
        args: Vec<String>,
        external: Option<(u32, u64)>,
    ) -> Result<NodeOutput> {
        self.launch(project, script, manager, args, Some(external))
    }
    fn launch(
        &self,
        project: DetectedProject,
        script: Option<String>,
        manager: Option<PackageManager>,
        args: Vec<String>,
        restart: Option<Option<(u32, u64)>>,
    ) -> Result<NodeOutput> {
        validate_args(&args)?;
        let available = options(&project)?;
        let script = script
            .or_else(|| available.scripts.first().cloned())
            .filter(|s| available.scripts.contains(s))
            .ok_or_else(|| {
                error(
                    "invalid_arguments",
                    "启动脚本已改变或不是可运行服务，请刷新工作区",
                )
            })?;
        let manager = manager.unwrap_or(available.package_manager);
        let root = Path::new(&project.path).canonicalize()?;
        let command = command(&root, manager, &script, &args)?;
        if let Some(external) = restart {
            self.stop(&project, external)?;
        }
        let mut runs = self.runs.lock().unwrap();
        let key = project.runtime_key();
        managed::restore_into(&mut runs, "node", &key)?;
        if let Some(run) = runs.get(&key) { managed::refresh(&mut run.lock().unwrap())?; }
        if runs
            .get(&key)
            .is_some_and(|run| matches!(run.lock().unwrap().state.as_str(), "starting" | "running"))
        {
            return Err(error("conflict", "该服务正在启动或运行中"));
        }
        if runs.len() >= 128 && !runs.contains_key(&key) {
            return Err(error(
                "unavailable",
                "运行记录已达上限，请重启 DevFlow 清理历史记录",
            ));
        }
        let run = Arc::new(Mutex::new(managed::launch("node", &key, LaunchPlan::Node { command: CommandSpec::from(&command) })?));
        run.lock().unwrap().log(
            "system",
            format!(
                "正在启动 {} run {script}（目录：{}）",
                manager.name(),
                project.path
            ),
        );
        let result = run.lock().unwrap().output(0);
        runs.insert(key, run.clone());
        Ok(result)
    }
    pub fn stop(
        &self,
        project: &DetectedProject,
        external: Option<(u32, u64)>,
    ) -> Result<NodeOutput> {
        let mut runs = self.runs.lock().unwrap();
        managed::restore_into(&mut runs, "node", &project.runtime_key())?;
        let run = runs
            .entry(project.runtime_key())
            .or_insert_with(|| Arc::new(Mutex::new(Run::new("external"))))
            .clone();
        let mut state = run.lock().unwrap();
        managed::refresh(&mut state)?;
        if state.managed.is_some() && matches!(state.state.as_str(), "starting" | "running") {
            managed::stop(&mut state)?;
            state.log("system", "已停止独立运行的 Node.js 服务进程树。");
            return Ok(state.output(0));
        }
        let active = matches!(state.state.as_str(), "starting" | "running");
        let target = if active { state.pid } else { external };
        if !active && target.is_none() {
            return Err(error("conflict", "未找到可停止的 Node.js 进程"));
        }
        state.cancelled = true;
        if let Some(target) = target {
            if let Err(err) = kill_tree(target) {
                state.cancelled = false;
                return Err(err);
            }
        }
        state.state = "stopped".into();
        state.log("system", "已停止 Node.js 服务进程树。");
        Ok(state.output(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_rejects_invalid_options_without_stopping_current_run() {
        let project = DetectedProject {
            name: "中文服务".into(),
            kind: crate::workspace::ProjectKind::Node,
            path: r"D:\工作区 空格\服务".into(),
            markers: vec![], main_classes: vec!["example.Application".into()],
            launch_scripts: vec!["dev".into()], node_script: Some("dev".into()),
            node_selector: None,
        };
        let service = NodeService::default();
        let run = Arc::new(Mutex::new(Run::new("running")));
        let runtime_key = project.runtime_key();
        service.runs.lock().unwrap().insert(runtime_key, run.clone());
        let result = service.restart(project, Some("dev".into()), None, vec!["bad\0arg".into()], None);
        assert_eq!(result.err().unwrap().code, "invalid_arguments");
        let state = run.lock().unwrap();
        assert_eq!(state.state, "running");
        assert!(!state.cancelled);
    }
    #[test]
    fn restart_request_keeps_launch_options_and_optional_pid() {
        let request: NodeRequest = serde_json::from_value(serde_json::json!({
            "projectPath": "D:/工作区 空格/服务", "expectedRevision": 7,
            "script": "dev",
            "operation": { "type": "restart", "packageManager": "pnpm", "args": ["--host=127.0.0.1"] }
        })).unwrap();
        assert!(matches!(request.operation, NodeOperation::Restart { pid: None, package_manager: Some(PackageManager::Pnpm), args } if args == vec!["--host=127.0.0.1"]));
    }

    #[test]
    fn manager_preference_and_argument_limits() {
        assert!(matches!(
            package_manager(
                &serde_json::json!({"packageManager":"pnpm@10.0.0"}),
                Path::new(".")
            ),
            PackageManager::Pnpm
        ));
        assert!(validate_args(&["中文 空格".into()]).is_ok());
        assert!(validate_args(&["x\0y".into()]).is_err());
        assert!(validate_args(&vec!["x".into(); 129]).is_err());
    }
    #[test]
    fn pid_reuse_is_safe() {
        assert!(kill_tree((std::process::id(), 0)).is_ok());
        assert!(node_identity(std::process::id()).is_err());
    }
}
