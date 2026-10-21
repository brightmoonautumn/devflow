//! Process observations and managed Java execution shared by HTTP and MCP.
pub mod java;
pub mod node;
pub mod managed;
mod ports;
mod presentation;
pub(crate) mod process;
use crate::workspace::{DetectedProject, ProjectKind, detect_projects};
use serde::Serialize;
use std::{
    collections::HashMap,
    io,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRuntime {
    pub project_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_script: Option<String>,
    pub node_pids: Vec<u32>,
    pub pids: Vec<u32>,
    pub java_pids: Vec<u32>,
    pub primary_pids: Vec<u32>,
    pub processes: Vec<ProjectProcess>,
    pub listeners: Option<Vec<ProjectListener>>,
    pub listening_ports: Option<Vec<u16>>,
    pub port_error: Option<String>,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f32>,
    pub uptime_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_state: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProcess {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub role: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListener {
    pub pid: u32,
    pub port: u16,
    pub purpose: String,
    pub purpose_source: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub sampled_at: u64,
    pub projects: Vec<ProjectRuntime>,
}

#[derive(Clone, Debug)]
struct Observation {
    pid: u32,
    parent: Option<u32>,
    started: u64,
    name: String,
    cwd: Option<String>,
    args: Vec<String>,
    memory: u64,
    cpu: Option<f32>,
}
struct RuntimeProject {
    path: String,
    node_script: Option<String>,
    node_selector: Option<String>,
    root: String,
    java: bool,
    node: bool,
    main_classes: Vec<String>,
}

struct Sampler {
    system: System,
    identities: HashMap<u32, u64>,
    cached: Option<(Instant, Vec<String>, RuntimeSnapshot)>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevFlowMetrics {
    pub sampled_at: u64,
    pub pid: u32,
    pub memory_bytes: Option<u64>,
    pub cpu_percent: Option<f32>,
    pub uptime_seconds: Option<u64>,
}
struct SelfSampler {
    system: System,
    cached: Option<(Instant, DevFlowMetrics)>,
}
pub struct RuntimeService {
    projects: Mutex<Sampler>,
    own_process: Mutex<SelfSampler>,
}

impl Default for RuntimeService {
    fn default() -> Self {
        Self {
            projects: Mutex::new(Sampler {
                system: System::new(),
                identities: HashMap::new(),
                cached: None,
            }),
            own_process: Mutex::new(SelfSampler {
                system: System::new(),
                cached: None,
            }),
        }
    }
}

impl RuntimeService {
    pub fn invalidate(&self) {
        if let Ok(mut sampler) = self.projects.lock() {
            sampler.cached = None;
        }
    }
    /// Sample only this backend PID, even with no workspace open. A separate
    /// sampler avoids changing managed-project CPU measurement intervals.
    pub fn self_snapshot(&self) -> io::Result<DevFlowMetrics> {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return Err(io::Error::other(
                "Process metrics unsupported on this platform",
            ));
        }
        let mut sampler = self
            .own_process
            .lock()
            .map_err(|_| io::Error::other("DevFlow sampler unavailable"))?;
        if let Some((time, cached)) = &sampler.cached {
            if time.elapsed() < Duration::from_secs(2) {
                return Ok(cached.clone());
            }
        }
        let ready = sampler.cached.is_some();
        let pid = sysinfo::Pid::from_u32(std::process::id());
        sampler.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_memory()
                .with_cpu(),
        );
        let process = sampler
            .system
            .process(pid)
            .ok_or_else(|| io::Error::other("DevFlow process metrics unavailable"))?;
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let result = DevFlowMetrics {
            sampled_at: now_seconds(),
            pid: pid.as_u32(),
            memory_bytes: Some(process.memory()),
            cpu_percent: normalized_self_cpu(ready.then(|| process.cpu_usage()), cores),
            uptime_seconds: Some(process.run_time()),
        };
        sampler.cached = Some((Instant::now(), result.clone()));
        Ok(result)
    }
    /// Roots come only from server configuration, never from request arguments.
    /// CPU requires two observations. Newly seen PIDs return null for the first.
    pub fn snapshot(&self, root: Option<&Path>) -> io::Result<RuntimeSnapshot> {
        let mut projects = Vec::new();
        if let Some(root) = root {
            let canonical_root = root.canonicalize()?;
            projects = detect_projects(&canonical_root)?
                .into_iter()
                .filter(|project| {
                    Path::new(&project.path)
                        .canonicalize()
                        .is_ok_and(|path| path.starts_with(&canonical_root))
                })
                .collect();
        }
        self.snapshot_for_projects(&projects)
    }

    /// The caller supplies only projects authorized by the workspace service.
    pub fn snapshot_for_projects(
        &self,
        authorized: &[DetectedProject],
    ) -> io::Result<RuntimeSnapshot> {
        self.snapshot_with_managed(authorized, &[])
    }

    /// Managed PIDs are supplied only by the core Java launcher, never by clients.
    pub(crate) fn snapshot_with_managed(
        &self,
        authorized: &[DetectedProject],
        managed: &[(String, u32, u64)],
    ) -> io::Result<RuntimeSnapshot> {
        if authorized.is_empty() {
            return Ok(RuntimeSnapshot {
                sampled_at: now_seconds(),
                projects: Vec::new(),
            });
        }
        let projects: Vec<RuntimeProject> = authorized
            .iter()
            .filter_map(|project| {
                Path::new(&project.path)
                    .canonicalize()
                    .ok()
                    .map(|path| RuntimeProject {
                        path: project.path.clone(),
                        node_script: project.node_script.clone(),
                        node_selector: project.node_selector.clone(),
                        root: normalize_path(&path.to_string_lossy()),
                        java: matches!(
                            project.kind,
                            ProjectKind::Maven | ProjectKind::MavenAndNode
                        ),
                        node: matches!(project.kind, ProjectKind::Node | ProjectKind::MavenAndNode),
                        main_classes: project.main_classes.clone(),
                    })
            })
            .collect();
        let mut cache_key: Vec<String> = projects
            .iter()
            .map(|p| {
                format!(
                    "{}|{:?}|{:?}|{}|{}|{:?}",
                    p.path, p.node_script, p.node_selector, p.java, p.node, p.main_classes
                )
            })
            .collect();
        cache_key.extend(
            managed
                .iter()
                .map(|(path, pid, started)| format!("managed:{path}:{pid}:{started}")),
        );
        let mut sampler = self
            .projects
            .lock()
            .map_err(|_| io::Error::other("Runtime sampler unavailable"))?;
        if let Some((time, cached_root, snapshot)) = &sampler.cached {
            if cached_root == &cache_key && time.elapsed() < Duration::from_secs(2) {
                return Ok(snapshot.clone());
            }
        }
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return Err(io::Error::other(
                "Process metrics unsupported on this platform",
            ));
        }
        sampler.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_memory()
                .with_cpu()
                .with_cmd(UpdateKind::Always)
                .with_cwd(UpdateKind::Always),
        );
        let observations: Vec<Observation> = sampler
            .system
            .processes()
            .iter()
            .map(|(pid, process)| {
                let stable = sampler.identities.get(&pid.as_u32()) == Some(&process.start_time());
                Observation {
                    pid: pid.as_u32(),
                    parent: process.parent().map(|pid| pid.as_u32()),
                    started: process.start_time(),
                    name: process.name().to_string_lossy().into_owned(),
                    cwd: process.cwd().map(|cwd| cwd.to_string_lossy().into_owned()),
                    args: process
                        .cmd()
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect(),
                    memory: process.memory(),
                    cpu: stable.then(|| process.cpu_usage()),
                }
            })
            .collect();
        sampler.identities = observations.iter().map(|p| (p.pid, p.started)).collect();
        let sampled_at = now_seconds();
        let cores = std::thread::available_parallelism().map_or(1, |count| count.get());
        let mut snapshot = RuntimeSnapshot {
            sampled_at,
            projects: aggregate_with_managed(&projects, &observations, sampled_at, cores, managed),
        };
        apply_listeners_with_processes(&mut snapshot.projects, ports::listeners(), &observations);
        sampler.cached = Some((Instant::now(), cache_key, snapshot.clone()));
        Ok(snapshot)
    }
}

