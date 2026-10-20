//! Explicit capability-based read access to a frozen workspace configuration.
//! Guest dispatch exposes no mutation, raw service handle, or owner credential.
use super::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareCreateRequest { pub expected_revision: u64, pub lifetime_minutes: u64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareRevokeRequest { pub id: String }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInvitation {
    pub id: String, pub name: String, pub folders: Vec<String>, pub expires_at: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareCreated { pub invitation: ShareInvitation, pub token: String }

// Tagged, closed enum is the core read-only permission boundary for HTTP/MCP.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ShareReadRequest {
    Workspace,
    Children { path: String },
    File { path: String },
    Search {
        query: String,
        #[serde(default)] mode: search::SearchMode,
        #[serde(default)] case_sensitive: bool,
        #[serde(default)] whole_word: bool,
        #[serde(default)] regex: bool,
        #[serde(default)] include_glob: String,
        #[serde(default)] exclude_glob: String,
    },
    Runtime,
    Metrics,
    GitStatus,
    GitDiff { repository: String, version: String, path: String },
    Output { project_path: String, script: Option<String>, #[serde(default)] after: u64, run_id: Option<String> },
}
pub(super) struct ShareGrant {
    info: ShareInvitation, workspace: Arc<WorkspaceService>, deadline: Instant,
    revoked: Arc<AtomicBool>,
}
struct Lease { workspace: Arc<WorkspaceService>, deadline: Instant, revoked: Arc<AtomicBool> }
impl Lease {
    fn check(&self) -> Result<()> {
        if self.revoked.load(Ordering::Acquire) || Instant::now() >= self.deadline {
            return Err(error("unauthorized", "分享已过期或被撤销，请联系分享者"));
        }
        Ok(())
    }
}
impl WorkspaceService {
    pub fn share_create(&self, request: ShareCreateRequest, authenticated: bool) -> Result<ShareCreated> {
        if !authenticated { return Err(error("unauthorized", "需要本机管理令牌")); }
        if !(5..=1440).contains(&request.lifetime_minutes) { return Err(error("invalid_query", "分享有效期为 5–1440 分钟")); }
        let state = self.lock()?;
        if state.revision != request.expected_revision { return Err(error("conflict", "工作区已变化，请刷新后分享")); }
        let session = state.session.as_ref().ok_or_else(|| error("no_workspace", "请先打开工作区"))?.clone();
        // Resolve and authorize exactly the currently included folders. Never
        // inherit the owner's broader allowed roots or native-picker grants.
        let document = parse_document(&session.source)?;
        let mut roots = Vec::new();
        for folder in folder_entries(&document)? {
            let root = self.authorize(&resolve_folder(folder, &session.base)?)?;
            if !root.is_dir() { return Err(error("invalid_path", "分享目录不可用")); }
            if !roots.contains(&root) { roots.push(root); }
        }
        if roots.is_empty() { return Err(error("no_workspace", "当前工作区没有可分享的文件夹")); }
        let isolated = WorkspaceService::new(roots.clone(), None)?;
        isolated.lock()?.session = Some(session);
        let snapshot = isolated.snapshot()?;
        let duration = Duration::from_secs(request.lifetime_minutes * 60);
        let expires_at = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| error("unavailable", "系统时间无效"))?.as_secs() + duration.as_secs();
        let info = ShareInvitation { id: uuid::Uuid::new_v4().to_string(), name: snapshot.name,
            folders: roots.iter().map(|root| display_path(root)).collect(), expires_at };
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let mut shares = self.shares.lock().map_err(|_| error("unavailable", "分享服务暂不可用"))?;
        shares.retain(|_, share| Instant::now() < share.deadline);
        if shares.len() >= 16 { return Err(error("limit_exceeded", "最多保留 16 个有效邀请，请先撤销部分邀请")); }
        shares.insert(token.clone(), ShareGrant { info: info.clone(), workspace: Arc::new(isolated), deadline: Instant::now() + duration, revoked: Arc::new(AtomicBool::new(false)) });
        Ok(ShareCreated { invitation: info, token })
    }
    pub fn share_list(&self, authenticated: bool) -> Result<Vec<ShareInvitation>> {
        if !authenticated { return Err(error("unauthorized", "需要本机管理令牌")); }
        let mut shares = self.shares.lock().map_err(|_| error("unavailable", "分享服务暂不可用"))?;
        shares.retain(|_, share| Instant::now() < share.deadline);
        let mut values: Vec<_> = shares.values().map(|share| share.info.clone()).collect();
        values.sort_by_key(|share| share.expires_at);
        Ok(values)
    }
    pub fn share_revoke(&self, request: ShareRevokeRequest, authenticated: bool) -> Result<Value> {
        if !authenticated { return Err(error("unauthorized", "需要本机管理令牌")); }
        let mut shares = self.shares.lock().map_err(|_| error("unavailable", "分享服务暂不可用"))?;
        let token = shares.iter().find(|(_, share)| share.info.id == request.id).map(|(token, _)| token.clone());
        if let Some(token) = token { if let Some(share) = shares.remove(&token) { share.revoked.store(true, Ordering::Release); } }
        Ok(json!({"revoked":true}))
    }
    fn share_lease(&self, token: &str) -> Result<Lease> {
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err(error("unauthorized", "邀请无效、已过期或被撤销")); }
        let shares = self.shares.lock().map_err(|_| error("unavailable", "分享服务暂不可用"))?;
        let share = shares.get(token).ok_or_else(|| error("unauthorized", "邀请无效、已过期或被撤销"))?;
        let lease = Lease { workspace: share.workspace.clone(), deadline: share.deadline, revoked: share.revoked.clone() };
        lease.check()?;
        Ok(lease)
    }
}

