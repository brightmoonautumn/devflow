use super::*;

#[test]
fn large_status_only_allocates_display_limit_and_one_overflow_entry() {
    let mut bytes = b"# branch.oid (initial)\0# branch.head main\0".to_vec();
    for index in 0..20_000 {
        bytes.extend_from_slice(format!("? 中文目录/文件 {index}.txt\0").as_bytes());
    }
    let (head, branch, changes) = parse_status(&bytes).unwrap();
    assert_eq!(head, None);
    assert_eq!(branch, "main");
    assert_eq!(changes.len(), MAX_STATUS_CHANGES + 1);
    assert_eq!(changes[0].path, "中文目录/文件 0.txt");
    assert!(changes.iter().all(|file| file.untracked));
}

#[test]
#[ignore = "requires Git; uses only UUID-scoped temporary repositories"]
fn adding_folder_file_selection_moves_only_those_files_out_of_untracked_after_refresh() {
    let fixture = Fixture::new(); let root = fixture.init("添加 中文目录");
    fs::create_dir_all(root.join("新目录/子目录")).unwrap();
    fs::write(root.join("新目录/文件 一.txt"), "one\n").unwrap();
    fs::write(root.join("新目录/子目录/文件 二.txt"), "two\n").unwrap();
    fs::write(root.join("其他文件.txt"), "keep untracked\n").unwrap();
    let service = fixture.service(&root); let revision = service.snapshot().unwrap().revision;
    let read = || service.git_status(GitStatusRequest { expected_revision: revision, force: true }, true).unwrap();
    let before = read(); let repo = &before.repositories[0];
    let paths: Vec<_> = repo.changes.iter().filter(|file| file.untracked && file.path.starts_with("新目录/")).map(|file| file.path.as_str()).collect();
    assert_eq!(paths.len(), 2);
    let result = service.git_mutate(GitMutationRequest { expected_revision: revision, selections: vec![selection(repo, &paths)], operation: Mutation::Add }, true).unwrap();
    assert!(result.results[0].error.is_none());
    assert_eq!(result.results[0].paths.len(), 2);
    let after = read(); let repo = &after.repositories[0];
    assert_ne!(repo.version, before.repositories[0].version);
    for path in paths {
        let file = repo.changes.iter().find(|file| file.path == path).unwrap();
        assert!(!file.untracked);
        assert_eq!(file.kind, "added");
    }
    assert!(repo.changes.iter().any(|file| file.path == "其他文件.txt" && file.untracked));
}
#[path = "update_tests.rs"]
mod update_tests;
#[path = "overview_tests.rs"]
mod overview_tests;
use operations::{Mutation, Selection};

#[test]
fn legacy_commit_apply_defaults_to_local_only() {
    let request: GitCommitApplyRequest = serde_json::from_str(r#"{"expectedRevision":1,"planId":"preview"}"#).unwrap();
    assert!(!request.push);
}

#[test]
#[ignore = "requires Git; uses only UUID-scoped temporary repositories"]
fn repository_catalog_is_lazy_and_cached_status_never_bypasses_fresh_diff_validation() {
    let fixture = Fixture::new(); let root = fixture.init("缓存 中文");
    let service = fixture.service(&root); let revision = service.snapshot().unwrap().revision;
    let catalog = service.git_repositories(GitStatusRequest { expected_revision: revision, force: false }, true).unwrap();
    assert_eq!(catalog.repositories.len(), 1);
    assert!(catalog.repositories[0].version.is_empty());
    assert!(!catalog.refreshing);
    fs::write(root.join("a.txt"), "changed a\n").unwrap();
    let status = service.git_status(GitStatusRequest { expected_revision: revision, force: true }, true).unwrap();
    assert!(!status.refreshing);
    let repo = &status.repositories[0]; assert!(!repo.version.is_empty());
    let warm = service.git_repositories(GitStatusRequest { expected_revision: revision, force: false }, true).unwrap();
    assert_eq!(warm.repositories[0].version, repo.version);
    // A missed watcher event must never let a stale display authorize a read or write.
    fs::write(root.join("new.txt"), "external file\n").unwrap();
    assert_eq!(service.git_diff(GitDiffRequest { expected_revision: revision, repository: repo.root.clone(), version: repo.version.clone(), path: "a.txt".into() }, true).unwrap_err().code, "conflict");
    assert!(service.git_commit_preview(GitCommitRequest { expected_revision: revision, selections: vec![selection(repo, &["a.txt"])], message: "stale preview".into() }, true).is_err());
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "changed a\n");
}