#[cfg(test)]
fn apply_listeners(projects: &mut [ProjectRuntime], listeners: io::Result<ports::Listeners>) {
    apply_listeners_with_processes(projects, listeners, &[]);
}

fn apply_listeners_with_processes(projects: &mut [ProjectRuntime], listeners: io::Result<ports::Listeners>, processes: &[Observation]) {
    let by_pid: HashMap<u32, &Observation> = processes.iter().map(|process| (process.pid, process)).collect();
    for project in projects {
        match &listeners {
            Ok(listeners) => {
                let mut details = Vec::new();
                for pid in &project.pids {
                    if let Some(ports) = listeners.get(pid) {
                        for port in ports {
                            let process = by_pid.get(pid).copied();
                            let (purpose, source) = presentation::port_purpose(process, *port);
                            details.push(ProjectListener { pid: *pid, port: *port, purpose: purpose.into(), purpose_source: source.into() });
                        }
                    }
                }
                details.sort_by_key(|listener| (listener.port, listener.pid));
                project.listeners = Some(details);
                let ports: std::collections::BTreeSet<_> = project
                    .pids
                    .iter()
                    .filter_map(|pid| listeners.get(pid))
                    .flatten()
                    .copied()
                    .collect();
                project.listening_ports = Some(ports.into_iter().collect());
                project.port_error = None;
            }
            Err(err) => {
                project.listeners = None;
                project.listening_ports = None;
                project.port_error = Some(err.to_string());
            }
        }
    }
}

