//! Process roles and conservative port labels shared by Web and MCP.
use super::{Observation, ProjectProcess, RuntimeProject, java_matches};
use std::collections::HashSet;

fn launcher(process: &Observation) -> bool {
    process.args.iter().any(|arg| {
        super::maven::is_launcher_arg(arg) || ["npm-cli.js", "pnpm.cjs", "pnpm.mjs", "pnpm.js", "yarn.js", "yarn.cjs"]
            .iter().any(|name| arg.replace('\\', "/").rsplit('/').next() == Some(*name))
    })
}

pub(super) fn process_details(members: &[&Observation], project: &RuntimeProject, managed: &[(String, u32, u64)]) -> (Vec<u32>, Vec<ProjectProcess>) {
    let candidates: HashSet<u32> = members.iter().filter(|process| {
        if launcher(process) { return false; }
        match process.name.to_ascii_lowercase().as_str() {
            "java" | "java.exe" | "javaw" | "javaw.exe" => java_matches(process, project)
                || managed.iter().any(|(_, pid, started)| *pid == process.pid && *started == process.started),
            "node" | "node.exe" => true,
            _ => false,
        }
    }).map(|process| process.pid).collect();
    let mut primary: Vec<u32> = members.iter().filter(|process| {
        if !candidates.contains(&process.pid) { return false; }
        let mut current = **process;
        let mut visited = HashSet::new();
        while let Some(parent) = current.parent.and_then(|pid| members.iter().find(|member| member.pid == pid).copied()) {
            if parent.started > current.started || !visited.insert(parent.pid) { break; }
            if candidates.contains(&parent.pid) { return false; }
            current = parent;
        }
        true
    }).map(|process| process.pid).collect();
    primary.sort_unstable();
    let mut details: Vec<_> = members.iter().map(|process| ProjectProcess {
        pid: process.pid,
        parent_pid: process.parent,
        name: process.name.clone(),
        role: if primary.contains(&process.pid) { "primary" } else if launcher(process) { "launcher" } else { "auxiliary" }.into(),
    }).collect();
    details.sort_by_key(|process| process.pid);
    (primary, details)
}

pub(super) fn port_purpose(process: Option<&Observation>, port: u16) -> (&'static str, &'static str) {
    if let Some(process) = process {
        if process.args.iter().any(|arg| arg.contains("arthas-boot") && arg.ends_with(".jar")) {
            for args in process.args.windows(2) {
                if args[1].parse::<u16>().ok() == Some(port) {
                    match args[0].as_str() {
                        "--http-port" => return ("Arthas HTTP", "command_line"),
                        "--telnet-port" => return ("Arthas Telnet", "command_line"),
                        _ => {}
                    }
                }
            }
        }
        for arg in &process.args {
            if let Some(options) = arg.strip_prefix("-agentlib:jdwp=").or_else(|| arg.strip_prefix("-Xrunjdwp:")) {
                let debug_port = options.split(',').find_map(|option| option.strip_prefix("address="))
                    .and_then(|address| address.rsplit(':').next()).and_then(|value| value.parse::<u16>().ok());
                if options.split(',').any(|option| option == "server=y") && debug_port == Some(port) {
                    return ("JVM 调试", "command_line");
                }
            }
            for (prefix, purpose) in [("--server.port=", "应用服务"), ("-Dserver.port=", "应用服务"),
                ("--management.server.port=", "管理端口"), ("-Dmanagement.server.port=", "管理端口"),
                ("--grpc.server.port=", "gRPC"), ("-Dgrpc.server.port=", "gRPC")] {
                if arg.strip_prefix(prefix).and_then(|value| value.parse::<u16>().ok()) == Some(port) {
                    return (purpose, "command_line");
                }
            }
        }
    }
    // Port numbers alone cannot identify the actual protocol.
    ("用途未知", "unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tests::{process, project};

    #[test]
    fn java_primary_excludes_windows_console_and_maven() {
        let app = project("app", "d:/code/中文 服务");
        let main = process(10, None, "java.exe", r"d:\code\中文 服务\app.jar");
        let console = process(11, Some(10), "conhost.exe", "");
        let mut maven = process(12, Some(10), "java.exe", "");
        maven.args = vec!["org.codehaus.plexus.classworlds.launcher.Launcher".into()];
        let (primary, details) = process_details(&[&main, &console, &maven], &app, &[]);
        assert_eq!(primary, vec![10]);
        assert_eq!(details[1].role, "auxiliary");
        assert_eq!(details[1].parent_pid, Some(10));
        assert_eq!(details[2].role, "launcher");
        assert!(process_details(&[], &app, &[]).0.is_empty());
        maven.args = vec![r"D:\工具 中文\DevflowMavenBridge.java".into()];
        let (primary, details) = process_details(&[&maven], &app, &[]);
        assert!(primary.is_empty());
        assert_eq!(details[0].role, "launcher");
    }

    #[test]
    fn node_primary_is_application_instead_of_package_manager_or_worker() {
        let app = project("app", "d:/code/app");
        let runner = process(10, None, "node.exe", "d:/npm/npm-cli.js");
        let main = process(11, Some(10), "node.exe", "d:/code/app/server.js");
        let worker = process(12, Some(11), "node.exe", "worker.js");
        assert_eq!(process_details(&[&runner, &main, &worker], &app, &[]).0, vec![11]);
    }

    #[test]
    fn purpose_requires_evidence_instead_of_conventional_port_numbers() {
        let mut main = process(10, None, "java.exe", "app.jar");
        main.args.extend(["-agentlib:jdwp=transport=dt_socket,server=y,address=*:5005".into(), "--server.port=8001".into()]);
        assert_eq!(port_purpose(Some(&main), 5005), ("JVM 调试", "command_line"));
        assert_eq!(port_purpose(Some(&main), 8001), ("应用服务", "command_line"));
        assert_eq!(port_purpose(Some(&main), 50051), ("用途未知", "unknown"));
        assert_eq!(port_purpose(None, 8080), ("用途未知", "unknown"));
    }
}
