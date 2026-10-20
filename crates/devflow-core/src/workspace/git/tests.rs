use super::*;
use operations::{Mutation, Selection};

#[test]
fn porcelain_keeps_unicode_spaces_renames_and_separate_submodule_states() {
    let bytes = b"# branch.oid 1234\0# branch.head main\0\
1 .M N... 100644 100644 100644 old index folder/a b.txt\0\
2 R. N... 100644 100644 100644 old index R100 new name.txt\0old name.txt\0\
1 .M S.M. 160000 160000 160000 old old modules/dirty\0\
1 .M SC.. 160000 160000 160000 old old modules/pointer\0\
u UU N... 100644 100644 100644 100644 a b c conflict.txt\0";
    let (head, branch, changes) = parse_status(bytes).unwrap();
    assert_eq!(head.as_deref(), Some("1234"));
    assert_eq!(branch, "main");
    assert_eq!(changes[0].path, "folder/a b.txt");
    assert_eq!(changes[1].original_path.as_deref(), Some("old name.txt"));
    assert!(!changes[2].selectable);
    assert!(changes[2].submodule_dirty);
    assert!(changes[3].selectable && changes[3].pointer_changed);
    assert!(!changes[4].selectable);
    let (_, _, unicode) = parse_status("# branch.oid (initial)\0# branch.head main\0? 目录/中文 文件.txt\0".as_bytes()).unwrap();
    assert_eq!(unicode[0].path, "目录/中文 文件.txt");
    assert!(unicode[0].untracked);
    assert!(parse_status(b"2 R. N... 100644 100644 100644 a b R100 new\0").is_err());
}