#[test]
#[ignore = "requires Git; creates and removes only UUID-scoped temporary repositories"]
fn history_reads_immutable_blobs_pages_and_merge_parents_without_touching_disk() {
    let fixture = Fixture::new(); let root = fixture.init("history 中文");
    let service = fixture.service(&root); let revision = service.snapshot().unwrap().revision;
    let initial = fixture.git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    fs::write(root.join("a.txt"), "新版 中文\n").unwrap();
    fixture.git(&root, &["add", "a.txt"]); fixture.git(&root, &["commit", "-m", "更新 中文文件"]);
    let head = fixture.git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    fs::write(root.join("a.txt"), "unsaved on disk\n").unwrap();
    let repository = display_path(&root);
    let page = || GitHistoryRequest { expected_revision: revision, repository: repository.clone(), skip: 0, limit: Some(1), reference: Some("HEAD".into()), query: None, version: None };
    assert!(service.git_history(page(), false).is_err());
    let mut stale = page(); stale.expected_revision += 1; assert!(service.git_history(stale, true).is_err());
    let first = service.git_history(page(), true).unwrap();
    assert_eq!(first.commits[0].id, head); assert_eq!(first.next_skip, Some(1));
    let mut next = page(); next.skip = 1; next.version = Some(first.version.clone());
    assert_eq!(service.git_history(next, true).unwrap().commits[0].id, initial);
    let detail = service.git_history_commit(GitHistoryCommitRequest { expected_revision: revision, repository: repository.clone(), commit: head.clone(), parent: None }, true).unwrap();
    assert_eq!(detail.files[0].path, "a.txt"); assert_eq!(detail.parent.as_deref(), Some(initial.as_str()));
    let diff = service.git_history_diff(GitHistoryDiffRequest { expected_revision: revision, repository: repository.clone(), commit: head.clone(), parent: None, path: "a.txt".into() }, true).unwrap();
    assert!(diff.historical); assert_eq!(diff.diff.before, "base a\n"); assert_eq!(diff.diff.after, "新版 中文\n");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "unsaved on disk\n");
    let root_commit = service.git_history_commit(GitHistoryCommitRequest { expected_revision: revision, repository: repository.clone(), commit: initial.clone(), parent: None }, true).unwrap();
    assert!(root_commit.parent.is_none()); assert_eq!(root_commit.files.len(), 2);
    assert!(service.git_history_commit(GitHistoryCommitRequest { expected_revision: revision, repository: repository.clone(), commit: initial, parent: Some(head.clone()) }, true).is_err());
    assert!(service.git_history_diff(GitHistoryDiffRequest { expected_revision: revision, repository: repository.clone(), commit: head.clone(), parent: None, path: "../outside".into() }, true).is_err());
    fixture.git(&root, &["tag", "v1"]);
    let mut shifted = page(); shifted.skip = 1; shifted.version = Some(first.version);
    assert!(service.git_history(shifted, true).is_err());
    fixture.git(&root, &["restore", "a.txt"]);
    fixture.git(&root, &["switch", "-c", "feature"]);
    fs::write(root.join("feature.txt"), "feature\n").unwrap();
    fixture.git(&root, &["add", "feature.txt"]); fixture.git(&root, &["commit", "-m", "feature"]);
    fixture.git(&root, &["switch", "main"]);
    fs::write(root.join("main.txt"), "main\n").unwrap();
    fixture.git(&root, &["add", "main.txt"]); fixture.git(&root, &["commit", "-m", "main"]);
    fixture.git(&root, &["merge", "--no-ff", "feature", "-m", "merge"]);
    let merge = fixture.git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    let detail = service.git_history_commit(GitHistoryCommitRequest { expected_revision: revision, repository: repository.clone(), commit: merge.clone(), parent: None }, true).unwrap();
    assert_eq!(detail.commit.parents.len(), 2); assert_eq!(detail.files[0].path, "feature.txt");
    let other = service.git_history_commit(GitHistoryCommitRequest { expected_revision: revision, repository, commit: merge, parent: Some(detail.commit.parents[1].clone()) }, true).unwrap();
    assert_eq!(other.files[0].path, "main.txt");
}

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
    service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id, push: false }, true).unwrap()
}