fn normalized_self_cpu(cpu: Option<f32>, cores: usize) -> Option<f32> {
    cpu.filter(|value| value.is_finite())
        .map(|value| (value / cores.max(1) as f32).clamp(0., 100.))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalize_path(value: &str) -> String {
    let value = value.replace('\\', "/");
    let value = value
        .strip_prefix("//?/")
        .unwrap_or(&value)
        .trim_end_matches('/');
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.to_owned()
    }
}

fn path_within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn launch_path(target: &str, cwd: Option<&str>) -> Option<String> {
    let target = normalize_path(target);
    let absolute = target.starts_with('/') || target.as_bytes().get(1) == Some(&b':');
    let target = if absolute {
        target
    } else {
        format!("{}/{target}", normalize_path(cwd?))
    };
    let mut clean = std::path::PathBuf::new();
    for component in Path::new(&target).components() {
        match component {
            std::path::Component::ParentDir => {
                clean.pop();
            }
            std::path::Component::CurDir => {}
            component => clean.push(component.as_os_str()),
        }
    }
    Some(normalize_path(&clean.to_string_lossy()))
}
fn node_matches(process: &Observation, project: &RuntimeProject) -> bool {
    if let Some(script) = &project.node_script {
        let cwd_matches = process
            .cwd
            .as_deref()
            .is_some_and(|cwd| normalize_path(cwd) == project.root);
        let cli = process.args.iter().any(|arg| {
            [
                "npm-cli.js",
                "pnpm.cjs",
                "pnpm.mjs",
                "pnpm.js",
                "yarn.js",
                "yarn.cjs",
            ]
            .contains(&normalize_path(arg).rsplit('/').next().unwrap_or(""))
        });
        return cwd_matches
            && cli
            && (process.args.windows(2).any(|args| {
                ["run", "run-script"].contains(&args[0].as_str()) && &args[1] == script
            }) || process.args.last() == Some(script)
                || project.node_selector.as_ref().is_some_and(|selector| {
                    process
                        .args
                        .iter()
                        .any(|arg| arg == &format!("--project={selector}"))
                        || process
                            .args
                            .windows(2)
                            .any(|args| args[0] == "--project" && &args[1] == selector)
                }));
    }
    let mut index = 0;
    if process.args.first().is_some_and(|s| {
        ["node", "node.exe"].contains(&normalize_path(s).rsplit('/').next().unwrap_or(""))
    }) {
        index = 1;
    }
    while let Some(arg) = process.args.get(index) {
        if ["-e", "--eval", "-p", "--print", "--run"].contains(&arg.as_str()) {
            return false;
        }
        if [
            "-r",
            "--require",
            "--import",
            "--loader",
            "--experimental-loader",
            "--conditions",
            "-C",
            "--inspect-port",
            "--icu-data-dir",
            "--openssl-config",
            "--redirect-warnings",
        ]
        .contains(&arg.as_str())
        {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return launch_path(arg, process.cwd.as_deref())
            .is_some_and(|path| path_within(&path, &project.root));
    }
    false
}

enum JavaTarget<'a> {
    Jar(&'a str),
    Class(&'a str),
}
fn java_target(args: &[String]) -> Option<JavaTarget<'_>> {
    let mut index = 0;
    if args.first().is_some_and(|s| {
        ["java", "java.exe", "javaw", "javaw.exe"]
            .contains(&normalize_path(s).rsplit('/').next().unwrap_or(""))
    }) {
        index = 1;
    }
    while let Some(arg) = args.get(index) {
        if arg == "-jar" {
            return args.get(index + 1).map(|s| JavaTarget::Jar(s));
        }
        if arg == "-m" || arg == "--module" {
            return args
                .get(index + 1)
                .and_then(|s| s.split_once('/'))
                .map(|(_, class)| JavaTarget::Class(class));
        }
        if let Some(module) = arg.strip_prefix("--module=") {
            return module
                .split_once('/')
                .map(|(_, class)| JavaTarget::Class(class));
        }
        if [
            "-cp",
            "-classpath",
            "--class-path",
            "-p",
            "--module-path",
            "--upgrade-module-path",
            "--add-opens",
            "--add-exports",
            "--add-reads",
            "--patch-module",
            "--add-modules",
            "--limit-modules",
            "--enable-native-access",
            "--source",
        ]
        .contains(&arg.as_str())
        {
            index += 2;
            continue;
        }
        if arg.starts_with('-') || arg.starts_with('@') {
            index += 1;
            continue;
        }
        return Some(JavaTarget::Class(arg));
    }
    None
}
fn java_matches(process: &Observation, project: &RuntimeProject) -> bool {
    match java_target(&process.args) {
        Some(JavaTarget::Class(class)) => project.main_classes.iter().any(|main| main == class),
        Some(JavaTarget::Jar(jar)) => launch_path(jar, process.cwd.as_deref())
            .is_some_and(|path| path_within(&path, &project.root)),
        None => false,
    }
}
#[cfg(test)]
fn aggregate(
    projects: &[RuntimeProject],
    processes: &[Observation],
    now: u64,
    cores: usize,
) -> Vec<ProjectRuntime> {
    aggregate_with_managed(projects, processes, now, cores, &[])
}

fn aggregate_with_managed(
    projects: &[RuntimeProject],
    processes: &[Observation],
    now: u64,
    cores: usize,
    managed: &[(String, u32, u64)],
) -> Vec<ProjectRuntime> {
    let by_pid: HashMap<u32, &Observation> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut owners: HashMap<u32, usize> = HashMap::new();
    for (path, pid, started) in managed {
        if by_pid
            .get(pid)
            .is_some_and(|process| process.started == *started)
        {
            if let Some(index) = projects.iter().position(|project| {
                let key = project.node_script.as_ref().map_or_else(
                    || project.path.clone(),
                    |script| format!("node:{}#{script}", project.path),
                );
                &key == path
            }) {
                owners.insert(*pid, index);
            }
        }
    }
    for process in processes {
        if owners.contains_key(&process.pid) {
            continue;
        }
        if !["java", "java.exe", "javaw", "javaw.exe", "node", "node.exe"]
            .contains(&process.name.to_ascii_lowercase().as_str())
        {
            continue;
        }
        // A main class can occur in multiple checkouts. Prefer launch-path
        // evidence, and leave equally strong candidates unassigned.
        let candidates: Vec<_> = projects
            .iter()
            .enumerate()
            .filter(|(_, project)| {
                if ["java", "java.exe", "javaw", "javaw.exe"]
                    .contains(&process.name.to_ascii_lowercase().as_str())
                {
                    return project.java && java_matches(process, project);
                }
                project.node && node_matches(process, project)
            })
            .map(|(index, project)| {
                let cwd_matches = process
                    .cwd
                    .as_deref()
                    .and_then(|cwd| launch_path(cwd, None))
                    .is_some_and(|cwd| path_within(&cwd, &project.root));
                let classpath_matches = java_classpath(process).is_some_and(|classpath| {
                    classpath
                        .split(if cfg!(windows) { ';' } else { ':' })
                        .any(|entry| {
                            launch_path(entry, process.cwd.as_deref())
                                .is_some_and(|path| path_within(&path, &project.root))
                        })
                });
                let class_launch = ["java", "java.exe", "javaw", "javaw.exe"]
                    .contains(&process.name.to_ascii_lowercase().as_str())
                    && matches!(java_target(&process.args), Some(JavaTarget::Class(_)));
                let specificity = if cwd_matches || !class_launch {
                    project.root.len()
                } else {
                    0
                };
                (index, (cwd_matches, classpath_matches, specificity))
            })
            .collect();
        let best = candidates.iter().map(|(_, score)| *score).max();
        let mut winners = candidates.iter().filter(|(_, score)| Some(*score) == best);
        let owner = winners
            .next()
            .map(|(index, _)| *index)
            .filter(|_| winners.next().is_none());
        if let Some(owner) = owner {
            owners.insert(process.pid, owner);
        }
    }
    // Include descendants (e.g. esbuild), without overriding directly matched
    // child projects. Guard PID reuse and cycles. No process is counted twice.
    for process in processes {
        if owners.contains_key(&process.pid) {
            continue;
        }
        let mut current = process;
        let mut visited = std::collections::HashSet::new();
        while let Some(parent) = current.parent.and_then(|pid| by_pid.get(&pid).copied()) {
            if parent.started > current.started || !visited.insert(parent.pid) {
                break;
            }
            if let Some(owner) = owners.get(&parent.pid).copied() {
                owners.insert(process.pid, owner);
                break;
            }
            current = parent;
        }
    }
    projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            let members: Vec<_> = processes
                .iter()
                .filter(|p| owners.get(&p.pid) == Some(&index))
                .collect();
            let mut pids: Vec<_> = members.iter().map(|p| p.pid).collect();
            pids.sort_unstable();
            let mut java_pids: Vec<_> = members
                .iter()
                .filter(|p| {
                    matches!(
                        p.name.to_ascii_lowercase().as_str(),
                        "java" | "java.exe" | "javaw" | "javaw.exe"
                    ) && !p.args.iter().any(|arg| arg.contains("org.codehaus.plexus"))
                })
                .map(|p| p.pid)
                .collect();
            java_pids.sort_unstable();
            let (primary_pids, process_details) = presentation::process_details(&members, project, managed);
            let has_members = !members.is_empty();
            let memory = members
                .iter()
                .map(|p| p.memory)
                .fold(0u64, u64::saturating_add);
            let cpu = members.iter().try_fold(0.0f32, |total, p| {
                p.cpu.filter(|cpu| cpu.is_finite()).map(|cpu| total + cpu)
            });
            ProjectRuntime {
                project_path: project.path.clone(),
                node_script: project.node_script.clone(),
                node_pids: members
                    .iter()
                    .filter(|p| {
                        ["node", "node.exe", "pnpm", "pnpm.exe"]
                            .contains(&p.name.to_ascii_lowercase().as_str())
                    })
                    .map(|p| p.pid)
                    .collect(),
                managed_state: None,
                listening_ports: None,
                port_error: None,
                java_pids,
                primary_pids,
                processes: process_details,
                listeners: None,
                pids,
                memory_bytes: (has_members && memory > 0).then_some(memory),
                cpu_percent: has_members
                    .then_some(cpu)
                    .flatten()
                    .map(|cpu| (cpu / cores.max(1) as f32).clamp(0.0, 100.0)),
                uptime_seconds: members
                    .iter()
                    .filter(|p| p.started > 0)
                    .map(|p| now.saturating_sub(p.started))
                    .max(),
            }
        })
        .collect()
}

