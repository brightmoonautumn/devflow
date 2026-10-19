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
    java_operations: std::sync::Mutex<()>,
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
            java_operations: std::sync::Mutex::new(()),
        })
    }

    pub fn runtime_snapshot(&self) -> Result<runtime::RuntimeSnapshot, workspace::WorkspaceError> {
        let workspace = self.workspace.snapshot()?;
        let mut outputs = Vec::new();
        let mut managed = Vec::new();
        for project in &workspace.projects {
            if matches!(project.kind, workspace::ProjectKind::Node) {
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
            if let Some(metrics) = snapshot
                .projects
                .iter_mut()
                .find(|p| p.project_path == path)
            {
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
            .java_operations
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
            JavaOperation::Start {
                jvm_args,
                args,
                profiles,
                main_class,
            } => {
                if !pids.is_empty() {
                    return Err(workspace::WorkspaceError {
                        code: "conflict",
                        message: "项目已有运行进程，请先停止".into(),
                    });
                }
                self.java
                    .start(project, jvm_args, args, profiles, main_class)
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
}