#[test]
#[ignore = "requires Git; uses only UUID-scoped local repositories, no network"]
fn commit_and_push_preserves_local_commit_on_rejection_and_retries_without_recommit() {
    let fixture = Fixture::new();
    let root = fixture.init("推送 中文 空格");
    let bare = fixture.0.join("remote.git");
    fixture.git(&fixture.0, &["init", "--bare", bare.to_str().unwrap()]);
    fixture.git(&root, &["remote", "add", "origin", bare.to_str().unwrap()]);
    fixture.git(&root, &["push", "-u", "origin", "main"]);
    let service = fixture.service(&root); let revision = service.snapshot().unwrap().revision;
    fs::write(root.join("a.txt"), "本次提交\n").unwrap();
    let repo = repository_status(&service, &root).unwrap();
    let preview = service.git_commit_preview(GitCommitRequest { expected_revision: revision, selections: vec![selection(&repo, &["a.txt"])], message: "提交并推送".into() }, true).unwrap();
    let result = service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id, push: true }, true).unwrap();
    assert!(result.results[0].pushed);
    let head = result.results[0].commit.as_ref().unwrap();
    assert_eq!(fixture.git(&bare, &["rev-parse", "refs/heads/main"]).trim(), head);

    // A second clone advances the remote so the next push is non-fast-forward.
    let other = fixture.0.join("other");
    fixture.git(&fixture.0, &["clone", "--branch", "main", bare.to_str().unwrap(), other.to_str().unwrap()]);
    fixture.configure(&other);
    fs::write(other.join("b.txt"), "remote change\n").unwrap();
    fixture.git(&other, &["commit", "-am", "remote advance"]);
    fixture.git(&other, &["push", "origin", "main"]);
    let remote_head = fixture.git(&bare, &["rev-parse", "refs/heads/main"]);
    fs::write(root.join("a.txt"), "推送失败但保留本地\n").unwrap();
    let repo = repository_status(&service, &root).unwrap();
    let preview = service.git_commit_preview(GitCommitRequest { expected_revision: revision, selections: vec![selection(&repo, &["a.txt"])], message: "local retained".into() }, true).unwrap();
    let result = service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id, push: true }, true).unwrap();
    let item = &result.results[0];
    assert!(item.error.is_none() && item.push_error.is_some() && !item.pushed);
    let local_head = item.commit.clone().unwrap();
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]).trim(), local_head);
    assert_eq!(fixture.git(&bare, &["rev-parse", "refs/heads/main"]), remote_head);
    assert!(service.git_push(GitPushRequest { expected_revision: revision, repository: display_path(&root), expected_head: local_head.clone() }, false).is_err());
    assert!(service.git_push(GitPushRequest { expected_revision: revision + 1, repository: display_path(&root), expected_head: local_head.clone() }, true).is_err());
    let outside = fixture.init("outside workspace");
    assert!(service.git_push(GitPushRequest { expected_revision: revision, repository: display_path(&outside), expected_head: local_head.clone() }, true).is_err());
    let wrong = service.git_push(GitPushRequest { expected_revision: revision, repository: display_path(&root), expected_head: "0".repeat(40) }, true).unwrap();
    assert!(wrong.results[0].push_error.is_some());
    // Restore only this temporary bare ref to permit retrying the same local SHA.
    fixture.git(&bare, &["update-ref", "refs/heads/main", head]);
    let retry = service.git_push(GitPushRequest { expected_revision: revision, repository: display_path(&root), expected_head: local_head.clone() }, true).unwrap();
    assert!(retry.results[0].pushed);
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]).trim(), local_head);
    assert_eq!(fixture.git(&bare, &["rev-parse", "refs/heads/main"]).trim(), local_head);
}

#[test]
#[ignore = "requires Git; uses only UUID-scoped temporary repositories"]
fn push_requires_upstream_before_commit_and_local_commit_remains_available() {
    let fixture = Fixture::new(); let root = fixture.init("no upstream");
    let service = fixture.service(&root); let revision = service.snapshot().unwrap().revision;
    let old = fixture.git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("a.txt"), "selected\n").unwrap();
    let repo = repository_status(&service, &root).unwrap();
    let preview = service.git_commit_preview(GitCommitRequest { expected_revision: revision, selections: vec![selection(&repo, &["a.txt"])], message: "local only".into() }, true).unwrap();
    assert!(service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id.clone(), push: true }, true).is_err());
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]), old);
    let result = service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id, push: false }, true).unwrap();
    assert!(result.results[0].commit.is_some() && !result.results[0].pushed && result.results[0].push_error.is_none());
    fixture.git(&root, &["checkout", "--detach"]);
    assert!(push::target(&root).is_err());
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
    assert_eq!(service.git_status(GitStatusRequest { expected_revision: 0, force: true }, false).unwrap_err().code, "unauthorized");
    assert_eq!(service.git_status(GitStatusRequest { expected_revision: 999, force: true }, true).unwrap_err().code, "conflict");
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
    let status = service.git_status(GitStatusRequest { expected_revision: revision, force: true }, true).unwrap();
    assert_eq!(status.repositories.len(), 2);
    let parent_repo = status.repositories.iter().find(|repo| repo.parent.is_none()).unwrap();
    let child_repo = status.repositories.iter().find(|repo| repo.parent.is_some()).unwrap();
    assert!(!parent_repo.changes.iter().any(|file| file.submodule));
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
    assert!(service.git_commit_apply(GitCommitApplyRequest { expected_revision: revision, plan_id: preview.plan_id, push: false }, true).is_err());
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]), head);
}

