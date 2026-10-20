//! Search follows VS Code's ripgrep provider design; no VS Code code is copied.
use super::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};

const SEARCH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RECORD: u64 = 128 * 1024;

fn ripgrep_executable(application: &Path, explicit: Option<std::ffi::OsString>) -> Result<PathBuf> {
    if let Some(explicit) = explicit {
        let path = PathBuf::from(explicit);
        if !path.is_absolute() || !path.is_file() {
            return Err(error("search_unavailable", "DEVFLOW_RG_PATH 必须指向已存在的 rg 可执行文件的绝对路径"));
        }
        return Ok(path);
    }
    let name = if cfg!(windows) { "rg.exe" } else { "rg" };
    if let Some(directory) = application.parent() {
        // Release bundle: <app>/tools/ripgrep. Source builds: <repo>/target/{debug,release}.
        let mut directories = vec![directory.to_owned()];
        if directory.file_name().is_some_and(|name| name == "debug" || name == "release") {
            if let Some(target) = directory.parent() {
                if target.file_name().is_some_and(|name| name == "target") {
                    if let Some(repo) = target.parent() { directories.push(repo.to_owned()); }
                }
            }
        }
        for directory in directories {
            let candidate = directory.join("tools").join("ripgrep").join(name);
            if candidate.is_file() { return Ok(candidate); }
        }
    }
    // Resolve PATH ourselves: never let OS executable lookup search a managed
    // project's current directory or a relative PATH entry.
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
            let candidate = directory.join(name);
            if candidate.is_file() { return Ok(candidate); }
        }
    }
    Err(error("search_unavailable", "搜索组件缺失，请使用完整发布包；源码开发请先运行 pnpm install 或 pnpm tools:prepare"))
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode { Files, Content, #[default] Both }

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSearchRequest {
    pub query: String,
    #[serde(default)]
    pub mode: SearchMode,
    pub expected_revision: u64,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub include_glob: String,
    #[serde(default)]
    pub exclude_glob: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub path: String,
    pub root: String,
    pub relative_path: String,
    pub line: Option<u64>,
    pub column: Option<usize>,
    pub preview: Option<String>,
    pub preview_matches: Vec<SearchRange>,
}

#[derive(Debug, Serialize)]
pub struct SearchRange { pub start: usize, pub end: usize }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearch {
    pub revision: u64,
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

// Always reap rg, including on malformed output, errors, limits and timeouts.
struct SearchProcess(Child);
impl Drop for SearchProcess {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}

impl WorkspaceService {
    pub fn search(&self, request: WorkspaceSearchRequest, authenticated: bool) -> Result<WorkspaceSearch> {
        if !authenticated { return Err(error("unauthorized", "需要有效的访问令牌")); }
        if request.query.trim().is_empty() || request.query.len() > 1024 || request.query.contains(['\0', '\n', '\r']) {
            return Err(error("invalid_query", "请输入 1–1024 字节的单行搜索词"));
        }
        if request.mode == SearchMode::Files && (request.regex || request.whole_word) {
            return Err(error("invalid_query", "文件名搜索支持路径片段匹配，整词和正则仅用于内容搜索"));
        }
        for glob in [&request.include_glob, &request.exclude_glob] {
            if glob.len() > 1024 || glob.contains(['\0', '\n', '\r']) || glob.trim().starts_with('!') {
                return Err(error("invalid_query", "包含/排除文件各支持一个不超过 1024 字节的 glob，请勿使用换行或开头的 !"));
            }
        }
        let roots = {
            let state = self.lock()?;
            if state.revision != request.expected_revision { return Err(error("conflict", "工作区已改变，请重新搜索")); }
            let session = state.session.as_ref().ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
            let document = parse_document(&session.source)?;
            let mut roots = Vec::new();
            for entry in folder_entries(&document)? {
                if let Ok(root) = resolve_folder(entry, &session.base).and_then(|p| self.authorize(&p)) {
                    if root.is_dir() && !roots.contains(&root) && !root.components().any(|c| matches!(c, Component::Normal(n) if excluded_directory(&n.to_string_lossy()))) {
                        roots.push(root);
                    }
                }
            }
            roots
        };
        let mut result = WorkspaceSearch { revision: request.expected_revision, matches: Vec::new(), truncated: false, warnings: Vec::new() };
        let started = Instant::now();
        let mut seen = HashSet::new();
        let mut candidates = 0;
        for root in roots {
            if started.elapsed() >= SEARCH_TIMEOUT { result.truncated = true; break; }
            if request.mode == SearchMode::Both {
                let mut content = request.clone();
                content.mode = SearchMode::Content;
                self.search_root(&root, &content, started, &mut result, &mut seen, &mut candidates)?;
                if !result.truncated {
                    let mut files = request.clone();
                    files.mode = SearchMode::Files; files.regex = false; files.whole_word = false;
                    self.search_root(&root, &files, started, &mut result, &mut seen, &mut candidates)?;
                }
            } else {
                self.search_root(&root, &request, started, &mut result, &mut seen, &mut candidates)?;
            }
            if result.truncated { break; }
        }
        if self.lock()?.revision != request.expected_revision { return Err(error("conflict", "工作区已改变，请重新搜索")); }
        if result.truncated { result.warnings.push("已达到搜索限制（200 条结果、20000 个候选文件或 5 秒），请缩小搜索范围。".into()); }
        Ok(result)
    }

    fn search_root(&self, root: &Path, request: &WorkspaceSearchRequest, started: Instant, result: &mut WorkspaceSearch, seen: &mut HashSet<(PathBuf, Option<u64>)>, candidates: &mut usize) -> Result<()> {
        let executable = ripgrep_executable(&std::env::current_exe()?, std::env::var_os("DEVFLOW_RG_PATH"))?;
        let mut command = Command::new(executable);
        command.current_dir(root).args(["--no-config", "--color", "never", "--hidden", "--no-follow", "--no-require-git"]);
        if !request.include_glob.trim().is_empty() { command.args(["--glob", request.include_glob.trim()]); }
        if !request.exclude_glob.trim().is_empty() { command.args(["--glob", &format!("!{}", request.exclude_glob.trim())]); }
        // Case-insensitive globs also cover Windows dependency/build directory variants.
        for name in ["node_modules", "target", ".git", "build", "dist"] {
            command.args(["--iglob", &format!("!**/{name}")]);
            command.args(["--iglob", &format!("!**/{name}/**")]);
        }
        if request.mode == SearchMode::Files {
            command.args(["--files", "--null"]);
        } else {
            command.args(["--json", "--crlf", "--max-filesize", "1M", "--encoding", "utf-8", "--threads", "2"]);
            command.arg(if request.case_sensitive { "--case-sensitive" } else { "--ignore-case" });
            if !request.regex { command.arg("--fixed-strings"); }
            if request.whole_word { command.arg("--word-regexp"); }
            command.args(["--regexp", &request.query]);
        }
        command.arg("--").arg(".").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let child = command.spawn().map_err(|e| error("search_unavailable", format!("搜索组件缺失或无法启动，请使用完整发布包；源码开发请先运行 pnpm install（或 pnpm tools:prepare）：{e}")))?;
        let mut process = SearchProcess(child);
        let stdout = process.0.stdout.take().ok_or_else(|| error("io_error", "无法读取搜索输出"))?;
        let stderr = process.0.stderr.take().ok_or_else(|| error("io_error", "无法读取搜索错误"))?;
        let (sender, receiver) = mpsc::sync_channel(8);
        let delimiter = if request.mode == SearchMode::Files { 0 } else { b'\n' };
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let read = reader.by_ref().take(MAX_RECORD + 1).read_until(delimiter, &mut bytes);
                match read {
                    Ok(0) => break,
                    Ok(_) if bytes.len() as u64 <= MAX_RECORD => { if sender.send(Ok(bytes)).is_err() { break; } }
                    _ => { let _ = sender.send(Err(())); break; }
                }
            }
        });
        let errors = std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut bytes = Vec::new();
            let mut buffer = [0; 1024];
            while let Ok(count) = stderr.read(&mut buffer) {
                if count == 0 { break; }
                let keep = count.min(8192usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..keep]);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        });
        let query = if request.case_sensitive { request.query.replace('\\', "/") } else { request.query.replace('\\', "/").to_lowercase() };
        let outcome = (|| -> Result<()> {
            loop {
                let Some(remaining) = SEARCH_TIMEOUT.checked_sub(started.elapsed()) else { result.truncated = true; break; };
                let bytes = match receiver.recv_timeout(remaining) {
                    Ok(Ok(bytes)) => bytes,
                    Ok(Err(())) => { result.truncated = true; result.warnings.push("部分搜索输出过长或读取失败，结果不完整。".into()); break; }
                    Err(RecvTimeoutError::Timeout) => { result.truncated = true; break; }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                let (relative, line, column, preview, preview_matches) = if request.mode == SearchMode::Files {
                    *candidates += 1;
                    if *candidates > 20000 { result.truncated = true; break; }
                    let Ok(path) = std::str::from_utf8(&bytes[..bytes.len().saturating_sub(1)]) else { continue; };
                    let normalized = path.replace('\\', "/");
                    let comparable = if request.case_sensitive { normalized.clone() } else { normalized.to_lowercase() };
                    if !comparable.contains(&query) { continue; }
                    (path.to_owned(), None, None, None, Vec::new())
                } else {
                    let event: Value = serde_json::from_slice(&bytes).map_err(|_| error("io_error", "无法解析 ripgrep 输出"))?;
                    if event["type"] != "match" { continue; }
                    let data = &event["data"];
                    let (Some(path), Some(text), Some(line)) = (data["path"]["text"].as_str(), data["lines"]["text"].as_str(), data["line_number"].as_u64()) else { continue; };
                    let offset = data["submatches"][0]["start"].as_u64().unwrap_or(0) as usize;
                    let prefix = text.get(..offset).unwrap_or("");
                    // UTF-16 columns match the browser editor, including astral characters.
                    let column = prefix.encode_utf16().count() + 1 - usize::from(line == 1 && prefix.starts_with('\u{feff}'));
                    let start = prefix.chars().count().saturating_sub(80);
                    let preview: String = text.trim_end_matches(['\r', '\n']).chars().skip(start).take(300).collect();
                    let byte_start = text.char_indices().nth(start).map(|(byte, _)| byte).unwrap_or(text.len());
                    let byte_end = byte_start + preview.len();
                    let preview_matches = data["submatches"].as_array().into_iter().flatten().take(200).filter_map(|matched| {
                        let start = (matched["start"].as_u64()? as usize).max(byte_start);
                        let end = (matched["end"].as_u64()? as usize).min(byte_end);
                        if start >= end { return None; }
                        Some(SearchRange {
                            start: text.get(byte_start..start)?.encode_utf16().count(),
                            end: text.get(byte_start..end)?.encode_utf16().count(),
                        })
                    }).collect();
                    (path.to_owned(), Some(line), Some(column), Some(preview), preview_matches)
                };
                let relative = Path::new(&relative);
                if relative.is_absolute() || relative.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)) || matches!(c, Component::Normal(n) if excluded_directory(&n.to_string_lossy()))) { continue; }
                let candidate = root.join(relative);
                let Ok(path) = self.authorize(&candidate) else { continue; };
                if !path.starts_with(root) || !path.is_file() || fs::symlink_metadata(&candidate).map(|m| m.file_type().is_symlink()).unwrap_or(true) { continue; }
                if !seen.insert((path.clone(), line)) { continue; }
                if result.matches.len() >= 200 { result.truncated = true; break; }
                result.matches.push(SearchMatch { relative_path: display_path(path.strip_prefix(root).unwrap_or(&path)), path: display_path(&path), root: display_path(root), line, column, preview, preview_matches });
            }
            Ok(())
        })();
        // Drop receiver before joining so a reader blocked on send can exit.
        while !result.truncated && outcome.is_ok() && process.0.try_wait()?.is_none() {
            if started.elapsed() >= SEARCH_TIMEOUT { result.truncated = true; break; }
            std::thread::sleep(Duration::from_millis(5));
        }
        if result.truncated || outcome.is_err() { let _ = process.0.kill(); }
        let status = process.0.wait();
        drop(receiver);
        let _ = reader.join();
        let stderr = errors.join().unwrap_or_default();
        outcome?;
        if !result.truncated && status?.code().is_none_or(|code| code > 1) {
            return Err(error("search_failed", format!("搜索失败：{}", stderr.trim())));
        }
        if !stderr.trim().is_empty() && !result.truncated { result.warnings.push(stderr); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("devflow-search-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn service(&self) -> WorkspaceService {
            WorkspaceService::new(vec![self.0.clone()], Some(self.0.clone())).unwrap()
        }
        fn request(query: &str, mode: SearchMode) -> WorkspaceSearchRequest {
            WorkspaceSearchRequest { query: query.into(), mode, expected_revision: 0, case_sensitive: false, regex: false, whole_word: false, include_glob: String::new(), exclude_glob: String::new() }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.0.file_name().is_some_and(|name| name.to_string_lossy().starts_with("devflow-search-test-")) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn resolves_bundled_search_for_release_and_source_builds_without_using_cwd() {
        let fixture = Fixture::new();
        let name = if cfg!(windows) { "rg.exe" } else { "rg" };
        let tools = fixture.0.join("tools/ripgrep");
        fs::create_dir_all(&tools).unwrap();
        let bundled = tools.join(name);
        fs::write(&bundled, "fixture only; never executed").unwrap();
        let source = fixture.0.join("target/debug/devflow.exe");
        assert_eq!(ripgrep_executable(&source, None).unwrap(), bundled);
        assert_eq!(ripgrep_executable(&fixture.0.join("devflow.exe"), None).unwrap(), bundled);
        let override_path = fixture.0.join("custom-rg.exe");
        fs::write(&override_path, "override fixture").unwrap();
        assert_eq!(ripgrep_executable(&source, Some(override_path.clone().into_os_string())).unwrap(), override_path);
        assert_eq!(ripgrep_executable(&source, Some("relative/rg.exe".into())).unwrap_err().code, "search_unavailable");
        assert_eq!(ripgrep_executable(&source, Some(fixture.0.join("missing.exe").into_os_string())).unwrap_err().code, "search_unavailable");
        if let Ok(fallback) = ripgrep_executable(&fixture.0.join("unrelated/bin/devflow.exe"), None) {
            assert!(fallback.is_absolute());
            assert_ne!(fallback, bundled);
        }
    }

    #[test]
    fn rejects_unauthorized_invalid_and_stale_searches_before_spawning() {
        let fixture = Fixture::new();
        let service = fixture.service();
        assert_eq!(service.search(Fixture::request("needle", SearchMode::Files), false).unwrap_err().code, "unauthorized");
        for query in ["", "  ", "a\nb", "a\rb", "a\0b", &"中".repeat(342)] {
            assert_eq!(service.search(Fixture::request(query, SearchMode::Content), true).unwrap_err().code, "invalid_query");
        }
        let mut request = Fixture::request("needle", SearchMode::Files);
        request.regex = true;
        assert_eq!(service.search(request, true).unwrap_err().code, "invalid_query");
        let mut request = Fixture::request("needle", SearchMode::Content);
        request.expected_revision = 1;
        assert_eq!(service.search(request, true).unwrap_err().code, "conflict");
        let empty = WorkspaceService::new(vec![fixture.0.clone()], None).unwrap();
        assert_eq!(empty.search(Fixture::request("needle", SearchMode::Files), true).unwrap_err().code, "no_workspace");
        assert!(serde_json::from_value::<WorkspaceSearchRequest>(json!({"query":"a","mode":"content","expectedRevision":0,"path":"C:/private"})).is_err());
        let unified = serde_json::from_value::<WorkspaceSearchRequest>(json!({"query":"a","expectedRevision":0})).unwrap();
        assert!(unified.mode == SearchMode::Both);
    }

    #[test]
    #[ignore = "Requires bundled rg or PATH; run explicitly for unified search acceptance"]
    fn unified_search_combines_filename_only_and_content_only_matches() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("needle-中文.txt"), "filename only").unwrap();
        fs::write(fixture.0.join("source.txt"), "needle content\r\n").unwrap();
        let result = fixture.service().search(Fixture::request("needle", SearchMode::Both), true).unwrap();
        assert!(!result.truncated);
        assert_eq!(result.matches.len(), 2);
        assert!(result.matches.iter().any(|matched| matched.line.is_none() && matched.path.ends_with("needle-中文.txt")));
        assert!(result.matches.iter().any(|matched| matched.line == Some(1) && matched.path.ends_with("source.txt")));
    }

    #[test]
    #[ignore = "Requires ripgrep in PATH or DEVFLOW_RG_PATH; run explicitly for search acceptance"]
    fn searches_chinese_paths_utf16_columns_ignores_and_literal_options() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("源码")).unwrap();
        fs::write(fixture.0.join("源码/中文 Test.txt"), "😀中文 Needle\r\nneedle needles\r\n--help\r\n").unwrap();
        fs::write(fixture.0.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(fixture.0.join("ignored.txt"), "needle").unwrap();
        fs::write(fixture.0.join("binary.bin"), b"\0needle").unwrap();
        let mut large = vec![b'a'; 1024 * 1024 + 1];
        large[..6].copy_from_slice(b"needle");
        fs::write(fixture.0.join("large.txt"), large).unwrap();
        for name in ["NODE_MODULES", "target", ".git", "build", "dist"] {
            fs::create_dir(fixture.0.join(name)).unwrap();
            fs::write(fixture.0.join(name).join("excluded.txt"), "needle").unwrap();
        }
        let service = fixture.service();
        let files = service.search(Fixture::request("源码\\中文 test", SearchMode::Files), true).unwrap();
        assert_eq!(files.matches.len(), 1);
        assert!(files.matches[0].path.ends_with("中文 Test.txt"));
        let result = service.search(Fixture::request("needle", SearchMode::Content), true).unwrap();
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].line, Some(1));
        assert_eq!(result.matches[0].column, Some(6));
        assert_eq!(result.matches[0].preview_matches[0].start, 5);
        assert_eq!(result.matches[0].preview_matches[0].end, 11);
        assert!(!result.truncated);
        let mut request = Fixture::request("Needle", SearchMode::Content);
        request.case_sensitive = true;
        assert_eq!(service.search(request, true).unwrap().matches.len(), 1);
        let mut request = Fixture::request("needles", SearchMode::Content);
        request.whole_word = true;
        assert_eq!(service.search(request, true).unwrap().matches.len(), 1);
        let mut request = Fixture::request("Needle[s]?", SearchMode::Content);
        request.regex = true;
        assert_eq!(service.search(request, true).unwrap().matches.len(), 2);
        assert_eq!(service.search(Fixture::request("--help", SearchMode::Content), true).unwrap().matches.len(), 1);
        let mut request = Fixture::request("[", SearchMode::Content);
        request.regex = true;
        assert_eq!(service.search(request, true).unwrap_err().code, "search_failed");
        assert!(service.search(Fixture::request("no-such-file", SearchMode::Files), true).unwrap().matches.is_empty());
        let mut request = Fixture::request("needle", SearchMode::Content);
        request.include_glob = "**/*.java".into();
        assert!(service.search(request, true).unwrap().matches.is_empty());
        let mut request = Fixture::request("needle", SearchMode::Content);
        request.exclude_glob = "**/*.txt".into();
        assert!(service.search(request, true).unwrap().matches.is_empty());
        let mut request = Fixture::request("excluded.txt", SearchMode::Files);
        request.include_glob = "**/NODE_MODULES/**".into();
        assert!(service.search(request, true).unwrap().matches.is_empty());
        fs::write(fixture.0.join("bom.txt"), "\u{feff}中文 needle\r\n").unwrap();
        let result = service.search(Fixture::request("中文 needle", SearchMode::Content), true).unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].column, Some(1));
    }

    #[test]
    #[ignore = "Requires ripgrep in PATH or DEVFLOW_RG_PATH; run explicitly for search acceptance"]
    fn bounds_results_and_deduplicates_overlapping_workspace_roots() {
        let fixture = Fixture::new();
        let nested = fixture.0.join("nested");
        fs::create_dir(&nested).unwrap();
        let lines = "needle\n".repeat(250);
        fs::write(nested.join("source.txt"), lines).unwrap();
        let service = fixture.service();
        service.execute(WorkspaceRequest {
            expected_revision: 0, discard_changes: false,
            operation: WorkspaceOperation::AddFolder { path: display_path(&nested), name: None },
        }, true).unwrap();
        let mut request = Fixture::request("source.txt", SearchMode::Files);
        request.expected_revision = 1;
        assert_eq!(service.search(request, true).unwrap().matches.len(), 1);
        let mut request = Fixture::request("needle", SearchMode::Content);
        request.expected_revision = 1;
        let result = service.search(request, true).unwrap();
        assert_eq!(result.matches.len(), 200);
        assert!(result.truncated);
        assert!(!result.warnings.is_empty());
        // A follow-up search must work after the bounded query killed and reaped rg.
        let mut request = Fixture::request("missing", SearchMode::Content);
        request.expected_revision = 1;
        assert!(service.search(request, true).unwrap().matches.is_empty());
    }

    #[test]
    #[ignore = "Requires ripgrep and permission to create symlinks; run explicitly for search acceptance"]
    fn never_searches_files_through_links_outside_the_active_root() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("private.txt"), "private needle").unwrap();
        let link = fixture.0.join("escape");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside.0, &link);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside.0, &link);
        #[cfg(any(windows, unix))]
        {
            linked.expect("Search symlink acceptance requires symlink privileges");
            let service = fixture.service();
            let files = service.search(Fixture::request("private", SearchMode::Files), true).unwrap();
            let content = service.search(Fixture::request("needle", SearchMode::Content), true).unwrap();
            #[cfg(windows)]
            fs::remove_dir(&link).unwrap();
            #[cfg(unix)]
            fs::remove_file(&link).unwrap();
            assert!(files.matches.is_empty());
            assert!(content.matches.is_empty());
            assert_eq!(fs::read_to_string(outside.0.join("private.txt")).unwrap(), "private needle");
        }
    }
}