fn java_classpath(process: &Observation) -> Option<&str> {
    let JavaTarget::Class(class) = java_target(&process.args)? else {
        return None;
    };
    let mut index = 0;
    while let Some(arg) = process.args.get(index) {
        if arg == class {
            break;
        }
        if ["-cp", "-classpath", "--class-path"].contains(&arg.as_str()) {
            return process.args.get(index + 1).map(String::as_str);
        }
        if let Some(value) = arg.strip_prefix("--class-path=") {
            return Some(value);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_listeners_follow_project_identity_not_parent_editor_or_port_number() {
        let projects = vec![project("app", "d:/code/中文 服务")];
        let external = process(
            10,
            Some(900),
            "java.exe",
            r"d:\code\中文 服务\target\server.jar",
        );
        let worker = process(11, Some(10), "worker.exe", "");
        let unrelated = process(12, None, "java.exe", "d:/other/server.jar");
        let mut metrics = aggregate(&projects, &[external, worker, unrelated], 20, 1);
        let listeners = ports::parse_windows_listeners(
            "TCP 0.0.0.0:18080 0.0.0.0:0 LISTENING 10\n\
             TCP [::]:18080 [::]:0 LISTENING 10\n\
             TCP 127.0.0.1:15005 0.0.0.0:0 LISTENING 11\n\
             TCP 0.0.0.0:19090 0.0.0.0:0 LISTENING 12",
        );
        apply_listeners(&mut metrics, Ok(listeners));
        assert_eq!(metrics[0].pids, vec![10, 11]);
        assert_eq!(metrics[0].listening_ports, Some(vec![15005, 18080]));
        assert_eq!(metrics[0].port_error, None);
        let mut exited = aggregate(&projects, &[], 21, 1);
        apply_listeners(&mut exited, Ok(HashMap::new()));
        assert_eq!(exited[0].listening_ports, Some(vec![]));
    }

    #[test]
    fn listener_failure_is_distinct_from_no_listeners_and_recovers() {
        let projects = vec![project("app", "d:/code/app")];
        let mut metrics = aggregate(&projects, &[], 20, 1);
        apply_listeners(
            &mut metrics,
            Err(io::Error::new(io::ErrorKind::TimedOut, "采集超时")),
        );
        assert_eq!(metrics[0].listening_ports, None);
        assert_eq!(metrics[0].port_error.as_deref(), Some("采集超时"));
        assert!(metrics[0].listeners.is_none());
        let json = serde_json::to_value(&metrics[0]).unwrap();
        assert!(json["listeningPorts"].is_null());
        assert_eq!(json["portError"], "采集超时");
        apply_listeners(&mut metrics, Ok(HashMap::new()));
        assert_eq!(metrics[0].listening_ports, Some(vec![]));
        assert!(metrics[0].listeners.as_ref().unwrap().is_empty());
        assert_eq!(metrics[0].port_error, None);
    }

    #[test]
    fn listener_details_keep_pid_ownership_and_only_label_evidenced_purposes() {
        let projects = vec![project("app", "d:/code/app")];
        let mut main = process(10, None, "java.exe", "d:/code/app/app.jar");
        main.args.push("--server.port=8001".into());
        let child = process(11, Some(10), "worker.exe", "");
        let processes = [main, child];
        let mut metrics = aggregate(&projects, &processes, 20, 1);
        apply_listeners_with_processes(&mut metrics, Ok(ports::parse_windows_listeners(
            "TCP 0.0.0.0:8001 0.0.0.0:0 LISTENING 10\n\
             TCP [::]:8001 [::]:0 LISTENING 10\n\
             TCP 127.0.0.1:8001 0.0.0.0:0 LISTENING 11\n\
             TCP [::]:50051 [::]:0 LISTENING 10"
        )), &processes);
        let details = metrics[0].listeners.as_ref().unwrap();
        assert_eq!(details.len(), 3);
        assert_eq!((details[0].pid, details[0].port, details[0].purpose.as_str()), (10, 8001, "应用服务"));
        assert_eq!((details[1].pid, details[1].port, details[1].purpose.as_str()), (11, 8001, "用途未知"));
        assert_eq!((details[2].pid, details[2].port, details[2].purpose_source.as_str()), (10, 50051, "unknown"));
        assert_eq!(metrics[0].primary_pids, vec![10]);
        assert_eq!(metrics[0].listening_ports, Some(vec![8001, 50051]));
        let json = serde_json::to_value(&metrics[0]).unwrap();
        assert_eq!(json["primaryPids"], serde_json::json!([10]));
        assert_eq!(json["processes"][1]["name"], "worker.exe");
        assert!(json["processes"][0].get("args").is_none());
        let exited = aggregate(&projects, &[], 21, 1);
        assert!(exited[0].primary_pids.is_empty());
        assert!(exited[0].processes.is_empty());
    }

    #[test]
    fn managed_identity_is_applied_before_children_metrics_and_ports() {
        let projects = vec![project("app", "d:/code/app")];
        let mut java = process(10, None, "java.exe", "");
        // IDE/Maven argument files can hide the actual main class.
        java.args = vec!["@hidden-arguments".into()];
        let child = process(11, Some(10), "worker.exe", "");
        let processes = vec![java, child];
        let managed = vec![("app".into(), 10, 10)];
        let mut metrics = aggregate_with_managed(&projects, &processes, 20, 1, &managed);
        apply_listeners(
            &mut metrics,
            Ok(ports::parse_windows_listeners(
                "TCP 0.0.0.0:18080 0.0.0.0:0 LISTENING 10\n\
             TCP 127.0.0.1:15005 0.0.0.0:0 LISTENING 11",
            )),
        );
        assert_eq!(metrics[0].pids, vec![10, 11]);
        assert_eq!(metrics[0].memory_bytes, Some(2048));
        assert_eq!(metrics[0].listening_ports, Some(vec![15005, 18080]));
        assert_eq!(metrics[0].primary_pids, vec![10]);
        let stale = vec![("app".into(), 10, 9)];
        assert!(
            aggregate_with_managed(&projects, &processes, 20, 1, &stale)[0]
                .pids
                .is_empty()
        );
    }

    #[test]
    fn duplicate_main_classes_require_launch_path_evidence() {
        let mut first = project("first", "d:/first");
        let mut second = project("second", "d:/second-longer");
        first.main_classes = vec!["example.Application".into()];
        second.main_classes = first.main_classes.clone();
        let projects = vec![first, second];
        let mut java = process(10, None, "java.exe", "");
        java.args = vec!["java.exe".into(), "example.Application".into()];
        let ambiguous = aggregate(&projects, &[java.clone()], 20, 1);
        assert!(ambiguous.iter().all(|project| project.pids.is_empty()));
        java.cwd = Some("d:/first".into());
        let resolved = aggregate(&projects, &[java.clone()], 20, 1);
        assert_eq!(resolved[0].pids, vec![10]);
        assert!(resolved[1].pids.is_empty());
        java.cwd = Some("d:/".into());
        java.args
            .insert(1, "--class-path=second-longer/target/classes".into());
        let resolved = aggregate(&projects, &[java.clone()], 20, 1);
        assert!(resolved[0].pids.is_empty());
        assert_eq!(resolved[1].pids, vec![10]);
        java.args[1] = format!(
            "--class-path=first/target/classes{}second-longer/target/classes",
            if cfg!(windows) { ';' } else { ':' },
        );
        assert!(
            aggregate(&projects, &[java], 20, 1)
                .iter()
                .all(|project| project.pids.is_empty())
        );
    }

    #[test]
    fn self_cpu_is_normalized_and_unknown_samples_are_not_zero() {
        assert_eq!(normalized_self_cpu(None, 8), None);
        assert_eq!(normalized_self_cpu(Some(f32::NAN), 8), None);
        assert_eq!(normalized_self_cpu(Some(400.), 8), Some(50.));
        assert_eq!(normalized_self_cpu(Some(-1.), 8), Some(0.));
        assert_eq!(normalized_self_cpu(Some(900.), 8), Some(100.));
        assert_eq!(normalized_self_cpu(Some(10.), 0), Some(10.));
    }
    #[test]
    fn self_sampler_only_reports_current_process_without_a_workspace() {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return;
        }
        let runtime = RuntimeService::default();
        let first = runtime.self_snapshot().unwrap();
        assert_eq!(first.pid, std::process::id());
        assert!(first.memory_bytes.is_some_and(|memory| memory > 0));
        assert_eq!(first.cpu_percent, None);
        let cached = runtime.self_snapshot().unwrap();
        assert_eq!(first.sampled_at, cached.sampled_at);
        assert_eq!(first.memory_bytes, cached.memory_bytes);
        assert!(
            runtime
                .snapshot_for_projects(&[])
                .unwrap()
                .projects
                .is_empty()
        );
        assert_eq!(runtime.self_snapshot().unwrap().pid, first.pid);
    }
    pub(super) fn project(name: &str, root: &str) -> RuntimeProject {
        RuntimeProject {
            path: name.into(),
            node_script: None,
            node_selector: None,
            root: root.into(),
            java: true,
            node: true,
            main_classes: Vec::new(),
        }
    }
    pub(super) fn process(pid: u32, parent: Option<u32>, name: &str, arg: &str) -> Observation {
        Observation {
            pid,
            parent,
            started: 10,
            name: name.into(),
            cwd: None,
            args: if name.starts_with("java") {
                vec!["-jar".into(), arg.into()]
            } else {
                vec![arg.into()]
            },
            memory: 1024,
            cpu: Some(10.0),
        }
    }
    #[test]
    fn windows_paths_and_segment_boundaries() {
        let root = "d:/code/app";
        assert!(path_within(
            &launch_path(r"d:\code\app\node_modules\vite\bin\vite.js", None).unwrap(),
            root
        ));
        assert!(!path_within(
            &launch_path(r"d:\code\app-copy\index.js", None).unwrap(),
            root
        ));
        assert!(!path_within(
            &launch_path("../other/server.jar", Some(root)).unwrap(),
            root
        ));
        assert_eq!(normalize_path(r"\\?\d:\code\app"), root);
    }
    #[test]
    fn java_operations_exclude_console_hosts_and_maven_launchers() {
        let projects = vec![project("app", "d:/code/app")];
        let mut launcher = process(3, Some(1), "java.exe", "");
        launcher.args = vec!["org.codehaus.plexus.classworlds.launcher.Launcher".into()];
        let processes = vec![
            process(1, None, "java.exe", "d:/code/app/app.jar"),
            process(2, Some(1), "conhost.exe", ""),
            launcher,
        ];
        let metrics = aggregate(&projects, &processes, 20, 2);
        assert_eq!(metrics[0].pids, vec![1, 2, 3]);
        assert_eq!(metrics[0].java_pids, vec![1]);
    }
    #[test]
    fn children_aggregate_once_and_exited_processes_clear_metrics() {
        let projects = vec![project("app", "d:/code/app")];
        let processes = vec![
            process(1, None, "node.exe", "d:/code/app/start.js"),
            process(2, Some(1), "esbuild.exe", ""),
            process(3, None, "java.exe", "d:/other/main.jar"),
        ];
        let metrics = aggregate(&projects, &processes, 20, 2);
        assert_eq!(metrics[0].pids, vec![1, 2]);
        assert!(metrics[0].java_pids.is_empty());
        assert_eq!(metrics[0].memory_bytes, Some(2048));
        assert_eq!(metrics[0].cpu_percent, Some(10.0));
        assert_eq!(metrics[0].uptime_seconds, Some(10));
        let cleared = aggregate(&projects, &[], 20, 2);
        assert!(cleared[0].pids.is_empty());
        assert_eq!(cleared[0].memory_bytes, None);
    }
    #[test]
    fn nested_projects_and_pid_reuse() {
        let projects = vec![project("root", "d:/code"), project("child", "d:/code/app")];
        let mut parent = process(1, None, "node", "d:/code/app/start.js");
        parent.started = 30;
        let child = process(2, Some(1), "worker", "");
        let metrics = aggregate(&projects, &[parent, child], 40, 1);
        assert!(metrics[0].pids.is_empty());
        assert_eq!(metrics[1].pids, vec![1]);
    }
    #[test]
    fn warming_up_cpu_is_not_zero() {
        let projects = vec![project("app", "d:/app")];
        let mut p = process(1, None, "java", "d:/app/app.jar");
        p.cpu = None;
        assert_eq!(aggregate(&projects, &[p], 20, 1)[0].cpu_percent, None);
    }
    #[test]
    fn java_classpath_dependencies_are_not_running_applications() {
        let mut system = project("system", "d:/code/system");
        system.main_classes = vec!["example.SystemApplication".into()];
        system.node = false;
        let mut dependency = project("notice", "d:/code/notice");
        dependency.main_classes = vec!["example.NoticeApplication".into()];
        dependency.node = false;
        let mut java = process(1, None, "java.exe", "");
        java.cwd = Some("d:/code".into());
        java.args = vec![
            "java.exe",
            "-Dspring.profiles.active=dev",
            "-classpath",
            "d:/code/notice/target/classes;d:/code/system/target/classes;d:/repository/spring.jar",
            "example.SystemApplication",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut tooling = process(2, None, "java.exe", "");
        tooling.cwd = Some("d:/code/system".into());
        tooling.args = vec![
            "java.exe",
            "-cp",
            "d:/code/system/target/classes",
            "org.eclipse.equinox.launcher.Main",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let metrics = aggregate(&[system, dependency], &[java, tooling], 20, 1);
        assert_eq!(metrics[0].pids, vec![1]);
        assert!(metrics[1].pids.is_empty());
    }
    #[test]
    fn java_jar_launch_uses_executable_jar_not_classpath_or_unrelated_cwd() {
        let mut app = project("app", "d:/code/app");
        app.node = false;
        let mut p = process(1, None, "java", "d:/other/server.jar");
        p.cwd = Some("d:/code/app".into());
        assert!(aggregate(&[app], &[p], 20, 1)[0].pids.is_empty());
        let mut app = project("app", "d:/code/app");
        app.node = false;
        let mut p = process(2, None, "java", "target/app.jar");
        p.cwd = Some("d:/code/app".into());
        assert_eq!(aggregate(&[app], &[p], 20, 1)[0].pids, vec![2]);
    }
    #[test]
    fn node_uses_executed_script_not_cwd_or_imported_dependencies() {
        let app = project("app", "d:/code/app");
        let mut tool = process(1, None, "node.exe", "d:/tools/language-server.js");
        tool.cwd = Some("d:/code/app".into());
        tool.args = vec![
            "node.exe",
            "--require",
            "d:/code/app/node_modules/register.js",
            "d:/tools/language-server.js",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut server = process(2, None, "node.exe", "node_modules/vite/bin/vite.js");
        server.cwd = Some("d:/code/app".into());
        let worker = process(3, Some(2), "esbuild.exe", "");
        assert_eq!(
            aggregate(&[app], &[tool, server, worker], 20, 1)[0].pids,
            vec![2, 3]
        );
    }
    #[test]
    fn aggregate_node_services_use_exact_script_or_project_selector() {
        let mut a = project("root", "d:/code/中文 space");
        a.java = false;
        a.node_script = Some("system".into());
        a.node_selector = Some("extreme-system".into());
        let mut b = project("root", "d:/code/中文 space");
        b.java = false;
        b.node_script = Some("file".into());
        b.node_selector = Some("extreme-file".into());
        let mut first = process(1, None, "node.exe", "");
        first.cwd = Some("d:/code/中文 space".into());
        first.args = vec![
            "node.exe".into(),
            "d:/tools/node_modules/npm/bin/npm-cli.js".into(),
            "run".into(),
            "serve".into(),
            "--project=extreme-system".into(),
        ];
        let child = process(
            2,
            Some(1),
            "node.exe",
            "d:/code/中文 space/node_modules/vite/bin/vite.js",
        );
        let mut second = first.clone();
        second.pid = 3;
        second.args = vec![
            "node.exe".into(),
            "d:/tools/node_modules/npm/bin/npm-cli.js".into(),
            "run".into(),
            "file".into(),
        ];
        let child2 = process(4, Some(3), "esbuild.exe", "");
        let mut unrelated = first.clone();
        unrelated.pid = 5;
        unrelated.args = vec![
            "node.exe".into(),
            "d:/tools/node_modules/npm/bin/npm-cli.js".into(),
            "run".into(),
            "serve".into(),
            "--project=extreme-system-copy".into(),
        ];
        let metrics = aggregate(&[a, b], &[first, child, second, child2, unrelated], 20, 2);
        assert_eq!(metrics[0].pids, vec![1, 2]);
        assert_eq!(metrics[1].pids, vec![3, 4]);
        assert_eq!(metrics[0].node_script.as_deref(), Some("system"));
        assert_eq!(metrics[0].node_pids, vec![1, 2]);
    }
}
