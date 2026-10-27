pub mod diagnose;
pub mod explorer;
pub mod qa;
pub mod runtime;
pub mod workspace;

/// Shared application engine; adapters never own separate workspace state.
pub struct DevFlowEngine {
    pub workspace: workspace::WorkspaceService,
    pub runtime: runtime::RuntimeService,
    pub java: runtime::java::JavaService,
    pub node: runtime::node::NodeService,
    diagnosis: std::sync::Arc<runtime::diagnosis::DiagnosisService>,
    qa: std::sync::Arc<qa::QaService>,
    runtime_operations: std::sync::Mutex<()>,
}

impl DevFlowEngine {
    pub fn new(
        roots: Vec<std::path::PathBuf>,
        initial: Option<std::path::PathBuf>,
    ) -> Result<Self, workspace::WorkspaceError> {
        Ok(Self {
            workspace: workspace::WorkspaceService::new(roots, initial)?,
            runtime: runtime::RuntimeService::default(),
            java: runtime::java::JavaService::default(),
            node: runtime::node::NodeService::default(),
            diagnosis: Default::default(),
            qa: Default::default(),
            runtime_operations: std::sync::Mutex::new(()),
        })
    }

    pub fn runtime_snapshot(&self) -> Result<runtime::RuntimeSnapshot, workspace::WorkspaceError> {
        self.runtime_snapshot_for(&self.workspace)
    }

    pub(crate) fn runtime_snapshot_for(&self, workspace: &workspace::WorkspaceService) -> Result<runtime::RuntimeSnapshot, workspace::WorkspaceError> {
        let workspace = workspace.snapshot()?;
        let mut outputs = Vec::new();
        let mut managed = Vec::new();
        for project in &workspace.projects {
            if matches!(project.kind, workspace::ProjectKind::Node) {
                let output = self.node.output(project, u64::MAX, None)?;
                if let Some((pid, started)) = self.node.managed_identity(project) {
                    managed.push((project.runtime_key(), pid, started));
                }
                outputs.push((project.runtime_key(), output));
                continue;
            }
            let output = self.java.output(project, u64::MAX, None)?;
            if matches!(output.state.as_str(), "starting" | "running") {
                if let Some((pid, started)) = self.java.managed_identity(project) {
                    if output.pid == Some(pid) {
                        managed.push((project.path.clone(), pid, started));
                    }
                }
            }
            outputs.push((project.path.clone(), output));
        }
        let mut snapshot = self
            .runtime
            .snapshot_with_managed(&workspace.projects, &managed)?;
        for (path, output) in outputs {
            if let Some(metrics) = snapshot.projects.iter_mut().find(|p| {
                p.node_script.as_ref().map_or_else(
                    || p.project_path.clone(),
                    |s| format!("node:{}#{s}", p.project_path),
                ) == path
            }) {
                if output.state != "external" {
                    metrics.managed_state = Some(output.state.clone());
                }
            }
        }
        Ok(snapshot)
    }

