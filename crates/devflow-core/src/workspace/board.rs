//! Workspace-scoped board. Both adapters use these transactions while holding
//! the workspace lock, so switching workspaces cannot redirect an in-flight write.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
#[path = "board_sync.rs"]
mod sync;
pub use sync::{BoardSyncRequest, BoardSyncSettings, BoardSyncStatus};

const BOARD_LIMIT: u64 = 8 * 1024 * 1024;

fn io_error(value: std::io::Error) -> WorkspaceError { value.into() }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Feature {
    pub id: String, pub name: String, pub description: String, pub goal: String,
    pub x: f64, pub y: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Task {
    pub id: String, pub title: String, pub description: String, pub acceptance: String,
    pub feature: String, pub priority: String, pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default)] pub activity: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardSnapshot {
    pub workspace_key: String, pub revision: u64, pub next_task: u64,
    pub features: Vec<Feature>, pub tasks: Vec<Task>, pub edges: Vec<[String; 2]>,
    #[serde(default)]
    pub activity: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardRead { pub workspace_key: String, pub expected_revision: u64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoardRequest {
    pub workspace_key: String, pub expected_revision: u64, pub board_revision: u64,
    pub operation: BoardOperation,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum BoardOperation {
    FeatureSave { feature: Feature },
    FeatureDelete { id: String, move_to: Option<String> },
    Edges { edges: Vec<[String; 2]> },
    TaskCreate { title: String, #[serde(default)] description: String, #[serde(default)] acceptance: String, #[serde(default)] feature: String, #[serde(default = "normal_priority")] priority: String },
    TaskUpdate { id: String, title: String, description: String, acceptance: String, feature: String, priority: String },
    TaskDelete { id: String },
    Action { id: String, action: String, text: Option<String> },
    Import { features: Vec<Feature>, tasks: Vec<Task>, edges: Vec<[String; 2]> },
    Restore { board: BoardSnapshot },
}
fn normal_priority() -> String { "普通".into() }

fn text(value: &str, max: usize, required: bool) -> Result<()> {
    if value.len() > max || (required && value.trim().is_empty()) {
        return Err(error("invalid_request", "字段为空或超过长度限制"));
    }
    Ok(())
}
fn stamp(actor: &str, message: &str) -> String {
    let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    format!("{at} · {actor} · {message}")
}
fn validate(board: &BoardSnapshot) -> Result<()> {
    if board.features.len() > 200 || board.tasks.len() > 5000 || board.edges.len() > 1000 {
        return Err(error("invalid_request", "看板最多支持 200 个功能、5000 个任务、1000 条关联"));
    }
    let mut ids = HashSet::new();
    if board.activity.len() > 1000 { return Err(error("invalid_request", "看板活动记录超限")); }
    for entry in &board.activity { text(entry, 20000, false)?; }
    for f in &board.features {
        text(&f.id, 100, true)?; text(&f.name, 240, true)?;
        text(&f.description, 16000, false)?; text(&f.goal, 16000, false)?;
        if !ids.insert(&f.id) || !f.x.is_finite() || !f.y.is_finite() || f.x.abs() > 100000.0 || f.y.abs() > 100000.0 {
            return Err(error("invalid_request", "功能标识重复或坐标无效"));
        }
    }
    let mut task_ids = HashSet::new();
    for t in &board.tasks {
        text(&t.id, 100, true)?; text(&t.title, 480, true)?;
        text(&t.description, 16000, false)?; text(&t.acceptance, 16000, false)?;
        if (!t.feature.is_empty() && !ids.contains(&t.feature)) || !task_ids.insert(&t.id)
            || !["todo", "doing", "review", "done"].contains(&t.status.as_str())
            || !["高", "普通"].contains(&t.priority.as_str()) {
            return Err(error("invalid_request", "任务标识、所属功能、状态或优先级无效"));
        }
        for value in [&t.owner, &t.owner_id, &t.blocked, &t.result].into_iter().flatten() { text(value, 16000, false)?; }
        if t.activity.len() > 1000 { return Err(error("invalid_request", "单个任务的活动记录已达 1000 条")); }
        for entry in &t.activity { text(entry, 20000, false)?; }
    }
    let mut edges = HashSet::new();
    for [a, b] in &board.edges {
        if a == b || !ids.contains(a) || !ids.contains(b) || !edges.insert((a, b)) {
            return Err(error("invalid_request", "功能关联无效或重复"));
        }
    }
    Ok(())
}

impl WorkspaceService {
    pub(crate) fn qa_location(&self, key: &str, revision: u64) -> Result<PathBuf> {
        let state = self.lock()?;
        self.check_board_scope(&state, key, revision)?;
        Ok(self.board_location(key)?.with_extension("qa.json"))
    }
    pub(crate) fn diagnosis_location(&self, key: &str, revision: u64) -> Result<PathBuf> {
        let state = self.lock()?;
        self.check_board_scope(&state, key, revision)?;
        Ok(self.board_location(key)?.with_extension("diagnosis.json"))
    }
    // App-owned storage alongside recent-workspace.json, never a caller-supplied path.
    fn board_location(&self, key: &str) -> Result<PathBuf> {
        let recent = self.recent_path.lock().map_err(|_| error("unavailable", "存储暂不可用"))?;
        let base = match recent.as_ref().and_then(|p| p.parent()) {
            Some(path) => path.to_owned(),
            None => std::env::current_dir().map_err(io_error)?.join(".devflow"),
        };
        // Stable filename; embedded full identity below also detects hash collisions.
        let hash = key.as_bytes().iter().fold(14695981039346656037u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(1099511628211));
        let directory = base.join("boards");
        for p in [&base, &directory] {
            if let Ok(meta) = fs::symlink_metadata(p) {
                if meta.file_type().is_symlink() || !meta.is_dir() { return Err(error("permission_denied", "看板存储目录不可用")); }
            }
        }
        fs::create_dir_all(&directory).map_err(io_error)?;
        Ok(directory.join(format!("{hash:016x}.json")))
    }
    fn check_board_scope(&self, state: &State, key: &str, revision: u64) -> Result<()> {
        if state.revision != revision { return Err(error("conflict", "工作区已变化，请刷新")); }
        let session = state.session.as_ref().ok_or_else(|| error("outside_workspace", "请先打开工作区"))?;
        let path = session.file.as_ref().unwrap_or(&session.base);
        self.authorize(path)?;
        if display_path(path) != key { return Err(error("conflict", "工作区已切换，请重新打开看板")); }
        Ok(())
    }
    pub fn board_read(&self, request: BoardRead, authenticated: bool) -> Result<BoardSnapshot> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let state = self.lock()?;
        self.check_board_scope(&state, &request.workspace_key, request.expected_revision)?;
        load(&self.board_location(&request.workspace_key)?, &request.workspace_key)
    }
    pub fn board_execute(&self, request: BoardRequest, authenticated: bool, actor: &str, human: bool) -> Result<BoardSnapshot> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        text(actor, 240, true)?;
        let state = self.lock()?;
        self.check_board_scope(&state, &request.workspace_key, request.expected_revision)?;
        let path = self.board_location(&request.workspace_key)?;
        // Cross-process exclusion, in addition to the in-process workspace lock.
        let lock_path = path.with_extension("lock");
        if let Ok(meta) = fs::symlink_metadata(&lock_path) {
            if !meta.is_file() || meta.file_type().is_symlink() { return Err(error("permission_denied", "看板锁文件无效")); }
        }
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path).map_err(io_error)?;
        // OS advisory locks release automatically even after a process crash.
        // Keep the lock inode, avoiding removal/recreation races across processes.
        lock.try_lock().map_err(|e| error("conflict", format!("看板正在写入，请稍后重试：{e}")))?;
        let mut board = load(&path, &request.workspace_key)?;
        if board.revision != request.board_revision { return Err(error("conflict", "看板已被其他入口修改，请刷新后重试；当前编辑尚未保存")); }
        let restore_backup = if matches!(&request.operation, BoardOperation::Restore { .. }) { Some(board.clone()) } else { None };
        let event = apply(&mut board, request.operation, actor, human)?;
        validate(&board)?;
        board.revision = board.revision.checked_add(1).ok_or_else(|| error("conflict", "看板版本已达上限"))?;
        board.activity.push(stamp(actor, &event));
        if board.activity.len() > 1000 { board.activity.remove(0); }
        let bytes = serde_json::to_vec_pretty(&board).map_err(|e| error("io_error", e.to_string()))?;
        if bytes.len() as u64 > BOARD_LIMIT { return Err(error("invalid_request", "看板数据超过 8 MiB")); }
        if let Some(backup) = restore_backup {
            sync::save_json(&path.with_extension(format!("{}.backup.json", uuid::Uuid::new_v4())), &backup)?;
        }
        let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(io_error)?;
            file.write_all(&bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            drop(file);
            fs::rename(&temp, &path).map_err(io_error)?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(temp); }
        result?;
        Ok(board)
    }
}
fn load(path: &Path, key: &str) -> Result<BoardSnapshot> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BoardSnapshot { workspace_key: key.into(), ..Default::default() }),
        Err(e) => return Err(io_error(e)),
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > BOARD_LIMIT => return Err(error("permission_denied", "看板文件类型或大小无效")),
        _ => (),
    }
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(io_error)?.take(BOARD_LIMIT + 1).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 > BOARD_LIMIT { return Err(error("invalid_request", "看板文件超过 8 MiB")); }
    let board: BoardSnapshot = serde_json::from_slice(&bytes)
        .map_err(|e| error("io_error", format!("看板文件损坏，原文件已保留：{e}")))?;
    if board.workspace_key != key { return Err(error("conflict", "看板存储标识冲突")); }
    validate(&board)?;
    Ok(board)
}
fn apply(board: &mut BoardSnapshot, operation: BoardOperation, actor: &str, human: bool) -> Result<String> {
    use BoardOperation::*;
    let event = match operation {
        Restore { board: restored } => {
            if !human { return Err(error("permission_denied", "恢复需由本机用户确认")); }
            let revision = board.revision;
            let key = board.workspace_key.clone();
            let next = board.next_task.max(restored.next_task);
            let local_activity = board.activity.clone();
            let local_tasks = board.tasks.clone();
            *board = restored;
            board.workspace_key = key; board.revision = revision; board.next_task = next;
            board.activity = local_activity;
            for task in &mut board.tasks {
                task.activity = local_tasks.iter().find(|local| local.id == task.id).map(|local| local.activity.clone()).unwrap_or_default();
                task.owner = None; task.owner_id = None;
                if task.status == "doing" { task.status = "todo".into(); }
            }
            "从 Gitee 恢复看板；执行身份已清除".into()
        }
        FeatureSave { feature } => {
            let name = feature.name.clone();
            if let Some(current) = board.features.iter_mut().find(|f| f.id == feature.id) { *current = feature; }
            else { board.features.push(feature); }
            format!("保存功能：{name}")
        }
        FeatureDelete { id, move_to } => {
            if !board.features.iter().any(|f| f.id == id) { return Err(error("not_found", "功能不存在")); }
            if board.tasks.iter().any(|t| t.feature == id) {
                let target = move_to.filter(|v| v != &id && board.features.iter().any(|f| &f.id == v))
                    .ok_or_else(|| error("invalid_request", "请先选择任务要迁移到的功能"))?;
                for t in board.tasks.iter_mut().filter(|t| t.feature == id) { t.feature = target.clone(); t.activity.push(stamp(actor, "所属功能已迁移")); }
            }
            board.features.retain(|f| f.id != id); board.edges.retain(|e| !e.contains(&id));
            format!("删除功能：{id}")
        }
        Edges { edges } => { board.edges = edges; "更新功能关联".into() }
        TaskCreate { title, description, acceptance, feature, priority } => {
            let id = loop { board.next_task = board.next_task.checked_add(1).ok_or_else(|| error("conflict", "任务编号已达上限"))?; let id = format!("DF-{:03}", board.next_task); if !board.tasks.iter().any(|t| t.id == id) { break id; } };
            board.tasks.push(Task { id: id.clone(), title, description, acceptance, feature, priority, status: "todo".into(), owner: None, owner_id: None, blocked: None, result: None, activity: vec![stamp(actor, "创建任务")] });
            format!("创建任务：{id}")
        }
        TaskUpdate { id, title, description, acceptance, feature, priority } => {
            let task = board.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| error("not_found", "任务不存在"))?;
            if !human && task.owner_id.as_deref().is_some_and(|v| v != actor) { return Err(error("permission_denied", "其他执行者正在处理该任务")); }
            task.title = title; task.description = description; task.acceptance = acceptance; task.feature = feature; task.priority = priority;
            task.activity.push(stamp(actor, "编辑任务")); format!("编辑任务：{id}")
        }
        TaskDelete { id } => {
            let task = board.tasks.iter().find(|t| t.id == id).ok_or_else(|| error("not_found", "任务不存在"))?;
            if !human && task.owner_id.as_deref().is_some_and(|v| v != actor) { return Err(error("permission_denied", "其他执行者正在处理该任务")); }
            board.tasks.retain(|t| t.id != id); format!("删除任务：{id}")
        }
        Action { id, action, text: note } => {
            let t = board.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| error("not_found", "任务不存在"))?;
            let note = note.unwrap_or_default().trim().to_owned(); text(&note, 16000, false)?;
            let owned = t.owner_id.as_deref() == Some(actor);
            let allowed = match action.as_str() {
                "move" => human && ["todo", "doing", "review", "done"].contains(&note.as_str()) && t.status != note,
                "complete" => human && t.status != "done",
                "claim" => t.status == "todo" && t.blocked.is_none() && t.owner_id.is_none(),
                "release" | "submit" | "progress" => t.status == "doing" && (owned || human),
                "approve" => human && t.status == "review" && t.blocked.is_none(),
                "reject" => human && t.status == "review",
                "reopen" => human && t.status == "done",
                "block" | "unblock" => (human || owned) && t.status != "done",
                _ => false,
            };
            if !allowed { return Err(error("conflict", "当前状态或执行者不允许此操作，请刷新后检查任务")); }
            if ["submit", "progress", "block", "reject"].contains(&action.as_str()) { text(&note, 16000, true)?; }
            match action.as_str() {
                "move" => { t.status = note.clone(); if note == "todo" || note == "done" { t.owner = None; t.owner_id = None; } if note == "done" { t.blocked = None; } }
                "complete" => { t.status = "done".into(); t.owner = None; t.owner_id = None; t.blocked = None; }
                "claim" => { t.status = "doing".into(); t.owner = Some(actor.into()); t.owner_id = Some(actor.into()); }
                "release" => { t.status = "todo".into(); t.owner = None; t.owner_id = None; }
                "submit" => { if t.blocked.is_some() { return Err(error("conflict", "请先解除阻塞")); } t.status = "review".into(); t.result = Some(note.clone()); }
                "approve" => t.status = "done".into(),
                "reject" => t.status = "doing".into(),
                "reopen" => { t.status = "todo".into(); t.owner = None; t.owner_id = None; }
                "block" => t.blocked = Some(note.clone()), "unblock" => t.blocked = None,
                _ => (),
            }
            let label = match action.as_str() { "move" => "人工调整状态", "complete" => "用户手动标记完成", "claim" => "认领任务", "release" => "释放任务", "submit" => "提交验收", "approve" => "通过验收", "reject" => "退回修改", "reopen" => "重新打开", "block" => "标记阻塞", "unblock" => "解除阻塞", _ => "记录进展" };
            t.activity.push(stamp(actor, &format!("{label}{}", if note.is_empty() { String::new() } else { format!("：{note}") })));
            format!("{id}：{label}")
        }
        Import { features, mut tasks, edges } => {
            if !human || board.revision != 0 { return Err(error("conflict", "仅允许向未使用的空看板导入一次草稿")); }
            for t in &mut tasks {
                // Prototype claims and deliveries never become real execution evidence.
                t.status = "todo".into(); t.owner = None; t.owner_id = None; t.result = None;
                t.activity = vec![stamp(actor, "从原型草稿导入；演示状态和交付已清除")];
            }
            board.features = features; board.tasks = tasks; board.edges = edges;
            board.next_task = board.tasks.iter().filter_map(|t| t.id.strip_prefix("DF-").and_then(|id| id.parse::<u64>().ok())).max().unwrap_or(0);
            "导入原型草稿".into()
        }
    };
    for task in &mut board.tasks {
        if task.activity.len() > 1000 { task.activity.drain(..task.activity.len() - 1000); }
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_humans_can_complete_directly_without_delivery() {
        for phase in ["todo", "doing", "review"] {
            let f = Fixture::new(); let id = f.seed();
            if phase != "todo" { f.execute(BoardOperation::Action { id: id.clone(), action: "claim".into(), text: None }, "Codex:worker", false).unwrap(); }
            if phase == "review" { f.execute(BoardOperation::Action { id: id.clone(), action: "submit".into(), text: Some("真实交付".into()) }, "Codex:worker", false).unwrap(); }
            f.execute(BoardOperation::Action { id: id.clone(), action: "block".into(), text: Some("人工已解决".into()) }, "你", true).unwrap();
            assert!(f.execute(BoardOperation::Action { id: id.clone(), action: "complete".into(), text: None }, "Codex:worker", false).is_err());
            let board = f.execute(BoardOperation::Action { id: id.clone(), action: "complete".into(), text: None }, "你", true).unwrap();
            let task = &board.tasks[0];
            assert_eq!(task.status, "done"); assert!(task.owner_id.is_none()); assert!(task.blocked.is_none());
            assert_eq!(task.result.as_deref(), if phase == "review" { Some("真实交付") } else { None });
            assert!(task.activity.last().unwrap().contains("用户手动标记完成"));
            assert!(f.execute(BoardOperation::Action { id: id.clone(), action: "complete".into(), text: None }, "你", true).is_err());
            let reopened = f.execute(BoardOperation::Action { id, action: "reopen".into(), text: None }, "你", true).unwrap();
            assert_eq!(reopened.tasks[0].status, "todo");
        }
    }

    #[test]
    fn quick_task_can_start_unclassified_and_be_refined() {
        let f = Fixture::new();
        let operation = serde_json::from_value(serde_json::json!({"type":"task_create","title":"先粗略记录中文待办"})).unwrap();
        let board = f.execute(operation, "你", true).unwrap();
        assert_eq!(board.tasks[0].feature, ""); assert_eq!(board.tasks[0].priority, "普通");
        f.execute(BoardOperation::FeatureSave { feature: feature("later") }, "你", true).unwrap();
        let board = f.execute(BoardOperation::TaskUpdate { id: board.tasks[0].id.clone(), title: "整理后的标题".into(), description: "补充范围".into(), acceptance: "验收目标".into(), feature: "later".into(), priority: "普通".into() }, "Codex:refine", false).unwrap();
        assert_eq!(board.tasks[0].status, "todo"); assert_eq!(board.tasks[0].feature, "later");
        assert!(board.tasks[0].owner_id.is_none());
    }
    #[test]
    fn restore_preserves_local_revision_sequence_and_releases_claims() {
        let f = Fixture::new(); f.seed();
        f.execute(BoardOperation::Action { id: "DF-001".into(), action: "claim".into(), text: None }, "Codex:old", false).unwrap();
        let mut restored = f.read(); restored.workspace_key = "D:/other/machine".into(); restored.revision = 999;
        let local_history = restored.tasks[0].activity.clone();
        restored.activity = vec!["不应导入的远端活动".into()];
        restored.tasks[0].activity = vec!["不应导入的远端任务活动".into()];
        let before = f.read().revision;
        assert!(f.execute(BoardOperation::Restore { board: restored.clone() }, "Codex:other", false).is_err());
        let board = f.execute(BoardOperation::Restore { board: restored }, "你", true).unwrap();
        assert_eq!(board.revision, before + 1); assert_eq!(board.workspace_key, f.key);
        assert_eq!(board.tasks[0].activity, local_history);
        assert!(!board.activity.iter().any(|entry| entry.contains("不应导入")));
        assert_eq!(board.tasks[0].status, "todo"); assert!(board.tasks[0].owner_id.is_none());
        let directory = f.service.board_location(&f.key).unwrap().parent().unwrap().to_owned();
        let backups: Vec<_> = fs::read_dir(directory).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(".backup.json")).collect();
        assert_eq!(backups.len(), 1);
        let backup: BoardSnapshot = serde_json::from_slice(&fs::read(&backups[0]).unwrap()).unwrap();
        assert_eq!(backup.revision, before); assert_eq!(backup.tasks[0].owner_id.as_deref(), Some("Codex:old"));
    }
    #[test]
    fn sync_config_requires_scope_auth_and_current_board_revision() {
        let f = Fixture::new();
        let request = |revision, action: &str| BoardSyncRequest { workspace_key: f.key.clone(), expected_revision: 0, board_revision: revision, action: action.into(), settings: Some(BoardSyncSettings { owner: "team".into(), repo: "board".into(), branch: "main".into(), board_id: uuid::Uuid::new_v4().to_string(), automatic: false }) };
        assert!(f.service.board_sync(request(0, "status"), false, true).is_err());
        assert!(f.service.board_sync(request(1, "configure"), true, true).is_err());
        f.service.board_sync(request(0, "configure"), true, true).unwrap();
        let file = f.service.board_location(&f.key).unwrap().with_extension("sync.json");
        assert!(!fs::read_to_string(file).unwrap().contains("access_token"));
        let mut stale = request(0, "status"); stale.expected_revision = 999;
        assert!(f.service.board_sync(stale, true, true).is_err());
    }

    struct Fixture { root: PathBuf, service: WorkspaceService, key: String }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("devflow-board-中文-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let root = root.canonicalize().unwrap();
            let service = WorkspaceService::new(vec![root.clone()], Some(root.clone())).unwrap();
            *service.recent_path.lock().unwrap() = Some(root.join(".devflow/recent-workspace.json"));
            Self { key: display_path(&root), root, service }
        }
        fn read(&self) -> BoardSnapshot {
            self.service.board_read(BoardRead { workspace_key: self.key.clone(), expected_revision: self.service.snapshot().unwrap().revision }, true).unwrap()
        }
        fn execute(&self, operation: BoardOperation, actor: &str, human: bool) -> Result<BoardSnapshot> {
            self.service.board_execute(BoardRequest { workspace_key: self.key.clone(), expected_revision: self.service.snapshot().unwrap().revision, board_revision: self.read().revision, operation }, true, actor, human)
        }
        fn seed(&self) -> String {
            self.execute(BoardOperation::FeatureSave { feature: feature("feature") }, "你", true).unwrap();
            self.execute(BoardOperation::TaskCreate { title: "中文任务".into(), description: "核心说明".into(), acceptance: "验收标准".into(), feature: "feature".into(), priority: "高".into() }, "你", true).unwrap().tasks[0].id.clone()
        }
        fn action(&self, id: &str, action: &str, note: Option<&str>, actor: &str, human: bool) -> Result<BoardSnapshot> {
            self.execute(BoardOperation::Action { id: id.into(), action: action.into(), text: note.map(str::to_owned) }, actor, human)
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }
    fn feature(id: &str) -> Feature {
        Feature { id: id.into(), name: "看板功能".into(), description: "描述".into(), goal: "目标".into(), x: 100.0, y: 100.0 }
    }

    #[test]
    fn human_can_move_task_without_notes() {
        let f = Fixture::new();
        let id = f.seed();
        assert!(f.action(&id, "move", Some("review"), "Codex:worker", false).is_err());
        let board = f.action(&id, "move", Some("review"), "你", true).unwrap();
        assert_eq!(board.tasks[0].status, "review");
        assert!(board.tasks[0].result.is_none());
        let board = f.action(&id, "move", Some("todo"), "你", true).unwrap();
        assert_eq!(board.tasks[0].status, "todo");
        assert!(f.action(&id, "move", Some("invalid"), "你", true).is_err());
    }

    #[test]
    fn persistent_utf8_and_workspace_isolation() {
        let f = Fixture::new(); let id = f.seed();
        let second = WorkspaceService::new(vec![f.root.clone()], Some(f.root.clone())).unwrap();
        *second.recent_path.lock().unwrap() = Some(f.root.join(".devflow/recent-workspace.json"));
        let restored = second.board_read(BoardRead { workspace_key: f.key.clone(), expected_revision: 0 }, true).unwrap();
        assert_eq!(restored.tasks[0].title, "中文任务"); assert_eq!(restored.tasks[0].id, id);
        let other = f.root.join("另一个工作区"); fs::create_dir(&other).unwrap();
        f.service.execute(WorkspaceRequest { expected_revision: 0, discard_changes: false, operation: WorkspaceOperation::Open { path: display_path(&other) } }, true).unwrap();
        assert_eq!(f.service.board_read(BoardRead { workspace_key: f.key.clone(), expected_revision: 1 }, true).unwrap_err().code, "conflict");
        let empty = f.service.board_read(BoardRead { workspace_key: display_path(&other), expected_revision: 1 }, true).unwrap();
        assert!(empty.tasks.is_empty());
    }
    #[test]
    fn full_delivery_and_human_acceptance_with_handoff() {
        let f = Fixture::new(); let id = f.seed();
        f.action(&id, "claim", None, "Codex:one", false).unwrap();
        assert!(f.action(&id, "submit", Some(""), "Codex:one", false).is_err());
        assert!(f.action(&id, "progress", Some("进展"), "Codex:two", false).is_err());
        f.action(&id, "progress", Some("已完成接口；剩余前端；未运行测试"), "Codex:one", false).unwrap();
        f.action(&id, "block", Some("等待配置"), "Codex:one", false).unwrap();
        assert!(f.action(&id, "submit", Some("交付"), "Codex:one", false).is_err());
        f.action(&id, "unblock", None, "Codex:one", false).unwrap();
        // Human can recover a claim from a terminated conversation without impersonation.
        f.action(&id, "release", None, "你", true).unwrap();
        f.action(&id, "claim", None, "Codex:two", false).unwrap();
        f.action(&id, "submit", Some("实现已完成；未编译，待用户验证"), "Codex:two", false).unwrap();
        assert!(f.action(&id, "approve", None, "Codex:two", false).is_err());
        assert!(f.action(&id, "reject", Some(" "), "你", true).is_err());
        f.action(&id, "reject", Some("补充失败提示"), "你", true).unwrap();
        f.action(&id, "submit", Some("已补充失败提示"), "Codex:two", false).unwrap();
        let done = f.action(&id, "approve", None, "你", true).unwrap();
        assert_eq!(done.tasks[0].status, "done");
        assert!(done.tasks[0].activity.iter().any(|a| a.contains("剩余前端")));
        let reopened = f.action(&id, "reopen", None, "你", true).unwrap();
        assert_eq!(reopened.tasks[0].status, "todo"); assert!(reopened.tasks[0].owner_id.is_none());
        assert!(reopened.tasks[0].result.is_some()); // Previous delivery remains auditable.
    }
    #[test]
    fn simultaneous_claims_have_one_winner() {
        let f = Fixture::new(); let id = f.seed(); let revision = f.read().revision;
        let outcomes = std::thread::scope(|scope| {
            let handles: Vec<_> = ["Codex:one", "Codex:two"].into_iter().map(|actor| {
                let service = &f.service; let key = f.key.clone(); let id = id.clone();
                scope.spawn(move || service.board_execute(BoardRequest { workspace_key: key, expected_revision: 0, board_revision: revision, operation: BoardOperation::Action { id, action: "claim".into(), text: None } }, true, actor, false))
            }).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(outcomes.iter().filter(|v| v.is_ok()).count(), 1);
        assert_eq!(outcomes.iter().filter(|v| v.as_ref().is_err_and(|e| e.code == "conflict")).count(), 1);
    }
    #[test]
    fn auth_revision_and_invalid_changes_preserve_disk() {
        let f = Fixture::new(); f.seed(); let before = serde_json::to_value(f.read()).unwrap();
        assert_eq!(f.service.board_read(BoardRead { workspace_key: f.key.clone(), expected_revision: 0 }, false).unwrap_err().code, "unauthorized");
        let stale = BoardRequest { workspace_key: f.key.clone(), expected_revision: 1, board_revision: 2, operation: BoardOperation::TaskDelete { id: "DF-001".into() } };
        assert_eq!(f.service.board_execute(stale, true, "你", true).unwrap_err().code, "conflict");
        assert!(f.execute(BoardOperation::Edges { edges: vec![["feature".into(), "missing".into()]] }, "你", true).is_err());
        assert!(f.execute(BoardOperation::FeatureDelete { id: "feature".into(), move_to: None }, "你", true).is_err());
        assert_eq!(serde_json::to_value(f.read()).unwrap(), before);
        // An aborted transaction releases its OS lock and permits the next valid write.
        f.execute(BoardOperation::FeatureSave { feature: feature("second") }, "你", true).unwrap();
        let moved = f.execute(BoardOperation::FeatureDelete { id: "feature".into(), move_to: Some("second".into()) }, "你", true).unwrap();
        assert_eq!(moved.tasks[0].feature, "second");
    }
    #[test]
    fn import_never_promotes_demo_delivery_or_reuses_deleted_numbers() {
        let f = Fixture::new();
        let task = Task { id: "DF-001".into(), title: "原型任务".into(), description: String::new(), acceptance: String::new(), feature: "feature".into(), priority: "普通".into(), status: "done".into(), owner: Some("演示".into()), owner_id: None, blocked: None, result: Some("模拟交付".into()), activity: vec!["模拟验收".into()] };
        let imported = f.execute(BoardOperation::Import { features: vec![feature("feature")], tasks: vec![task], edges: vec![] }, "你", true).unwrap();
        assert_eq!(imported.tasks[0].status, "todo"); assert!(imported.tasks[0].result.is_none());
        assert!(f.execute(BoardOperation::Import { features: vec![], tasks: vec![], edges: vec![] }, "你", true).is_err());
        f.execute(BoardOperation::TaskDelete { id: "DF-001".into() }, "你", true).unwrap();
        let created = f.execute(BoardOperation::TaskCreate { title: "正式任务".into(), description: String::new(), acceptance: String::new(), feature: "feature".into(), priority: "普通".into() }, "你", true).unwrap();
        assert_ne!(created.tasks[0].id, "DF-001");
    }
    #[test]
    fn malformed_files_are_retained_instead_of_replaced_with_empty_board() {
        let f = Fixture::new(); let path = f.service.board_location(&f.key).unwrap();
        fs::write(&path, b"{broken").unwrap();
        let request = BoardRead { workspace_key: f.key.clone(), expected_revision: 0 };
        assert_eq!(f.service.board_read(request, true).unwrap_err().code, "io_error");
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
    }
    #[test]
    fn external_store_lock_and_failed_auth_do_not_change_the_board() {
        let f = Fixture::new(); f.seed();
        let before = serde_json::to_value(f.read()).unwrap();
        let request = BoardRequest { workspace_key: f.key.clone(), expected_revision: 0, board_revision: 2, operation: BoardOperation::TaskDelete { id: "DF-001".into() } };
        assert_eq!(f.service.board_execute(request, false, "你", true).unwrap_err().code, "unauthorized");
        let lock_path = f.service.board_location(&f.key).unwrap().with_extension("lock");
        let file = fs::OpenOptions::new().read(true).write(true).open(lock_path).unwrap();
        file.try_lock().unwrap();
        assert_eq!(f.execute(BoardOperation::TaskDelete { id: "DF-001".into() }, "你", true).unwrap_err().code, "conflict");
        assert_eq!(serde_json::to_value(f.read()).unwrap(), before);
        drop(file);
        f.execute(BoardOperation::TaskDelete { id: "DF-001".into() }, "你", true).unwrap();
    }
}