#[test]
fn literal_pathspecs_cannot_become_options_or_globs() {
    let paths = vec!["--all".into(), "目录/[a]*.txt".into(), ":(glob)**".into()];
    assert_eq!(command::pathspecs(&paths), ":(literal)--all\0:(literal)目录/[a]*.txt\0:(literal):(glob)**\0".as_bytes());
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("devflow-git-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
    fn git(&self, root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new(command::executable().unwrap()).current_dir(root)
            .args(["-c", "protocol.file.allow=always", "-c", "core.autocrlf=false"])
            .args(args).env("GIT_TERMINAL_PROMPT", "0").output().unwrap();
        assert!(output.status.success(), "{:?}: {}", args, String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    }
    fn init(&self, name: &str) -> PathBuf {
        let root = self.0.join(name); fs::create_dir(&root).unwrap();
        self.git(&root, &["init", "-b", "main"]);
        self.configure(&root);
        fs::write(root.join("a.txt"), "base a\n").unwrap();
        fs::write(root.join("b.txt"), "base b\n").unwrap();
        self.git(&root, &["add", "."]); self.git(&root, &["commit", "-m", "initial"]);
        root.canonicalize().unwrap()
    }
    fn configure(&self, root: &Path) {
        self.git(root, &["config", "user.name", "DevFlow Test"]);
        self.git(root, &["config", "user.email", "devflow@example.invalid"]);
        self.git(root, &["config", "commit.gpgsign", "false"]);
        let hooks = self.0.join("empty-hooks"); fs::create_dir_all(&hooks).unwrap();
        self.git(root, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    }
    fn service(&self, root: &Path) -> WorkspaceService {
        WorkspaceService::new(vec![self.0.clone()], Some(root.to_owned())).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // This fixture owns exactly this UUID-named directory; never a user repo.
        if self.0.file_name().is_some_and(|name| name.to_string_lossy().starts_with("devflow-git-test-")) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
fn selection(repo: &GitRepository, paths: &[&str]) -> Selection {
    Selection { repository: repo.root.clone(), version: repo.version.clone(), paths: paths.iter().map(|path| (*path).into()).collect() }
}
fn commit(service: &WorkspaceService, revision: u64, repo: &GitRepository, paths: &[&str]) -> GitOutcome {
    let preview = service.git_commit_preview(GitCommitRequest { expected_revision: revision, selections: vec![selection(repo, paths)], message: "测试选择提交".into() }, true).unwrap();
    service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id }, true).unwrap()
}

#[test]
fn git_path_guards_reject_traversal_and_windows_prefixes() {
    let fixture = Fixture::new();
    let service = fixture.service(&fixture.0);
    for path in ["../outside", "/absolute", "node_modules/a", "target/a", ".git/config", "a/../../b"] {
        assert!(service.git_path(&fixture.0, path).is_err(), "{path}");
    }
    #[cfg(windows)]
    for path in ["C:\\outside", "C:relative", "\\\\server\\share\\file", "..\\outside"] {
        assert!(service.git_path(&fixture.0, path).is_err(), "{path}");
    }
    assert!(service.git_path(&fixture.0, "中文文件.txt").unwrap().starts_with(&fixture.0));
    assert_eq!(service.git_status(GitStatusRequest { expected_revision: 0 }, false).unwrap_err().code, "unauthorized");
    assert_eq!(service.git_status(GitStatusRequest { expected_revision: 999 }, true).unwrap_err().code, "conflict");
}

#[test]
#[ignore = "requires a local Git executable; creates isolated temporary repositories"]
fn selected_commit_preserves_unselected_staged_content_and_rollback_preserves_new_disk_files() {
    let fixture = Fixture::new(); let root = fixture.init("中文 项目"); let service = fixture.service(&root);
    let revision = service.snapshot().unwrap().revision;
    fs::write(root.join("a.txt"), "changed a\n").unwrap();
    fs::write(root.join("b.txt"), "staged b\n").unwrap(); fixture.git(&root, &["add", "b.txt"]);
    fs::write(root.join("b.txt"), "unstaged b\n").unwrap();
    let repo = repository_status(&service, &root).unwrap();
    let result = commit(&service, revision, &repo, &["a.txt"]);
    assert!(result.results[0].error.is_none());
    assert_eq!(fixture.git(&root, &["show", "HEAD:a.txt"]), "changed a\n");
    assert_eq!(fixture.git(&root, &["show", "HEAD:b.txt"]), "base b\n");
    assert_eq!(fixture.git(&root, &["show", ":b.txt"]), "staged b\n");
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "unstaged b\n");
    fs::write(root.join("新 文件.txt"), "keep new\n").unwrap();
    let repo = repository_status(&service, &root).unwrap();
    service.git_mutate(GitMutationRequest { expected_revision: revision, selections: vec![selection(&repo, &["新 文件.txt"])], operation: Mutation::Add }, true).unwrap();
    let repo = repository_status(&service, &root).unwrap();
    let result = service.git_mutate(GitMutationRequest { expected_revision: revision, selections: vec![selection(&repo, &["新 文件.txt", "b.txt"])], operation: Mutation::Rollback }, true).unwrap();
    assert!(result.results[0].error.is_none());
    assert_eq!(fs::read_to_string(root.join("新 文件.txt")).unwrap(), "keep new\n");
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "base b\n");
    assert!(fixture.git(&root, &["ls-files", "--", "新 文件.txt"]).is_empty());
    let repo = repository_status(&service, &root).unwrap();
    let untracked = selection(&repo, &["新 文件.txt"]);
    assert!(service.git_mutate(GitMutationRequest { expected_revision: revision, selections: vec![untracked], operation: Mutation::Rollback }, true).is_err());
}

#[test]
#[ignore = "requires Git with local file submodule transport; operates on temporary repositories only"]
fn initialized_submodules_have_separate_selections_and_parent_pointer_commits() {
    let fixture = Fixture::new(); let parent = fixture.init("parent"); let origin = fixture.init("child-origin");
    fixture.git(&parent, &["submodule", "add", origin.to_str().unwrap(), "modules/子仓库"]);
    fixture.git(&parent, &["commit", "-m", "add submodule"]);
    let child = parent.join("modules/子仓库").canonicalize().unwrap(); fixture.configure(&child);
    let service = fixture.service(&parent); let revision = service.snapshot().unwrap().revision;
    fs::write(child.join("a.txt"), "child change\n").unwrap();
    let status = service.git_status(GitStatusRequest { expected_revision: revision }, true).unwrap();
    assert_eq!(status.repositories.len(), 2);
    let parent_repo = status.repositories.iter().find(|repo| repo.parent.is_none()).unwrap();
    let child_repo = status.repositories.iter().find(|repo| repo.parent.is_some()).unwrap();
    assert!(parent_repo.changes.iter().any(|file| file.submodule && !file.selectable));
    assert_eq!(child_repo.submodule_path.as_deref(), Some("modules/子仓库"));
    let parent_before = fixture.git(&parent, &["rev-parse", "HEAD"]);
    assert!(commit(&service, revision, child_repo, &["a.txt"]).results[0].error.is_none());
    assert_eq!(fixture.git(&parent, &["rev-parse", "HEAD"]), parent_before);
    let parent_repo = repository_status(&service, &parent).unwrap();
    assert!(parent_repo.changes.iter().any(|file| file.pointer_changed && file.selectable));
    fs::write(child.join("b.txt"), "second child change\n").unwrap();
    let child_repo = repository_status(&service, &child).unwrap();
    assert!(service.git_commit_preview(GitCommitRequest { expected_revision: revision, message: "invalid combined commit".into(), selections: vec![selection(&parent_repo, &["modules/子仓库"]), selection(&child_repo, &["b.txt"])] }, true).is_err());
    let parent_repo = repository_status(&service, &parent).unwrap();
    assert!(commit(&service, revision, &parent_repo, &["modules/子仓库"]).results[0].error.is_none());
    let pointer = fixture.git(&parent, &["ls-tree", "HEAD", "modules/子仓库"]);
    assert!(pointer.contains(fixture.git(&child, &["rev-parse", "HEAD"]).trim()));
    assert_eq!(fs::read_to_string(child.join("b.txt")).unwrap(), "second child change\n");
}

#[test]
#[ignore = "requires a local Git executable; validates immutable commit preview in a temporary repo"]
fn edits_after_preview_invalidate_commit_even_when_porcelain_code_stays_the_same() {
    let fixture = Fixture::new(); let root = fixture.init("repo"); let service = fixture.service(&root);
    let revision = service.snapshot().unwrap().revision;
    fs::write(root.join("a.txt"), "draft one\n").unwrap();
    let repo = repository_status(&service, &root).unwrap();
    let preview = service.git_commit_preview(GitCommitRequest { expected_revision: revision, selections: vec![selection(&repo, &["a.txt"])], message: "preview".into() }, true).unwrap();
    let head = fixture.git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("a.txt"), "draft two\n").unwrap();
    assert!(service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id }, true).is_err());
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]), head);
}
