//! Workspace-owned visual planning, shared by HTTP and MCP.
use super::*;

const LIMIT: u64 = 12 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdeaNode {
    pub id: String,
    pub title: String,
    #[serde(default)] pub body: String,
    #[serde(default = "node_kind")] pub kind: String,
    pub x: f64, pub y: f64,
    #[serde(default)] pub parent_id: Option<String>,
    #[serde(default)] pub group_id: Option<String>,
    #[serde(default)] pub collapsed: bool,
    #[serde(default)] pub image: String,
    #[serde(default)] pub html: String,
}
fn node_kind() -> String { "note".into() }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdeaEdge { pub id: String, pub source: String, pub target: String, #[serde(default)] pub label: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdeaCanvas {
    pub id: String, pub name: String,
    #[serde(default)] pub nodes: Vec<IdeaNode>,
    #[serde(default)] pub edges: Vec<IdeaEdge>,
    #[serde(default)] pub updated_at: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdeasSnapshot { pub workspace_key: String, pub revision: u64, pub canvases: Vec<IdeaCanvas> }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdeasRead {
    pub workspace_key: String, pub expected_revision: u64,
    pub canvas_id: Option<String>, pub if_revision: Option<u64>,
    #[serde(default)] pub include_media: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdeasRequest {
    pub workspace_key: String, pub expected_revision: u64, pub ideas_revision: u64,
    pub operation: IdeasOperation,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum IdeasOperation {
    CanvasSave { canvas: IdeaCanvas },
    CanvasDelete { id: String },
    Patch { canvas_id: String, name: Option<String>, #[serde(default)] nodes: Vec<Value>, #[serde(default)] edges: Vec<IdeaEdge>, #[serde(default)] delete_nodes: Vec<String>, #[serde(default)] delete_edges: Vec<String> },
}

fn validate_canvas(canvas: &IdeaCanvas) -> Result<()> {
    text(&canvas.id, 100, true)?; text(&canvas.name, 240, true)?;
    if canvas.nodes.len() > 500 || canvas.edges.len() > 1000 { return Err(error("invalid_request", "每张画布最多 500 个节点、1000 条连线")); }
    let mut ids = HashSet::new();
    for n in &canvas.nodes {
        text(&n.id, 100, true)?; text(&n.title, 240, true)?; text(&n.body, 16000, false)?;
        text(&n.html, 512 * 1024, false)?; text(&n.image, 4 * 1024 * 1024, false)?;
        if !ids.insert(n.id.as_str()) || !["note", "group"].contains(&n.kind.as_str())
            || !n.x.is_finite() || !n.y.is_finite() || n.x.abs() > 100000.0 || n.y.abs() > 100000.0 {
            return Err(error("invalid_request", "节点标识、类型或坐标无效"));
        }
        if !n.image.is_empty() {
            let encoded = ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64,", "data:image/gif;base64,"]
                .iter().find_map(|prefix| n.image.strip_prefix(*prefix));
            if encoded.is_none_or(|s| s.is_empty() || s.len() % 4 != 0 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))) {
                return Err(error("invalid_request", "图片需为 PNG/JPEG/WebP/GIF 的 base64 data URL"));
            }
        }
    }
    for n in &canvas.nodes {
        for relation in [&n.parent_id, &n.group_id] {
            if let Some(id) = relation {
                if id == &n.id || !ids.contains(id.as_str()) { return Err(error("invalid_request", "父节点或分组不存在")); }
            }
        }
        if let Some(id) = &n.group_id {
            if !canvas.nodes.iter().any(|g| &g.id == id && g.kind == "group") { return Err(error("invalid_request", "分组类型无效")); }
        }
        // Both containment relationships must be acyclic, including mixed cycles.
        let mut pending = vec![n.id.as_str()];
        let mut visited = HashSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) { continue; }
            let node = canvas.nodes.iter().find(|v| v.id == id).ok_or_else(|| error("invalid_request", "节点不存在"))?;
            for next in [&node.parent_id, &node.group_id].into_iter().flatten() {
                if next == &n.id { return Err(error("invalid_request", "节点不能循环嵌套")); }
                pending.push(next);
            }
        }
    }
    let mut edges = HashSet::new();
    for e in &canvas.edges {
        text(&e.id, 100, true)?; text(&e.label, 240, false)?;
        if e.source == e.target || !ids.contains(e.source.as_str()) || !ids.contains(e.target.as_str()) || !edges.insert(&e.id) {
            return Err(error("invalid_request", "连线标识或端点无效"));
        }
    }
    Ok(())
}

fn load_ideas(path: &Path, key: &str) -> Result<IdeasSnapshot> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(IdeasSnapshot { workspace_key: key.into(), ..Default::default() }),
        Err(e) => return Err(e.into()),
        Ok(m) if m.file_type().is_symlink() || !m.is_file() || m.len() > LIMIT => return Err(error("permission_denied", "构思存储文件类型或大小无效")),
        _ => (),
    }
    let data: IdeasSnapshot = serde_json::from_slice(&fs::read(path)?).map_err(|e| error("io_error", format!("构思数据损坏，原文件已保留：{e}")))?;
    if data.workspace_key != key { return Err(error("conflict", "构思数据不属于当前工作区")); }
    validate_ideas(&data)?;
    Ok(data)
}
fn validate_ideas(data: &IdeasSnapshot) -> Result<()> {
    if data.canvases.len() > 30 { return Err(error("invalid_request", "每个工作区最多 30 张画布")); }
    let mut ids = HashSet::new();
    for canvas in &data.canvases {
        if !ids.insert(&canvas.id) { return Err(error("invalid_request", "画布标识重复")); }
        validate_canvas(canvas)?;
    }
    Ok(())
}