#[test]
#[ignore = "requires Git; creates only UUID-scoped temporary repositories"]
fn blame_matches_editor_snapshot_and_enforces_workspace_boundaries() {
    let fixture = Fixture::new(); let root = fixture.init("blame 中文 空格");
    let service = fixture.service(&root); let revision = service.snapshot().unwrap().revision;
    let path = display_path(&root.join("a.txt"));
    let request = || GitBlameRequest { expected_revision: revision, path: path.clone(), content: Some("base a\n新草稿\n".into()) };
    assert!(service.git_blame(request(), false).is_err());
    let mut stale = request(); stale.expected_revision += 1;
    assert!(service.git_blame(stale, true).is_err());
    let result = service.git_blame(request(), true).unwrap();
    assert_eq!(result.commits.len(), 1); assert_eq!(result.commits[0].author, "DevFlow Test");
    assert!(result.ranges[0].commit.is_some()); assert!(result.ranges[1].commit.is_none());
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "base a\n");
    let detail = service.git_history_commit(GitHistoryCommitRequest { expected_revision: revision, repository: result.repository, commit: result.commits[0].id.clone(), parent: None }, true).unwrap();
    assert_eq!(detail.commit.author, "DevFlow Test");
    let outside = fixture.init("outside"); let mut escaped = request(); escaped.path = display_path(&outside.join("a.txt"));
    assert!(service.git_blame(escaped, true).is_err());
    let mut huge = request(); huge.content = Some("x".repeat(1024 * 1024 + 1));
    assert!(service.git_blame(huge, true).is_err());
    let mut many = request(); many.content = Some("x\n".repeat(10001));
    assert!(service.git_blame(many, true).is_err());
    fs::write(root.join("新 文件.txt"), "new\n").unwrap();
    let mut untracked = request(); untracked.path = display_path(&root.join("新 文件.txt"));
    assert!(service.git_blame(untracked, true).is_err());
    let mut disk = request(); disk.content = None;
    assert_eq!(service.git_blame(disk, true).unwrap().ranges.len(), 1);
    fixture.git(&root, &["config", "user.name", "Fly"]);
    let bom = "\u{feff}第一行\r\n第二行\r\n";
    fs::write(root.join("日期 中文.java"), bom).unwrap();
    fixture.git(&root, &["add", "日期 中文.java"]);
    fixture.git(&root, &["commit", "--date=2026-10-16T12:00:00+08:00", "-m", "date and BOM"]);
    let dated = service.git_blame(GitBlameRequest { expected_revision: revision, path: display_path(&root.join("日期 中文.java")), content: Some("第一行\n第二行\n".into()) }, true).unwrap();
    assert_eq!(dated.ranges.len(), 1); assert_eq!(dated.commits[0].author, "Fly");
    assert_eq!(dated.commits[0].timezone, "+0800");
    assert_eq!(fs::read_to_string(root.join("日期 中文.java")).unwrap(), bom);
    // Nearest nested repository wins; a workspace containing only a subfolder
    // cannot use blame to authorize its parent repository implicitly.
    let child = fixture.init("child repo");
    fixture.git(&root, &["-c", "protocol.file.allow=always", "submodule", "add", child.to_str().unwrap(), "nested"]);
    fixture.git(&root, &["commit", "-am", "add submodule"]);
    let nested = service.git_blame(GitBlameRequest { expected_revision: revision, path: display_path(&root.join("nested/a.txt")), content: None }, true).unwrap();
    assert_eq!(Path::new(&nested.repository).canonicalize().unwrap(), root.join("nested").canonicalize().unwrap());
    fs::create_dir(root.join("folder")).unwrap(); fs::write(root.join("folder/a.txt"), "inside\n").unwrap();
    fixture.git(&root, &["add", "folder/a.txt"]); fixture.git(&root, &["commit", "-m", "folder"]);
    let scoped = fixture.service(&root.join("folder"));
    assert!(scoped.git_blame(GitBlameRequest { expected_revision: scoped.snapshot().unwrap().revision, path: display_path(&root.join("folder/a.txt")), content: None }, true).is_err());
}
