//! Persist the last successful workspace location, never unsaved documents or tokens.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecentWorkspace {
    version: u8,
    path: Option<String>,
    roots: Vec<String>,
    #[serde(default)]
    history: Vec<RecentLocation>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecentLocation { path: String, roots: Vec<String> }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentWorkspaceEntry {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
    pub available: bool,
}

fn read_recent(path: &Path) -> Result<RecentWorkspace> {
    let source = read_workspace(path)?;
    let mut recent: RecentWorkspace = serde_json::from_str(source.trim_start_matches('\u{feff}'))
        .map_err(|_| error("invalid_workspace", "最近工作区记录损坏"))?;
    if recent.version != 1 || recent.roots.len() > MAX_FOLDERS || recent.history.len() > 20
        || recent.history.iter().any(|entry| entry.roots.len() > MAX_FOLDERS) {
        return Err(error("invalid_workspace", "最近工作区记录格式不受支持"));
    }
    // Migrate the previous single-location format without losing its last location.
    if let Some(path) = &recent.path {
        if !recent.history.iter().any(|entry| comparison_path(Path::new(&entry.path)) == comparison_path(Path::new(path))) {
            recent.history.insert(0, RecentLocation { path: path.clone(), roots: recent.roots.clone() });
            recent.history.truncate(20);
        }
    }
    Ok(recent)
}

impl WorkspaceService {
    /// Called once at startup with a server-owned path, never a client argument.
    /// Explicit launch locations can override restoration; all future operations persist.
    pub fn configure_recent(&self, record_path: PathBuf, initial: Option<PathBuf>, restore: bool) -> Result<()> {
        validate_absolute(&record_path)?;
        let mut state = self.lock()?;
        *self.recent_path.lock().map_err(|_| error("unavailable", "最近工作区记录暂不可用"))? = Some(record_path.clone());
        if restore && record_path.exists() {
            match self.restore_recent(&record_path) {
                Ok(session) => state.session = session,
                Err(err) => {
                    state.session = None;
                    state.notice = Some(format!("无法恢复最近工作区：{err}。请重新选择工作区。"));
                }
            }
        } else if let Some(initial) = initial {
            state.session = Some(self.open_session(&initial)?);
            state.notice = self.persist_recent(&state).err().map(|err| format!("无法记住当前位置：{err}"));
        }
        Ok(())
    }

    fn restore_recent(&self, record_path: &Path) -> Result<Option<Session>> {
        let recent = read_recent(record_path)?;
        let Some(selected) = recent.path else { return Ok(None); };
        self.open_remembered(&selected, &recent.roots)
    }

    fn open_remembered(&self, selected: &str, remembered_roots: &[String]) -> Result<Option<Session>> {
        let selected = Path::new(&selected);
        validate_absolute(selected)?;
        let canonical = selected.canonicalize()?;
        // Retargeted links must not silently change the scope remembered last time.
        if comparison_path(&canonical) != comparison_path(selected) {
            return Err(error("outside_roots", "最近工作区路径指向已改变"));
        }
        let mut roots = Vec::new();
        for root in remembered_roots {
            let root = PathBuf::from(root);
            validate_absolute(&root)?;
            if let Ok(resolved) = root.canonicalize() {
                if resolved.is_dir() && comparison_path(&resolved) == comparison_path(&root) {
                    roots.push(resolved);
                }
            }
        }
        if canonical.is_dir() && !roots.contains(&canonical) {
            return Err(error("outside_roots", "最近文件夹不在记忆的授权范围内"));
        }
        if !canonical.is_dir() && !is_workspace_file(&canonical) {
            return Err(error("invalid_path", "最近位置不是工作区文件或文件夹"));
        }
        if canonical.is_file() { parse_document(&read_workspace(&canonical)?)?; }
        {
            let mut grants = self.local_grants.lock().map_err(|_| error("unavailable", "本机位置暂不可用"))?;
            grants.roots.extend(roots);
            grants.roots.sort();
            grants.roots.dedup();
            if canonical.is_file() && !grants.files.contains(&canonical) { grants.files.push(canonical.clone()); }
        }
        self.open_session(&canonical).map(Some)
    }