impl crate::DevFlowEngine {
    pub fn share_read(&self, token: &str, request: ShareReadRequest) -> Result<Value> {
        let lease = self.workspace.share_lease(token)?;
        let workspace = &lease.workspace;
        // Frozen workspace revision is deliberately independent of the owner's
        // active workspace. Permission is rechecked after potentially slow reads.
        let revision = workspace.lock()?.revision;
        let value = match request {
            ShareReadRequest::Workspace => {
                let mut snapshot = workspace.snapshot()?;
                snapshot.workspace_file = None;
                json!(snapshot)
            }
            ShareReadRequest::Children { path } => json!(workspace.children(WorkspaceChildrenRequest { path, expected_revision: revision })?),
            ShareReadRequest::File { path } => {
                let mut file = workspace.read_file(WorkspaceFileReadRequest { path, expected_revision: revision }, true)?;
                file.readonly = true; file.readonly_reason = Some("只读分享".into()); json!(file)
            }
            ShareReadRequest::Search { query, mode, case_sensitive, whole_word, regex, include_glob, exclude_glob } => json!(workspace.search(WorkspaceSearchRequest { query, expected_revision: revision, mode, case_sensitive, whole_word, regex, include_glob, exclude_glob }, true)?),
            ShareReadRequest::Runtime => json!(self.runtime_snapshot_for(workspace)?),
            ShareReadRequest::Metrics => json!(self.runtime.self_snapshot()?),
            ShareReadRequest::GitStatus => json!(workspace.git_status(GitStatusRequest { expected_revision: revision }, true)?),
            ShareReadRequest::GitDiff { repository, version, path } => json!(workspace.git_diff(GitDiffRequest { expected_revision: revision, repository, version, path }, true)?),
            ShareReadRequest::Output { project_path, script, after, run_id } => {
                let project = if script.is_some() { workspace.node_runtime_project(&project_path, script.as_deref(), Some(revision))? }
                    else { workspace.runtime_project(&project_path, Some(revision))? };
                let mut output = if matches!(project.kind, super::super::ProjectKind::Node) || script.is_some() {
                    json!(self.node.output(&project, after, run_id.as_deref())?)
                } else { json!(self.java.output(&project, after, run_id.as_deref())?) };
                // Do not advertise loopback diagnostic/control console URLs.
                if let Some(object) = output.as_object_mut() { object.remove("arthasUrl"); object.remove("arthas"); }
                output
            }
        };
        lease.check()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guest_protocol_cannot_deserialize_owner_actions() {
        for action in ["write", "rollback", "commit", "mutate", "open", "replace", "execute", "share_create"] {
            assert!(serde_json::from_value::<ShareReadRequest>(json!({"type":action})).is_err());
        }
        assert!(serde_json::from_value::<ShareReadRequest>(json!({"type":"file","path":"a","operation":"delete"})).is_err());
    }
    #[test]
    fn revocation_expiry_scope_and_workspace_switch_are_enforced() {
        let root = std::env::temp_dir().join(format!("devflow-share-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let a = root.join("中文 A"); let b = root.join("B");
        fs::create_dir(&a).unwrap(); fs::create_dir(&b).unwrap();
        fs::write(a.join("a.txt"), "shared").unwrap(); fs::write(b.join("b.txt"), "private").unwrap();
        fs::write(a.join("package.json"), r#"{"name":"shared-a","scripts":{"dev":"node server.js"}}"#).unwrap();
        fs::write(b.join("package.json"), r#"{"name":"private-b","scripts":{"dev":"node server.js"}}"#).unwrap();
        let engine = crate::DevFlowEngine::new(vec![root.clone()], Some(a.clone())).unwrap();
        let created = engine.workspace.share_create(ShareCreateRequest { expected_revision: 0, lifetime_minutes: 5 }, true).unwrap();
        let read = || ShareReadRequest::File { path: display_path(&a.join("a.txt")) };
        assert_eq!(engine.share_read(&created.token, read()).unwrap()["content"], "shared");
        assert_eq!(engine.share_read(&created.token, read()).unwrap()["readonly"], true);
        assert!(!serde_json::to_string(&engine.workspace.share_list(true).unwrap()).unwrap().contains(&created.token));
        assert!(engine.share_read(&created.token, ShareReadRequest::File { path: display_path(&b.join("b.txt")) }).is_err());
        assert!(engine.share_read(&created.token, ShareReadRequest::File { path: "C:\\Windows\\win.ini".into() }).is_err());
        assert!(engine.share_read(&created.token, ShareReadRequest::File { path: display_path(&a.join("../B/b.txt")) }).is_err());
        assert!(engine.workspace.share_create(ShareCreateRequest { expected_revision: 0, lifetime_minutes: 5 }, false).is_err());
        assert!(engine.workspace.share_create(ShareCreateRequest { expected_revision: 0, lifetime_minutes: 4 }, true).is_err());
        assert!(engine.workspace.share_create(ShareCreateRequest { expected_revision: 999, lifetime_minutes: 5 }, true).is_err());
        assert!(engine.workspace.share_list(false).is_err());
        assert!(engine.workspace.share_revoke(ShareRevokeRequest { id: created.invitation.id.clone() }, false).is_err());
        engine.workspace.execute(WorkspaceRequest { expected_revision: 0, discard_changes: false, operation: WorkspaceOperation::Open { path: display_path(&b) } }, true).unwrap();
        assert_eq!(engine.share_read(&created.token, read()).unwrap()["content"], "shared");
        let runtime = engine.share_read(&created.token, ShareReadRequest::Runtime).unwrap();
        let projects = runtime["projects"].as_array().unwrap();
        assert!(!projects.is_empty());
        assert!(projects.iter().all(|project| comparison_path(Path::new(project["projectPath"].as_str().unwrap())) == comparison_path(&a)));
        let expired = engine.workspace.share_create(ShareCreateRequest { expected_revision: engine.workspace.snapshot().unwrap().revision, lifetime_minutes: 5 }, true).unwrap();
        engine.workspace.shares.lock().unwrap().get_mut(&expired.token).unwrap().deadline = Instant::now() - Duration::from_secs(1);
        assert!(engine.share_read(&expired.token, ShareReadRequest::Workspace).is_err());
        let in_flight = engine.workspace.share_lease(&created.token).unwrap();
        engine.workspace.share_revoke(ShareRevokeRequest { id: created.invitation.id }, true).unwrap();
        assert!(in_flight.check().is_err());
        assert!(engine.share_read(&created.token, read()).is_err());
        assert!(engine.workspace.share_list(true).unwrap().is_empty());
        drop(engine); fs::remove_dir_all(root).unwrap();
    }
}