impl WorkspaceService {
    pub fn ideas_read(&self, request: IdeasRead, authenticated: bool) -> Result<Option<IdeasSnapshot>> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let state = self.lock()?;
        self.check_board_scope(&state, &request.workspace_key, request.expected_revision)?;
        let mut data = load_ideas(&self.board_location(&request.workspace_key)?.with_extension("ideas.json"), &request.workspace_key)?;
        if request.if_revision == Some(data.revision) { return Ok(None); }
        if let Some(id) = request.canvas_id { data.canvases.retain(|c| c.id == id); }
        if !request.include_media {
            for canvas in &mut data.canvases { for node in &mut canvas.nodes { node.image.clear(); node.html.clear(); } }
        }
        Ok(Some(data))
    }

    pub fn ideas_execute(&self, request: IdeasRequest, authenticated: bool) -> Result<IdeasSnapshot> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        let state = self.lock()?;
        self.check_board_scope(&state, &request.workspace_key, request.expected_revision)?;
        let path = self.board_location(&request.workspace_key)?.with_extension("ideas.json");
        let lock_path = path.with_extension("lock");
        if let Ok(m) = fs::symlink_metadata(&lock_path) {
            if m.file_type().is_symlink() || !m.is_file() { return Err(error("permission_denied", "构思锁文件无效")); }
        }
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path)?;
        lock.try_lock().map_err(|_| error("conflict", "构思正在保存，请稍后重试"))?;
        let mut data = load_ideas(&path, &request.workspace_key)?;
        if data.revision != request.ideas_revision { return Err(error("conflict", "画布已被其他入口修改，请刷新后合并；本地编辑尚未保存")); }
        match request.operation {
            IdeasOperation::CanvasSave { mut canvas } => {
                canvas.updated_at = now_seconds();
                if let Some(current) = data.canvases.iter_mut().find(|c| c.id == canvas.id) { *current = canvas; }
                else { data.canvases.push(canvas); }
            }
            IdeasOperation::CanvasDelete { id } => {
                if !data.canvases.iter().any(|c| c.id == id) { return Err(error("not_found", "画布不存在")); }
                data.canvases.retain(|c| c.id != id);
            }
            IdeasOperation::Patch { canvas_id, name, nodes, edges, delete_nodes, delete_edges } => {
                let canvas = data.canvases.iter_mut().find(|c| c.id == canvas_id).ok_or_else(|| error("not_found", "画布不存在"))?;
                if let Some(name) = name { canvas.name = name; }
                let mut node_ids = HashSet::new(); let mut edge_ids = HashSet::new();
                for patch in nodes {
                    let id = patch.get("id").and_then(Value::as_str).ok_or_else(|| error("invalid_request", "节点修改需要 id"))?;
                    if !node_ids.insert(id.to_owned()) || delete_nodes.iter().any(|v| v == id) { return Err(error("invalid_request", "节点修改重复或与删除冲突")); }
                    let mut value = match canvas.nodes.iter().find(|n| n.id == id) {
                        Some(n) => serde_json::to_value(n).map_err(|e| error("io_error", e.to_string()))?,
                        None => json!({}),
                    };
                    let fields = patch.as_object().ok_or_else(|| error("invalid_request", "节点修改必须为对象"))?;
                    for (key, field) in fields { value[key] = field.clone(); }
                    let n: IdeaNode = serde_json::from_value(value).map_err(|e| error("invalid_request", e.to_string()))?;
                    if let Some(current) = canvas.nodes.iter_mut().find(|v| v.id == n.id) { *current = n; } else { canvas.nodes.push(n); }
                }
                canvas.nodes.retain(|n| !delete_nodes.contains(&n.id));
                for n in &mut canvas.nodes {
                    if n.parent_id.as_ref().is_some_and(|id| delete_nodes.contains(id)) { n.parent_id = None; }
                    if n.group_id.as_ref().is_some_and(|id| delete_nodes.contains(id)) { n.group_id = None; }
                }
                canvas.edges.retain(|e| !delete_edges.contains(&e.id) && !delete_nodes.contains(&e.source) && !delete_nodes.contains(&e.target));
                for e in edges {
                    if !edge_ids.insert(e.id.clone()) || delete_edges.contains(&e.id) { return Err(error("invalid_request", "连线修改重复或与删除冲突")); }
                    if let Some(current) = canvas.edges.iter_mut().find(|v| v.id == e.id) { *current = e; } else { canvas.edges.push(e); }
                }
                canvas.updated_at = now_seconds();
            }
        }
        validate_ideas(&data)?;
        data.revision = data.revision.checked_add(1).ok_or_else(|| error("conflict", "构思版本已达上限"))?;
        let bytes = serde_json::to_vec_pretty(&data).map_err(|e| error("io_error", e.to_string()))?;
        if bytes.len() as u64 > LIMIT {
            return Err(error("invalid_request", "构思数据超过 12 MiB，请减少图片体积"));
        }
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            file.write_all(&bytes)?; file.sync_all()?; drop(file);
            fs::rename(&temporary, &path)?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result?;
        Ok(data)
    }
}
