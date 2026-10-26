//! Gitee snapshots. Credentials never enter persisted settings or response DTOs.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardSyncSettings {
    pub owner: String, pub repo: String, pub branch: String, pub board_id: String, pub automatic: bool,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Stored {
    settings: Option<BoardSyncSettings>, sha: Option<String>, synced_revision: Option<u64>,
    last_synced: Option<u64>, error: Option<String>,
    #[serde(default)] export_version: u8,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSyncStatus {
    settings: Option<BoardSyncSettings>, synced_revision: Option<u64>, last_synced: Option<u64>,
    error: Option<String>, pending: bool, credential_ready: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardSyncRequest {
    pub workspace_key: String, pub expected_revision: u64, pub board_revision: u64,
    pub action: String, pub settings: Option<BoardSyncSettings>,
}

fn settings_valid(s: &BoardSyncSettings) -> Result<()> {
    let segment = |v: &str| !v.is_empty() && v.len() <= 100 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) && v != "." && v != "..";
    if !segment(&s.owner) || !segment(&s.repo) || uuid::Uuid::parse_str(&s.board_id).is_err()
        || s.branch.trim().is_empty() || s.branch.len() > 200 || s.branch.chars().any(char::is_control) {
        return Err(error("invalid_request", "仓库、分支或看板 UUID 无效"));
    }
    Ok(())
}
fn safe_file(path: &Path, limit: u64) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() || !m.is_file() || m.len() > limit => Err(error("permission_denied", "同步文件类型或大小无效")),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}
pub(super) fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    safe_file(path, BOARD_LIMIT)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(value).map_err(|_| error("io_error", "同步数据编码失败"))?)?;
        file.sync_all()?; drop(file); fs::rename(&temporary, path)?; Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(temporary); }
    result
}
fn status(stored: Stored, revision: u64) -> BoardSyncStatus {
    BoardSyncStatus { pending: stored.settings.is_some() && (stored.synced_revision != Some(revision) || stored.export_version != 2),
        credential_ready: std::env::var("DEVFLOW_GITEE_TOKEN").is_ok_and(|v| !v.trim().is_empty()),
        settings: stored.settings, synced_revision: stored.synced_revision, last_synced: stored.last_synced, error: stored.error }
}
fn client() -> Result<(reqwest::blocking::Client, String)> {
    let token = std::env::var("DEVFLOW_GITEE_TOKEN").ok().filter(|s| !s.trim().is_empty())
        .ok_or_else(|| error("unavailable", "请设置后端 DEVFLOW_GITEE_TOKEN 环境变量并重启 DevFlow"))?;
    let client = reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| error("unavailable", "无法创建 Gitee 连接"))?;
    Ok((client, token))
}
fn remote(s: &BoardSyncSettings, client: &reqwest::blocking::Client, token: &str) -> Result<Option<Value>> {
    let response = client.get(endpoint(s)).query(&[("ref", s.branch.as_str()), ("access_token", token)])
        .send().map_err(|_| error("unavailable", "Gitee 连接失败或超时"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
    decode(response).map(Some)
}
fn endpoint(s: &BoardSyncSettings) -> String {
    format!("https://gitee.com/api/v5/repos/{}/{}/contents/boards/{}.json", s.owner, s.repo, s.board_id)
}
fn decode(response: reqwest::blocking::Response) -> Result<Value> {
    if !response.status().is_success() {
        return Err(error("unavailable", format!("Gitee 请求失败（HTTP {}），请检查权限、分支和远端变更", response.status().as_u16())));
    }
    let mut bytes = Vec::new();
    response.take(BOARD_LIMIT * 2 + 1).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 > BOARD_LIMIT * 2 { return Err(error("invalid_request", "Gitee 响应过大")); }
    serde_json::from_slice(&bytes).map_err(|_| error("invalid_request", "Gitee 响应格式无效"))
}
fn portable(mut board: BoardSnapshot) -> BoardSnapshot {
    board.workspace_key.clear();
    for task in &mut board.tasks { task.owner = None; task.owner_id = None; }
    // Activity is local audit history, never part of the remote snapshot.
    board.activity.clear();
    for task in &mut board.tasks { task.activity.clear(); }
    board
}
fn export_snapshot(board: BoardSnapshot) -> Result<Vec<u8>> {
    let mut value = serde_json::to_value(portable(board)).map_err(|_| error("io_error", "快照编码失败"))?;
    if let Some(object) = value.as_object_mut() { object.remove("activity"); }
    if let Some(tasks) = value["tasks"].as_array_mut() {
        for task in tasks { if let Some(object) = task.as_object_mut() { object.remove("activity"); } }
    }
    serde_json::to_vec_pretty(&value).map_err(|_| error("io_error", "快照编码失败"))
}
impl WorkspaceService {
    pub fn board_sync(&self, request: BoardSyncRequest, authenticated: bool, human: bool) -> Result<BoardSyncStatus> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let board = self.board_read(BoardRead { workspace_key: request.workspace_key.clone(), expected_revision: request.expected_revision }, true)?;
        let path = self.board_location(&request.workspace_key)?.with_extension("sync.json");
        let lock_path = path.with_extension("sync.lock");
        safe_file(&lock_path, 1024)?;
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path)?;
        lock.try_lock().map_err(|_| error("conflict", "同步正在进行，请稍后重试"))?;
        safe_file(&path, BOARD_LIMIT)?;
        let mut stored: Stored = if path.exists() { serde_json::from_slice(&fs::read(&path)?).map_err(|_| error("io_error", "同步配置损坏，原文件已保留"))? } else { Stored::default() };
        if request.action == "status" { return Ok(status(stored, board.revision)); }
        if request.action != "auto" && board.revision != request.board_revision { return Err(error("conflict", "看板已变化，请刷新后重试")); }
        if request.action == "configure" {
            let settings = request.settings.ok_or_else(|| error("invalid_request", "缺少同步设置"))?;
            settings_valid(&settings)?;
            let changed = stored.settings.as_ref().is_none_or(|s| s.owner != settings.owner || s.repo != settings.repo || s.branch != settings.branch || s.board_id != settings.board_id);
            if changed { stored = Stored::default(); }
            stored.settings = Some(settings); stored.error = None;
            save_json(&path, &stored)?; return Ok(status(stored, board.revision));
        }
        if !["push", "restore", "auto"].contains(&request.action.as_str()) { return Err(error("invalid_request", "未知同步操作")); }
        let settings = stored.settings.clone().ok_or_else(|| error("invalid_request", "请先配置 Gitee 仓库"))?;
        settings_valid(&settings)?;
        if request.action == "auto" && (!settings.automatic || (stored.synced_revision == Some(board.revision) && stored.export_version == 2) || stored.error.as_deref().is_some_and(|s| s.contains("远端已变化"))) {
            return Ok(status(stored, board.revision));
        }
        let result = (|| -> Result<()> {
            let (client, token) = client()?;
            let remote = remote(&settings, &client, &token)?;
            let sha = remote.as_ref().map(|v| v["sha"].as_str().filter(|s| !s.is_empty()).map(str::to_owned)
                .ok_or_else(|| error("invalid_request", "Gitee 未返回有效文件版本，已停止同步"))).transpose()?;
            if request.action == "restore" {
                if !human { return Err(error("permission_denied", "恢复需由本机用户确认")); }
                let value = remote.ok_or_else(|| error("not_found", "远端快照不存在，请检查仓库、分支和看板 ID"))?;
                let content = value["content"].as_str().ok_or_else(|| error("invalid_request", "远端缺少快照内容"))?;
                let bytes = STANDARD.decode(content.split_whitespace().collect::<String>()).map_err(|_| error("invalid_request", "远端快照编码无效"))?;
                if bytes.len() as u64 > BOARD_LIMIT { return Err(error("invalid_request", "远端快照超过 8 MiB")); }
                let restored: BoardSnapshot = serde_json::from_slice(&bytes).map_err(|_| error("invalid_request", "远端看板格式无效"))?;
                validate(&restored)?;
                self.board_execute(BoardRequest { workspace_key: request.workspace_key.clone(), expected_revision: request.expected_revision,
                    board_revision: request.board_revision, operation: BoardOperation::Restore { board: restored } }, true, "你", true)?;
                stored.sha = sha;
                // Restored tasks can have normalized status; next upload records that local state.
                stored.synced_revision = None;
            } else {
                if sha != stored.sha { return Err(error("conflict", "远端已变化，已暂停上传；请检查后恢复远端快照，或改用新的看板 ID")); }
                let content = STANDARD.encode(export_snapshot(board.clone())?);
                let mut body = json!({"access_token":token,"branch":settings.branch,"message":format!("DevFlow board revision {}", board.revision),"content":content});
                let builder = if let Some(sha) = sha { body["sha"] = json!(sha); client.put(endpoint(&settings)) } else { client.post(endpoint(&settings)) };
                let response = builder.json(&body).send().map_err(|_| error("unavailable", "Gitee 上传失败或超时；稍后重试前会重新核对远端"))?;
                let value = decode(response)?;
                stored.sha = Some(value["content"]["sha"].as_str().ok_or_else(|| error("invalid_request", "Gitee 未返回文件版本，请检查远端后再同步"))?.into());
                stored.synced_revision = Some(board.revision);
                stored.export_version = 2;
            }
            stored.last_synced = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs());
            Ok(())
        })();
        stored.error = result.as_ref().err().map(|e| e.message.clone());
        save_json(&path, &stored)?;
        // Status carries failures so the page can retain settings and show retry controls.
        Ok(status(stored, board.revision))
    }
    pub fn board_sync_tick(&self) {
        let Ok(snapshot) = self.snapshot() else { return };
        let Some(key) = snapshot.workspace_file.or(snapshot.workspace_root) else { return };
        let Ok(path) = self.board_location(&key) else { return };
        if !path.with_extension("sync.json").exists() { return; }
        let _ = self.board_sync(BoardSyncRequest { workspace_key: key, expected_revision: snapshot.revision,
            board_revision: 0, action: "auto".into(), settings: None }, true, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_path_escape_and_invalid_board_identity() {
        let mut s = BoardSyncSettings { owner: "team".into(), repo: "项目".into(), branch: "main".into(), board_id: uuid::Uuid::new_v4().to_string(), automatic: true };
        assert!(settings_valid(&s).is_err()); s.repo = "tasks".into(); assert!(settings_valid(&s).is_ok());
        s.owner = "../team".into(); assert!(settings_valid(&s).is_err());
    }
    #[test]
    fn portable_snapshot_removes_local_identity() {
        let task: Task = serde_json::from_value(json!({"id":"DF-001","title":"任务正文","description":"仍要同步的描述","acceptance":"验收条件","feature":"","priority":"普通","status":"todo","activity":["任务活动不上传"]})).unwrap();
        let b = BoardSnapshot { workspace_key: "D:/中文/project".into(), tasks: vec![task], activity: vec!["123 · Codex:secret-session · 已完成登录交接".into()], ..Default::default() };
        let exported: Value = serde_json::from_slice(&export_snapshot(b).unwrap()).unwrap();
        assert_eq!(exported["workspaceKey"], ""); assert!(exported.get("activity").is_none());
        assert!(exported["tasks"][0].get("activity").is_none());
        assert_eq!(exported["tasks"][0]["description"], "仍要同步的描述");
        let restored: BoardSnapshot = serde_json::from_value(exported).unwrap();
        assert!(restored.activity.is_empty()); assert!(restored.tasks[0].activity.is_empty());
    }
}