    pub(super) fn recent_entries(&self) -> Result<Vec<RecentWorkspaceEntry>> {
        let record = self.recent_path.lock().map_err(|_| error("unavailable", "最近工作区记录暂不可用"))?.clone();
        let Some(record) = record.filter(|path| path.exists()) else { return Ok(Vec::new()); };
        let recent = read_recent(&record)?;
        Ok(recent.history.into_iter().filter_map(|entry| {
            let path = Path::new(&entry.path);
            if validate_absolute(path).is_err() { return None; }
            let workspace_file = is_workspace_file(path) && !path.is_dir();
            let available = path.canonicalize().ok().is_some_and(|resolved| {
                comparison_path(&resolved) == comparison_path(path)
                    && (resolved.is_dir() || (workspace_file && resolved.is_file()))
            });
            let name = if workspace_file { path.file_stem() } else { path.file_name() }
                .map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| display_path(path));
            Some(RecentWorkspaceEntry { path: display_path(path), name, kind: if workspace_file { "workspace_file" } else { "folder" }, available })
        }).collect())
    }

    pub(super) fn open_recent_session(&self, selected: &str) -> Result<Session> {
        let record = self.recent_path.lock().map_err(|_| error("unavailable", "最近工作区记录暂不可用"))?.clone()
            .ok_or_else(|| error("not_found", "暂无最近打开记录，请从本机选择"))?;
        let recent = read_recent(&record)?;
        let key = comparison_path(Path::new(selected));
        let location = recent.history.into_iter().find(|entry| comparison_path(Path::new(&entry.path)) == key)
            .ok_or_else(|| error("not_found", "此位置不在最近打开列表中，请从本机选择"))?;
        self.open_remembered(&location.path, &location.roots)?
            .ok_or_else(|| error("not_found", "最近位置无法打开"))
    }

    pub(super) fn persist_recent(&self, state: &State) -> Result<()> {
        let record_path = self.recent_path.lock().map_err(|_| error("unavailable", "最近工作区记录暂不可用"))?.clone();
        let Some(record_path) = record_path else { return Ok(()); };
        let mut roots = Vec::new();
        let path = if let Some(session) = &state.session {
            let document = parse_document(&session.source)?;
            for entry in folder_entries(&document)? {
                if let Ok(root) = resolve_folder(entry, &session.base).and_then(|p| self.authorize(&p)) {
                    if root.is_dir() { roots.push(display_path(&root)); }
                }
            }
            // A folder session must retain its original base even if all roots were removed.
            if session.file.is_none() { roots.push(display_path(&session.base)); }
            Some(display_path(session.file.as_deref().unwrap_or(&session.base)))
        } else { None };
        roots.sort();
        roots.dedup();
        let mut history = read_recent(&record_path).map(|recent| recent.history).unwrap_or_default();
        if let Some(selected) = &path {
            let key = comparison_path(Path::new(selected));
            history.retain(|entry| comparison_path(Path::new(&entry.path)) != key);
            history.insert(0, RecentLocation { path: selected.clone(), roots: roots.clone() });
            history.truncate(20);
        }
        let recent = RecentWorkspace { version: 1, path, roots, history };
        let source = serde_json::to_string_pretty(&recent).map_err(|err| error("io_error", err.to_string()))? + "\n";
        let directory = parent(&record_path)?;
        fs::create_dir_all(directory)?;
        let temp = directory.join(format!(".recent-{}.tmp", uuid::Uuid::new_v4()));
        self.write_new(&temp, &source)?;
        if let Err(err) = fs::rename(&temp, &record_path) {
            let _ = fs::remove_file(&temp);
            return Err(err.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("devflow-recent-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn record(&self) -> PathBuf { self.0.join(".devflow/recent-workspace.json") }
        fn open(&self, service: &WorkspaceService, path: &Path) {
            let revision = service.lock().unwrap().revision;
            service.execute(WorkspaceRequest {
                expected_revision: revision, discard_changes: false,
                operation: WorkspaceOperation::Open { path: display_path(path) },
            }, true).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.0.file_name().is_some_and(|name| name.to_string_lossy().starts_with("devflow-recent-test-")) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn restores_latest_folder_instead_of_launchers_initial_root() {
        let fixture = Fixture::new();
        let selected = fixture.0.join("最近 中文项目");
        fs::create_dir(&selected).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        service.configure_recent(fixture.record(), Some(fixture.0.clone()), true).unwrap();
        fixture.open(&service, &selected);
        let restarted = WorkspaceService::new(vec![], None).unwrap();
        restarted.configure_recent(fixture.record(), Some(fixture.0.clone()), true).unwrap();
        assert_eq!(restarted.snapshot().unwrap().workspace_root, Some(display_path(&selected)));
        assert_eq!(restarted.authorize(&fixture.0).unwrap_err().code, "outside_roots");
    }

    #[test]
    fn restores_exact_workspace_file_and_scoped_project_roots() {
        let fixture = Fixture::new();
        let project = fixture.0.join("项目");
        let config_dir = fixture.0.join("工作区");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&config_dir).unwrap();
        let file = config_dir.join("最近.code-workspace");
        fs::write(&file, r#"{"folders":[{"path":"../项目"}],"settings":{}}"#).unwrap();
        let service = WorkspaceService::new(vec![], None).unwrap();
        service.configure_recent(fixture.record(), None, true).unwrap();
        service.accept_local_selection(WorkspacePickRequest { kind: WorkspacePickKind::WorkspaceFile, expected_revision: 0 }, Some(display_path(&file))).unwrap();
        fixture.open(&service, &file);
        let restarted = WorkspaceService::new(vec![], None).unwrap();
        restarted.configure_recent(fixture.record(), None, true).unwrap();
        let snapshot = restarted.snapshot().unwrap();
        assert_eq!(snapshot.workspace_file, Some(display_path(&file)));
        assert_eq!(snapshot.folders[0].status, "available");
        assert_eq!(restarted.authorize(&config_dir).unwrap_err().code, "outside_roots");
        assert_eq!(restarted.authorize(&fixture.0).unwrap_err().code, "outside_roots");
    }

    #[test]
    fn close_stays_closed_and_explicit_launch_overrides_recent() {
        let fixture = Fixture::new();
        let other = fixture.0.join("other");
        fs::create_dir(&other).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        service.configure_recent(fixture.record(), Some(fixture.0.clone()), true).unwrap();
        fixture.open(&service, &other);
        let explicit = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        explicit.configure_recent(fixture.record(), Some(fixture.0.clone()), false).unwrap();
        assert_eq!(explicit.snapshot().unwrap().workspace_root, Some(display_path(&fixture.0)));
        explicit.execute(WorkspaceRequest {
            expected_revision: 0, discard_changes: false, operation: WorkspaceOperation::Close,
        }, true).unwrap();
        let restarted = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        restarted.configure_recent(fixture.record(), Some(fixture.0.clone()), true).unwrap();
        assert!(restarted.snapshot().unwrap().folders.is_empty());
    }

    #[test]
    fn invalid_or_missing_recent_location_shows_empty_workspace_and_notice() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.record().parent().unwrap()).unwrap();
        fs::write(fixture.record(), "not json").unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        service.configure_recent(fixture.record(), Some(fixture.0.clone()), true).unwrap();
        let snapshot = service.snapshot().unwrap();
        assert!(snapshot.folders.is_empty());
        assert!(!snapshot.warnings.is_empty());
        fs::write(fixture.record(), serde_json::to_string(&RecentWorkspace {
            version: 1, path: Some(display_path(&fixture.0.join("removed"))), roots: vec![], history: vec![],
        }).unwrap()).unwrap();
        let restarted = WorkspaceService::new(vec![], None).unwrap();
        restarted.configure_recent(fixture.record(), None, true).unwrap();
        assert!(!restarted.snapshot().unwrap().warnings.is_empty());
    }

    #[test]
    fn recent_list_survives_close_deduplicates_and_reopens_with_scoped_access() {
        let fixture = Fixture::new();
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        service.configure_recent(fixture.record(), None, true).unwrap();
        fixture.open(&service, &first);
        fixture.open(&service, &second);
        fixture.open(&service, &first);
        let revision = service.snapshot().unwrap().revision;
        service.execute(WorkspaceRequest {
            expected_revision: revision, discard_changes: false, operation: WorkspaceOperation::Close,
        }, true).unwrap();
        let restarted = WorkspaceService::new(vec![], None).unwrap();
        restarted.configure_recent(fixture.record(), None, true).unwrap();
        assert!(restarted.snapshot().unwrap().folders.is_empty());
        let entries = restarted.catalog().unwrap().recent_workspaces;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, display_path(&first));
        assert_eq!(entries[1].path, display_path(&second));
        assert_eq!(restarted.authorize(&first).unwrap_err().code, "outside_roots");
        let request = || WorkspaceRequest {
            expected_revision: 0, discard_changes: false,
            operation: WorkspaceOperation::OpenRecent { path: display_path(&first) },
        };
        assert_eq!(restarted.execute(request(), false).unwrap_err().code, "unauthorized");
        assert_eq!(restarted.execute(request(), true).unwrap().workspace_root, Some(display_path(&first)));
        assert_eq!(restarted.authorize(&second).unwrap_err().code, "outside_roots");
        assert_eq!(restarted.authorize(&fixture.0).unwrap_err().code, "outside_roots");
    }

    #[test]
    fn recent_list_is_bounded_and_legacy_records_migrate() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.record().parent().unwrap()).unwrap();
        let initial = fixture.0.join("legacy");
        fs::create_dir(&initial).unwrap();
        fs::write(fixture.record(), serde_json::to_string(&json!({
            "version": 1, "path": display_path(&initial), "roots": [display_path(&initial)]
        })).unwrap()).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        service.configure_recent(fixture.record(), None, true).unwrap();
        assert_eq!(service.catalog().unwrap().recent_workspaces.len(), 1);
        for index in 0..22 {
            let directory = fixture.0.join(format!("project-{index}"));
            fs::create_dir(&directory).unwrap();
            fixture.open(&service, &directory);
        }
        let entries = service.catalog().unwrap().recent_workspaces;
        assert_eq!(entries.len(), 20);
        assert_eq!(entries[0].name, "project-21");
        assert_eq!(entries[19].name, "project-2");
        fs::remove_dir(fixture.0.join("project-21")).unwrap();
        assert!(!service.catalog().unwrap().recent_workspaces[0].available);
        let revision = service.snapshot().unwrap().revision;
        assert_eq!(service.execute(WorkspaceRequest {
            expected_revision: revision, discard_changes: false,
            operation: WorkspaceOperation::OpenRecent { path: display_path(&fixture.0) },
        }, true).unwrap_err().code, "not_found");
    }
}