    pub fn java_execute(
        &self,
        request: runtime::java::JavaRequest,
    ) -> Result<runtime::java::JavaOutput, workspace::WorkspaceError> {
        use runtime::java::JavaOperation;
        let _operation = self
            .runtime_operations
            .lock()
            .map_err(|_| workspace::WorkspaceError {
                code: "unavailable",
                message: "运行服务暂不可用".into(),
            })?;
        let project = self
            .workspace
            .runtime_project(&request.project_path, Some(request.expected_revision))?;
        self.runtime.invalidate();
        let observed = self.runtime_snapshot()?;
        let pids = observed
            .projects
            .iter()
            .find(|metrics| metrics.project_path == project.path)
            .map(|p| p.pids.as_slice())
            .unwrap_or_default();
        let target = |pid: u32| -> Result<(u32, u64), workspace::WorkspaceError> {
            if !pids.contains(&pid) {
                return Err(workspace::WorkspaceError {
                    code: "conflict",
                    message: "进程已退出或不属于所选项目，请刷新状态".into(),
                });
            }
            runtime::java::java_identity(pid)
        };
        let result = match request.operation {
            JavaOperation::ConfigureDependencies { dependencies } => {
                let boundary = self.workspace.java_build_boundary(&project.path, request.expected_revision)?;
                runtime::local_dependencies::save(&project.path, dependencies, &boundary)?;
                self.java.output(&project, 0, None)
            }
            JavaOperation::Start {
                jvm_args,
                args,
                profiles,
                main_class,
                maven_profiles,
                build_mode,
                force_build,
            } => {
                if !pids.is_empty() {
                    return Err(workspace::WorkspaceError {
                        code: "conflict",
                        message: "项目已有运行进程，请先停止".into(),
                    });
                }
                let boundary = self.workspace.java_build_boundary(&project.path, request.expected_revision)?;
                self.java.start_with_build(project, jvm_args, args, profiles, main_class, boundary,
                    runtime::java::BuildOptions { maven_profiles, build_mode, force_build })
            }
            JavaOperation::Restart { jvm_args, args, profiles, main_class, pid, maven_profiles, build_mode, force_build } => {
                let external = pid.map(target).transpose()?;
                let metrics = observed.projects.iter().find(|m| m.project_path == project.path);
                let managed = metrics.and_then(|m| m.managed_state.as_deref())
                    .is_some_and(|state| matches!(state, "starting" | "running"));
                if !managed && metrics.is_some_and(|m| m.java_pids.len() > 1) {
                    return Err(workspace::WorkspaceError {
                        code: "conflict",
                        message: "检测到多个关联进程，请先停止外部服务，再启动".into(),
                    });
                }
                let boundary = self.workspace.java_build_boundary(&project.path, request.expected_revision)?;
                self.java.restart_with_build(project, jvm_args, args, profiles, main_class, external, boundary,
                    runtime::java::BuildOptions { maven_profiles, build_mode, force_build })
            }
            JavaOperation::Stop { pid } => self.java.stop(&project, pid.map(target).transpose()?),
            JavaOperation::Attach { pid } => {
                let identity = target(pid)?;
                self.java.attach(&project, identity)
            }
        };
        self.runtime.invalidate();
        result
    }
    pub fn node_execute(
        &self,
        request: runtime::node::NodeRequest,
    ) -> Result<runtime::node::NodeOutput, workspace::WorkspaceError> {
        let _operation = self
            .runtime_operations
            .lock()
            .map_err(|_| workspace::WorkspaceError {
                code: "unavailable",
                message: "运行服务暂不可用".into(),
            })?;
        let project = self.workspace.node_runtime_project(
            &request.project_path,
            request.script.as_deref(),
            Some(request.expected_revision),
        )?;
        self.runtime.invalidate();
        let snapshot = self.runtime_snapshot()?;
        let metrics = snapshot
            .projects
            .iter()
            .find(|p| p.project_path == project.path && p.node_script == project.node_script);
        let target = |pid: u32| -> Result<(u32, u64), workspace::WorkspaceError> {
            if !metrics.is_some_and(|m| m.node_pids.contains(&pid)) {
                return Err(workspace::WorkspaceError {
                    code: "conflict",
                    message: "进程不属于所选服务或已退出".into(),
                });
            }
            runtime::node::node_identity(pid)
        };
        let result = match request.operation {
            runtime::node::NodeOperation::Start {
                package_manager,
                args,
            } => {
                if metrics.is_some_and(|m| !m.pids.is_empty()) {
                    return Err(workspace::WorkspaceError {
                        code: "conflict",
                        message: "该服务已有运行进程，请先停止".into(),
                    });
                }
                self.node
                    .start(project, request.script, package_manager, args)
            }
            runtime::node::NodeOperation::Restart { package_manager, args, pid } => {
                let external = pid.map(target).transpose()?;
                let managed = metrics.and_then(|m| m.managed_state.as_deref())
                    .is_some_and(|state| matches!(state, "starting" | "running"));
                if !managed && metrics.is_some_and(|m| m.node_pids.len() > 1) {
                    return Err(workspace::WorkspaceError {
                        code: "conflict",
                        message: "检测到多个关联进程，请先停止外部服务，再启动".into(),
                    });
                }
                self.node.restart(project, request.script, package_manager, args, external)
            }
            runtime::node::NodeOperation::Stop { pid } => {
                let target = pid.map(target).transpose()?;
                self.node.stop(&project, target)
            }
        };
        self.runtime.invalidate();
        result
    }
}
