//! Bounded UTF-8 file editing shared by HTTP and MCP, restricted to active roots.
use super::*;

const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceImage {
    pub path: String,
    pub revision: u64,
    pub mime_type: &'static str,
    pub size: usize,
    pub data_url: String,
}

fn image_mime(path: &Path) -> Result<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "svg" => Ok("image/svg+xml"),
        "png" => Ok("image/png"),
        "jpg" | "jpeg" | "jfif" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        "bmp" => Ok("image/bmp"),
        "ico" => Ok("image/x-icon"),
        "avif" => Ok("image/avif"),
        _ => Err(error("unsupported_file", "此格式不支持图片预览")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceFileReadRequest {
    pub path: String,
    pub expected_revision: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileMutation {
    pub path: String,
    pub revision: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceFileWriteRequest {
    pub path: String,
    pub expected_revision: u64,
    /// Exact previous bytes decoded as UTF-8, including BOM and line endings.
    pub expected_content: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub path: String,
    pub content: String,
    pub revision: u64,
    pub readonly: bool,
    pub readonly_reason: Option<String>,
    pub line_ending: &'static str,
    pub bom: bool,
}

impl WorkspaceService {
    pub fn read_image(&self, request: WorkspaceFileReadRequest, authenticated: bool) -> Result<WorkspaceImage> {
        use base64::Engine;
        if !authenticated {
            return Err(error("unauthorized", "需要有效的访问令牌"));
        }
        let state = self.lock()?;
        let path = self.file_path(&state, &request.path, request.expected_revision)?;
        let mime_type = image_mime(&path)?;
        let file = fs::File::open(&path)?;
        if !file.metadata()?.is_file() {
            return Err(error("invalid_path", "只能打开已有的普通文件"));
        }
        if file.metadata()?.len() > MAX_IMAGE_BYTES {
            return Err(error("limit_exceeded", "图片最多支持 20 MiB"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(error("limit_exceeded", "图片最多支持 20 MiB"));
        }
        Ok(WorkspaceImage {
            path: display_path(&path), revision: state.revision, mime_type, size: bytes.len(),
            data_url: format!("data:{mime_type};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)),
        })
    }

    pub fn copy_file(
        &self,
        request: WorkspaceFileReadRequest,
        authenticated: bool,
    ) -> Result<WorkspaceFileMutation> {
        if !authenticated {
            return Err(error("unauthorized", "需要有效的访问令牌"));
        }
        let state = self.lock()?;
        let path = self.file_path(&state, &request.path, request.expected_revision)?;
        let mut discovery = self
            .discovery
            .lock()
            .map_err(|_| error("unavailable", "服务发现缓存暂不可用"))?;
        let directory = parent(&path)?;
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let extension = path
            .extension()
            .map(|value| format!(".{}", value.to_string_lossy()))
            .unwrap_or_default();
        for index in 1..=1000 {
            let suffix = if index == 1 {
                " 副本".to_owned()
            } else {
                format!(" 副本 ({index})")
            };
            let target = directory.join(format!("{stem}{suffix}{extension}"));
            // Authorize the existing parent; create_new refuses every existing target, including links.
            if self.authorize(directory)?.as_path() != directory {
                return Err(error("invalid_path", "文件夹路径已改变"));
            }
            let mut output = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
            {
                Ok(file) => file,
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err.into()),
            };
            let result = (|| -> Result<()> {
                let checked = self.file_path(&state, &request.path, request.expected_revision)?;
                if checked != path {
                    return Err(error("file_conflict", "源文件路径已改变"));
                }
                let mut input = fs::File::open(&path)?;
                if !input.metadata()?.is_file() {
                    return Err(error("invalid_path", "只能复制普通文件"));
                }
                std::io::copy(&mut input, &mut output)?;
                output.sync_all()?;
                Ok(())
            })();
            drop(output);
            if result.is_err() {
                let _ = fs::remove_file(&target);
            }
            result?;
            discovery.clear();
            return Ok(WorkspaceFileMutation {
                path: display_path(&target),
                revision: state.revision,
            });
        }
        Err(error(
            "already_exists",
            "副本名称过多，请先重命名或清理已有副本",
        ))
    }

    pub fn delete_file(
        &self,
        request: WorkspaceFileReadRequest,
        authenticated: bool,
    ) -> Result<WorkspaceFileMutation> {
        if !authenticated {
            return Err(error("unauthorized", "需要有效的访问令牌"));
        }
        let state = self.lock()?;
        let path = self.entry_path(&state, &request.path, request.expected_revision, true)?;
        let directory = path.is_dir();
        if directory {
            let session = state.session.as_ref()
                .ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
            for entry in folder_entries(&parse_document(&session.source)?)? {
                if let Ok(root) = resolve_folder(entry, &session.base).and_then(|p| self.authorize(&p)) {
                    if root.starts_with(&path) {
                        return Err(error("invalid_path", "不能删除工作区根文件夹或包含其他工作区根的目录，请使用从工作区移除"));
                    }
                }
            }
            let recent = self.recent_path.lock()
                .map_err(|_| error("unavailable", "最近工作区记录暂不可用"))?;
            if session.file.as_ref().is_some_and(|file| file.starts_with(&path))
                || recent.as_ref().is_some_and(|file| file.starts_with(&path))
            {
                return Err(error("permission_denied", "不能删除包含当前工作区配置或最近工作区记录的文件夹"));
            }
        }
        let snapshot = self.file_snapshot(&state, &path, String::new())?;
        if snapshot.readonly {
            return Err(error(
                "permission_denied",
                snapshot.readonly_reason.unwrap_or_default(),
            ));
        }
        let mut discovery = self
            .discovery
            .lock()
            .map_err(|_| error("unavailable", "服务发现缓存暂不可用"))?;
        // Revalidate immediately before deletion; remove_dir_all does not follow child links.
        if self.entry_path(&state, &request.path, request.expected_revision, true)? != path {
            return Err(error("file_conflict", "删除目标路径已改变"));
        }
        let result = if directory { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        // A recursive deletion can fail after removing some entries (e.g. a locked file).
        discovery.clear();
        result?;
        Ok(WorkspaceFileMutation {
            path: display_path(&path),
            revision: state.revision,
        })
    }

    fn file_path(&self, state: &State, requested: &str, revision: u64) -> Result<PathBuf> {
        self.entry_path(state, requested, revision, false)
    }

    fn entry_path(&self, state: &State, requested: &str, revision: u64, allow_directory: bool) -> Result<PathBuf> {
        if state.revision != revision {
            return Err(error("conflict", "工作区已改变，请重新打开文件"));
        }
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| error("no_workspace", "请先打开工作区"))?;
        let path = self.authorize(Path::new(requested))?;
        let document = parse_document(&session.source)?;
        let in_workspace = folder_entries(&document)?
            .iter()
            .filter_map(|entry| {
                resolve_folder(entry, &session.base)
                    .ok()
                    .and_then(|p| self.authorize(&p).ok())
            })
            .any(|root| root.is_dir() && path.starts_with(root));
        if !in_workspace {
            return Err(error("outside_workspace", "文件不在当前工作区内"));
        }
        if path.components().any(
            |c| matches!(c, Component::Normal(name) if excluded_directory(&name.to_string_lossy())),
        ) {
            return Err(error(
                "excluded_directory",
                "依赖、构建产物和版本管理元数据目录不支持编辑",
            ));
        }
        let metadata = fs::symlink_metadata(requested)?;
        let linked = metadata.file_type().is_symlink();
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            linked || metadata.file_attributes() & 0x400 != 0
        };
        if linked || !(path.is_file() || (allow_directory && path.is_dir())) {
            return Err(error("invalid_path", if allow_directory {
                "只能删除已有的普通文件或文件夹，不支持链接"
            } else {
                "只能打开已有的普通文件"
            }));
        }
        Ok(path)
    }

    fn file_snapshot(&self, state: &State, path: &Path, content: String) -> Result<WorkspaceFile> {
        let active_config = state.session.as_ref().and_then(|s| s.file.as_deref()) == Some(path);
        let recent_config = self
            .recent_path
            .lock()
            .map_err(|_| error("unavailable", "最近工作区记录暂不可用"))?
            .as_ref()
            .is_some_and(|record| comparison_path(record) == comparison_path(path));
        let readonly_reason = if active_config {
            Some("当前工作区配置请通过工作区管理修改".into())
        } else if recent_config {
            Some("最近工作区记录由 DevFlow 自动维护".into())
        } else if fs::metadata(path)?.permissions().readonly() {
            Some("文件为只读，无法保存".into())
        } else {
            None
        };
        Ok(WorkspaceFile {
            path: display_path(path),
            revision: state.revision,
            readonly: readonly_reason.is_some(),
            readonly_reason,
            line_ending: if content.contains("\r\n") {
                "CRLF"
            } else if content.contains('\r') {
                "CR"
            } else {
                "LF"
            },
            bom: content.starts_with('\u{feff}'),
            content,
        })
    }

    pub fn read_file(
        &self,
        request: WorkspaceFileReadRequest,
        authenticated: bool,
    ) -> Result<WorkspaceFile> {
        if !authenticated {
            return Err(error("unauthorized", "需要有效的访问令牌"));
        }
        let state = self.lock()?;
        let path = self.file_path(&state, &request.path, request.expected_revision)?;
        self.file_snapshot(&state, &path, read_text(&path)?)
    }

    pub fn write_file(
        &self,
        request: WorkspaceFileWriteRequest,
        authenticated: bool,
    ) -> Result<WorkspaceFile> {
        if !authenticated {
            return Err(error("unauthorized", "需要有效的访问令牌"));
        }
        if request.content.len() > MAX_FILE_BYTES || request.expected_content.len() > MAX_FILE_BYTES
        {
            return Err(error("limit_exceeded", "文本文件最多支持 1 MiB"));
        }
        let state = self.lock()?;
        let path = self.file_path(&state, &request.path, request.expected_revision)?;
        let original = read_text(&path)?;
        if original != request.expected_content {
            return Err(error(
                "file_conflict",
                "文件已被其他程序修改，未覆盖磁盘内容。请重新读取后合并修改。",
            ));
        }
        let snapshot = self.file_snapshot(&state, &path, original.clone())?;
        if snapshot.readonly {
            return Err(error(
                "permission_denied",
                snapshot.readonly_reason.unwrap_or_default(),
            ));
        }
        let normalized = request
            .content
            .trim_start_matches('\u{feff}')
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let mut content = match snapshot.line_ending {
            "CRLF" => normalized.replace('\n', "\r\n"),
            "CR" => normalized.replace('\n', "\r"),
            _ => normalized,
        };
        if snapshot.bom {
            content.insert(0, '\u{feff}');
        }
        validate_text(content.as_bytes())?;
        let permissions = fs::metadata(&path)?.permissions();
        let temp = parent(&path)?.join(format!(".devflow-edit-{}.tmp", uuid::Uuid::new_v4()));
        // Write alongside the file and replace only after sync and a second conflict check.
        self.write_new(&temp, &content)?;
        let result = (|| -> Result<()> {
            let checked = self.file_path(&state, &request.path, request.expected_revision)?;
            if checked != path || read_text(&path)? != original {
                return Err(error(
                    "file_conflict",
                    "保存期间文件被修改，请重新读取后合并",
                ));
            }
            fs::set_permissions(&temp, permissions)?;
            fs::rename(&temp, &path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result?;
        self.discovery
            .lock()
            .map_err(|_| error("unavailable", "服务发现缓存暂不可用"))?
            .clear();
        self.file_snapshot(&state, &path, content)
    }
}

fn validate_text(bytes: &[u8]) -> Result<&str> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(error("limit_exceeded", "文本文件最多支持 1 MiB"));
    }
    if bytes
        .iter()
        .any(|b| *b < 0x20 && !matches!(*b, b'\t' | b'\n' | b'\r' | 0x0c))
    {
        return Err(error(
            "unsupported_file",
            "此文件为二进制，暂不支持查看和编辑",
        ));
    }
    std::str::from_utf8(bytes)
        .map_err(|_| error("unsupported_file", "此文件不是 UTF-8 文本，暂不支持编辑"))
}

fn read_text(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(error("invalid_path", "只能打开已有的普通文件"));
    }
    if file.metadata()?.len() > MAX_FILE_BYTES as u64 {
        return Err(error("limit_exceeded", "文本文件最多支持 1 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(validate_text(&bytes)?.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_reads_preserve_bytes_and_enforce_workspace_boundaries() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>中文</text></svg>";
        fs::write(fixture.0.join("图片.SVG"), svg).unwrap();
        fs::write(fixture.0.join("photo.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        fs::write(fixture.0.join("text.txt"), "text").unwrap();
        fs::write(outside.0.join("private.svg"), svg).unwrap();
        fs::create_dir(fixture.0.join("node_modules")).unwrap();
        fs::write(fixture.0.join("node_modules/hidden.svg"), svg).unwrap();
        let service = fixture.service();
        let revision = service.lock().unwrap().revision;
        let request = |path: PathBuf| WorkspaceFileReadRequest { path: display_path(&path), expected_revision: revision };
        let image = service.read_image(request(fixture.0.join("图片.SVG")), true).unwrap();
        assert_eq!(image.mime_type, "image/svg+xml");
        assert_eq!(image.size, svg.len());
        assert_eq!(image.revision, revision);
        use base64::Engine;
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(image.data_url.split_once(',').unwrap().1).unwrap(), svg.as_bytes());
        assert_eq!(service.read_image(request(fixture.0.join("photo.png")), true).unwrap().mime_type, "image/png");
        assert_eq!(service.read_image(request(fixture.0.join("图片.SVG")), false).unwrap_err().code, "unauthorized");
        assert_eq!(service.read_image(WorkspaceFileReadRequest { expected_revision: revision + 1, ..request(fixture.0.join("图片.SVG")) }, true).unwrap_err().code, "conflict");
        assert_eq!(service.read_image(request(outside.0.join("private.svg")), true).unwrap_err().code, "outside_roots");
        assert_eq!(service.read_image(request(fixture.0.join("node_modules/hidden.svg")), true).unwrap_err().code, "excluded_directory");
        assert_eq!(service.read_image(request(fixture.0.join("text.txt")), true).unwrap_err().code, "unsupported_file");
        let large = fs::File::create(fixture.0.join("large.png")).unwrap();
        large.set_len(MAX_IMAGE_BYTES + 1).unwrap();
        drop(large);
        assert_eq!(service.read_image(request(fixture.0.join("large.png")), true).unwrap_err().code, "limit_exceeded");
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("devflow-file-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn service(&self) -> WorkspaceService {
            let service = WorkspaceService::new(vec![self.0.clone()], None).unwrap();
            service
                .execute(
                    WorkspaceRequest {
                        expected_revision: 0,
                        discard_changes: false,
                        operation: WorkspaceOperation::Open {
                            path: display_path(&self.0),
                        },
                    },
                    true,
                )
                .unwrap();
            service
        }
        fn read(&self, service: &WorkspaceService, name: &str) -> Result<WorkspaceFile> {
            let revision = service.lock()?.revision;
            service.read_file(
                WorkspaceFileReadRequest {
                    path: display_path(&self.0.join(name)),
                    expected_revision: revision,
                },
                true,
            )
        }
        fn write(
            &self,
            service: &WorkspaceService,
            file: WorkspaceFile,
            content: &str,
        ) -> Result<WorkspaceFile> {
            service.write_file(
                WorkspaceFileWriteRequest {
                    path: file.path,
                    expected_revision: file.revision,
                    expected_content: file.content,
                    content: content.into(),
                },
                true,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self
                .0
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("devflow-file-test-"))
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn copies_binary_chinese_files_without_overwriting_and_deletes_only_the_copy() {
        let fixture = Fixture::new();
        let source = fixture.0.join("中文 文件.bin");
        let bytes = [0, 0xff, 0xfe, 42];
        fs::write(&source, bytes).unwrap();
        let service = fixture.service();
        let revision = service.lock().unwrap().revision;
        let request = || WorkspaceFileReadRequest {
            path: display_path(&source),
            expected_revision: revision,
        };
        let first = service.copy_file(request(), true).unwrap();
        let second = service.copy_file(request(), true).unwrap();
        assert_eq!(
            first.path,
            display_path(&fixture.0.join("中文 文件 副本.bin"))
        );
        assert_eq!(
            second.path,
            display_path(&fixture.0.join("中文 文件 副本 (2).bin"))
        );
        assert_eq!(fs::read(&first.path).unwrap(), bytes);
        assert_eq!(fs::read(&second.path).unwrap(), bytes);
        service
            .delete_file(
                WorkspaceFileReadRequest {
                    path: first.path.clone(),
                    expected_revision: revision,
                },
                true,
            )
            .unwrap();
        assert!(!Path::new(&first.path).exists());
        assert_eq!(fs::read(source).unwrap(), bytes);
        assert_eq!(service.lock().unwrap().revision, revision);
    }

    #[test]
    fn file_mutations_require_auth_revision_membership_and_regular_files() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(fixture.0.join("source.txt"), "original").unwrap();
        fs::write(outside.0.join("private.txt"), "private").unwrap();
        fs::create_dir(fixture.0.join("node_modules")).unwrap();
        fs::write(fixture.0.join("node_modules/file.txt"), "dependency").unwrap();
        let service = WorkspaceService::new(
            vec![fixture.0.clone(), outside.0.clone()],
            Some(fixture.0.clone()),
        )
        .unwrap();
        let revision = service.lock().unwrap().revision;
        for copy in [true, false] {
            let mutate = |path: &Path, expected_revision, authenticated| {
                let request = WorkspaceFileReadRequest {
                    path: display_path(path),
                    expected_revision,
                };
                if copy {
                    service.copy_file(request, authenticated)
                } else {
                    service.delete_file(request, authenticated)
                }
            };
            assert_eq!(
                mutate(&fixture.0.join("source.txt"), revision, false)
                    .unwrap_err()
                    .code,
                "unauthorized"
            );
            assert_eq!(
                mutate(&fixture.0.join("source.txt"), revision + 1, true)
                    .unwrap_err()
                    .code,
                "conflict"
            );
            assert_eq!(
                mutate(&outside.0.join("private.txt"), revision, true)
                    .unwrap_err()
                    .code,
                "outside_workspace"
            );
            assert_eq!(
                mutate(&fixture.0, revision, true).unwrap_err().code,
                "invalid_path"
            );
            assert_eq!(
                mutate(&fixture.0.join("node_modules/file.txt"), revision, true)
                    .unwrap_err()
                    .code,
                "excluded_directory"
            );
        }
        assert_eq!(
            fs::read_to_string(fixture.0.join("source.txt")).unwrap(),
            "original"
        );
    }

    #[test]
    fn recursively_deletes_chinese_directory_including_generated_children() {
        let fixture = Fixture::new();
        let directory = fixture.0.join("中文 文件夹");
        fs::create_dir_all(directory.join("子目录/node_modules")).unwrap();
        fs::write(directory.join("子目录/node_modules/binary.bin"), [0, 255]).unwrap();
        fs::write(fixture.0.join("保留.txt"), "保留").unwrap();
        let service = fixture.service();
        let revision = service.lock().unwrap().revision;
        let request = |expected_revision| WorkspaceFileReadRequest {
            path: display_path(&directory), expected_revision,
        };
        assert_eq!(service.delete_file(request(revision), false).unwrap_err().code, "unauthorized");
        assert_eq!(service.delete_file(request(revision + 1), true).unwrap_err().code, "conflict");
        let deleted = service.delete_file(request(revision), true).unwrap();
        assert_eq!(deleted.path, display_path(&directory));
        assert_eq!(deleted.revision, revision);
        assert!(!directory.exists());
        assert!(fixture.0.join("保留.txt").is_file());
    }

    #[test]
    fn rejects_deleting_directory_containing_active_configuration() {
        let fixture = Fixture::new();
        let directory = fixture.0.join("配置");
        fs::create_dir(&directory).unwrap();
        let config = directory.join("active.code-workspace");
        fs::write(&config, r#"{"folders":[{"path":".."}]}"#).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], Some(config.clone())).unwrap();
        let revision = service.lock().unwrap().revision;
        let result = service.delete_file(WorkspaceFileReadRequest {
            path: display_path(&directory), expected_revision: revision,
        }, true);
        assert_eq!(result.unwrap_err().code, "permission_denied");
        assert!(config.is_file());
    }

    #[test]
    fn rejects_deleting_directory_containing_another_workspace_root() {
        let fixture = Fixture::new();
        let directory = fixture.0.join("父目录");
        fs::create_dir_all(directory.join("子项目")).unwrap();
        let config = fixture.0.join("active.code-workspace");
        fs::write(&config, r#"{"folders":[{"path":"."},{"path":"父目录/子项目"}]}"#).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], Some(config)).unwrap();
        let revision = service.lock().unwrap().revision;
        let result = service.delete_file(WorkspaceFileReadRequest {
            path: display_path(&directory), expected_revision: revision,
        }, true);
        assert_eq!(result.unwrap_err().code, "invalid_path");
        assert!(directory.join("子项目").is_dir());
    }

    #[test]
    fn deleting_active_workspace_configuration_is_forbidden() {
        let fixture = Fixture::new();
        let config = fixture.0.join("active.code-workspace");
        fs::write(&config, r#"{"folders":[{"path":"."}]}"#).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], Some(config.clone())).unwrap();
        let revision = service.lock().unwrap().revision;
        let result = service.delete_file(
            WorkspaceFileReadRequest {
                path: display_path(&config),
                expected_revision: revision,
            },
            true,
        );
        assert_eq!(result.unwrap_err().code, "permission_denied");
        assert!(config.is_file());
    }

    #[test]
    fn saves_chinese_paths_preserving_bom_crlf_and_workspace_revision() {
        let fixture = Fixture::new();
        let path = fixture.0.join("中文 配置.txt");
        fs::write(&path, "\u{feff}第一行\r\n第二行\r\n").unwrap();
        let service = fixture.service();
        let read = fixture.read(&service, "中文 配置.txt").unwrap();
        let revision = read.revision;
        let saved = fixture.write(&service, read, "更新\n第二行\n").unwrap();
        assert_eq!(saved.content, "\u{feff}更新\r\n第二行\r\n");
        assert_eq!(fs::read_to_string(path).unwrap(), saved.content);
        assert_eq!(saved.revision, revision);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn external_edits_and_stale_readers_never_overwrite_disk() {
        let fixture = Fixture::new();
        let path = fixture.0.join("source.txt");
        fs::write(&path, "original").unwrap();
        let service = fixture.service();
        let first = fixture.read(&service, "source.txt").unwrap();
        let second = fixture.read(&service, "source.txt").unwrap();
        fixture.write(&service, first, "first writer").unwrap();
        assert_eq!(
            fixture
                .write(&service, second, "stale writer")
                .unwrap_err()
                .code,
            "file_conflict"
        );
        let current = fixture.read(&service, "source.txt").unwrap();
        fs::write(&path, "external editor").unwrap();
        assert_eq!(
            fixture
                .write(&service, current, "overwrite")
                .unwrap_err()
                .code,
            "file_conflict"
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "external editor");
    }

    #[test]
    fn rejects_unauthenticated_stale_binary_oversize_and_outside_workspace() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(fixture.0.join("binary"), [0, 1, 2]).unwrap();
        fs::write(fixture.0.join("not-utf8"), [0xff, 0xfe]).unwrap();
        fs::write(fixture.0.join("large"), vec![b'a'; MAX_FILE_BYTES + 1]).unwrap();
        fs::write(outside.0.join("private.txt"), "outside").unwrap();
        let service = WorkspaceService::new(
            vec![fixture.0.clone(), outside.0.clone()],
            Some(fixture.0.clone()),
        )
        .unwrap();
        let request = || WorkspaceFileReadRequest {
            path: display_path(&outside.0.join("private.txt")),
            expected_revision: 0,
        };
        assert_eq!(
            service.read_file(request(), false).unwrap_err().code,
            "unauthorized"
        );
        assert_eq!(
            service
                .write_file(
                    WorkspaceFileWriteRequest {
                        path: request().path,
                        expected_revision: 0,
                        expected_content: "outside".into(),
                        content: "overwrite".into(),
                    },
                    false
                )
                .unwrap_err()
                .code,
            "unauthorized"
        );
        assert_eq!(
            service.read_file(request(), true).unwrap_err().code,
            "outside_workspace"
        );
        assert_eq!(
            service
                .read_file(
                    WorkspaceFileReadRequest {
                        expected_revision: 1,
                        ..request()
                    },
                    true
                )
                .unwrap_err()
                .code,
            "conflict"
        );
        assert_eq!(
            fixture.read(&service, "binary").unwrap_err().code,
            "unsupported_file"
        );
        assert_eq!(
            fixture.read(&service, "not-utf8").unwrap_err().code,
            "unsupported_file"
        );
        assert_eq!(
            fixture.read(&service, "large").unwrap_err().code,
            "limit_exceeded"
        );
        fs::create_dir(fixture.0.join("node_modules")).unwrap();
        fs::write(fixture.0.join("node_modules/file.txt"), "dependency").unwrap();
        assert_eq!(
            fixture
                .read(&service, "node_modules/file.txt")
                .unwrap_err()
                .code,
            "excluded_directory"
        );
    }

    #[test]
    fn rejects_link_escape_when_supported_and_protects_active_workspace_config() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("private.txt"), "outside").unwrap();
        let link = fixture.0.join("escape");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside.0, &link);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside.0, &link);
        #[cfg(any(windows, unix))]
        if linked.is_ok() {
            let service = fixture.service();
            assert_eq!(
                fixture
                    .read(&service, "escape/private.txt")
                    .unwrap_err()
                    .code,
                "outside_roots"
            );
            // Remove only the link before fixture cleanup, leaving the outside target intact.
            #[cfg(windows)]
            fs::remove_dir(&link).unwrap();
            #[cfg(unix)]
            fs::remove_file(&link).unwrap();
        }
        let config = fixture.0.join("active.code-workspace");
        fs::write(&config, r#"{"folders":[{"path":"."}]}"#).unwrap();
        let service = WorkspaceService::new(vec![fixture.0.clone()], Some(config)).unwrap();
        let read = fixture.read(&service, "active.code-workspace").unwrap();
        assert!(read.readonly);
        assert_eq!(
            fixture.write(&service, read, "{}").unwrap_err().code,
            "permission_denied"
        );
    }
}
