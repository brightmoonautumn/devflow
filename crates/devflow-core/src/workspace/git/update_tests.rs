//! Opt-in real-Git scenarios, isolated from user repositories and network services.
use super::*;

fn merge_request(service: &WorkspaceService, root: &Path, version: Option<String>) -> GitMergeRequest {
    GitMergeRequest { expected_revision: service.snapshot().unwrap().revision, repository: display_path(root), version, path: None, message: None }
}
fn update_request(service: &WorkspaceService, root: &Path) -> GitUpdateRequest {
    GitUpdateRequest { expected_revision: service.snapshot().unwrap().revision, repository: display_path(root), version: None }
}
fn wait(service: &WorkspaceService, id: &str) -> GitUpdateProgress {
    let started = Instant::now();
    loop {
        let progress = service.git_update_progress(GitUpdateProgressRequest { expected_revision: service.snapshot().unwrap().revision, job_id: Some(id.into()) }, true).unwrap().unwrap();
        if progress.done { return progress; }
        assert!(started.elapsed() < Duration::from_secs(30), "update timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn clone_upstream(fixture: &Fixture) -> (PathBuf, PathBuf) {
    let origin = fixture.init("origin 中文"); let local = fixture.0.join("local 中文 空格");
    fixture.git(&fixture.0, &["clone", origin.to_str().unwrap(), local.to_str().unwrap()]);
    fixture.configure(&local);
    fixture.git(&local, &["config", "protocol.file.allow", "always"]);
    (origin, local.canonicalize().unwrap())
}
fn conflict(fixture: &Fixture, root: &Path) {
    fixture.git(root, &["checkout", "-b", "incoming"]);
    fs::write(root.join("a.txt"), "incoming 中文\n").unwrap();
    fixture.git(root, &["commit", "-am", "incoming"]);
    fixture.git(root, &["checkout", "main"]);
    fs::write(root.join("a.txt"), "local 中文\n").unwrap();
    fixture.git(root, &["commit", "-am", "local"]);
    assert_ne!(run(root, &["-c", "merge.conflictStyle=diff3", "merge", "--no-edit", "incoming"], None, true).unwrap().code, 0);
}

#[test]
#[ignore = "requires Git; UUID temporary repositories only"]
fn updates_fast_forward_and_reject_dirty_stale_or_unauthorized_requests() {
    let fixture = Fixture::new(); let (origin, root) = clone_upstream(&fixture);
    let service = fixture.service(&root);
    assert!(service.git_update_check(update_request(&service, &root), false).is_err());
    fs::write(origin.join("a.txt"), "upstream\n").unwrap(); fixture.git(&origin, &["commit", "-am", "upstream"]);
    let info = service.git_update_check(update_request(&service, &root), true).unwrap(); assert!(info.reason.is_none());
    let mut start = update_request(&service, &root); start.version = Some(info.version);
    let progress = service.git_update_start(start, true).unwrap();
    assert_eq!(wait(&service, &progress.job_id).phase, "completed");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "upstream\n");
    let info = service.git_update_check(update_request(&service, &root), true).unwrap();
    fs::write(root.join("a.txt"), "do not overwrite\n").unwrap();
    let mut stale = update_request(&service, &root); stale.version = Some(info.version);
    let job = service.git_update_start(stale, true).unwrap(); assert_eq!(wait(&service, &job.job_id).phase, "failed");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "do not overwrite\n");
    assert!(service.git_update_check(update_request(&service, &root), true).unwrap().reason.is_some());
}

#[test]
#[ignore = "requires Git; UUID temporary repositories only"]
fn update_conflicts_can_be_recovered_resolved_and_finished_after_service_restart() {
    let fixture = Fixture::new(); let (origin, root) = clone_upstream(&fixture);
    fs::write(origin.join("a.txt"), "remote 中文\n").unwrap(); fixture.git(&origin, &["commit", "-am", "remote"]);
    fs::write(root.join("a.txt"), "local 中文\n").unwrap(); fixture.git(&root, &["commit", "-am", "local"]);
    let service = fixture.service(&root);
    let info = service.git_update_check(update_request(&service, &root), true).unwrap();
    let mut start = update_request(&service, &root); start.version = Some(info.version);
    let job = service.git_update_start(start, true).unwrap(); assert_eq!(wait(&service, &job.job_id).phase, "conflicts");
    drop(service);
    let service = fixture.service(&root);
    let state = service.git_merge_status(merge_request(&service, &root, None), true).unwrap(); assert_eq!(state.files.len(), 1);
    assert!(service.git_merge_finish(merge_request(&service, &root, Some(state.version.clone())), true).is_err());
    let mut read = merge_request(&service, &root, None); read.path = Some("a.txt".into());
    let file = service.git_merge_file(read, true).unwrap(); assert_eq!(file.local.as_deref(), Some("local 中文\n")); assert_eq!(file.incoming.as_deref(), Some("remote 中文\n"));
    let make = |version: String, content: &str| GitResolveRequest { expected_revision: service.snapshot().unwrap().revision, repository: display_path(&root), version, path: "a.txt".into(), choice: "edit".into(), content: Some(content.into()) };
    assert!(service.git_merge_resolve(make(file.version.clone(), "<<<<<<< HEAD\n"), true).is_err());
    assert!(service.git_merge_resolve(make(file.version.clone(), "merged\n"), false).is_err());
    fs::write(root.join("a.txt"), "external edit\n").unwrap();
    assert!(service.git_merge_resolve(make(file.version, "merged\n"), true).is_err());
    let state = service.git_merge_status(merge_request(&service, &root, None), true).unwrap();
    let solved = service.git_merge_resolve(make(state.version, "local + remote 中文\n"), true).unwrap(); assert!(solved.files.is_empty());
    let done = service.git_merge_finish(merge_request(&service, &root, Some(solved.version)), true).unwrap(); assert!(!done.active);
    assert_eq!(fixture.git(&root, &["rev-list", "--parents", "-n", "1", "HEAD"]).split_whitespace().count(), 3);
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "local + remote 中文\n");
}

#[test]
#[ignore = "requires Git; UUID temporary repositories only"]
fn abort_preserves_extra_edits_and_refuses_stale_merge_snapshots() {
    let fixture = Fixture::new(); let root = fixture.init("abort"); conflict(&fixture, &root);
    let service = fixture.service(&root);
    let state = service.git_merge_status(merge_request(&service, &root, None), true).unwrap();
    fs::write(root.join("b.txt"), "extra edit\n").unwrap();
    assert!(service.git_merge_abort(merge_request(&service, &root, Some(state.version)), true).is_err());
    let state = service.git_merge_status(merge_request(&service, &root, None), true).unwrap();
    assert!(service.git_merge_abort(merge_request(&service, &root, Some(state.version)), true).is_err());
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "extra edit\n");
    fixture.git(&root, &["restore", "--", "b.txt"]);
    let state = service.git_merge_status(merge_request(&service, &root, None), true).unwrap();
    assert!(!service.git_merge_abort(merge_request(&service, &root, Some(state.version)), true).unwrap().active);
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "local 中文\n");
}

#[test]
#[ignore = "requires Git; UUID temporary repositories only"]
fn deleted_side_resolves_only_that_path_and_new_files_do_not_get_deleted_on_abort() {
    let fixture = Fixture::new(); let root = fixture.init("delete-conflict");
    fixture.git(&root, &["checkout", "-b", "incoming"]); fixture.git(&root, &["rm", "a.txt"]); fixture.git(&root, &["commit", "-m", "delete"]);
    fixture.git(&root, &["checkout", "main"]); fs::write(root.join("a.txt"), "local\n").unwrap(); fixture.git(&root, &["commit", "-am", "local"]);
    assert_ne!(run(&root, &["merge", "--no-edit", "incoming"], None, true).unwrap().code, 0);
    let service = fixture.service(&root); let state = service.git_merge_status(merge_request(&service, &root, None), true).unwrap();
    assert!(state.files[0].incoming_deleted);
    let solved = service.git_merge_resolve(GitResolveRequest { expected_revision: service.snapshot().unwrap().revision, repository: display_path(&root), version: state.version, path: "a.txt".into(), choice: "incoming".into(), content: None }, true).unwrap();
    assert!(!root.join("a.txt").exists()); assert!(solved.files.is_empty());
    fs::write(root.join("untracked 中文.txt"), "keep\n").unwrap();
    service.git_merge_abort(merge_request(&service, &root, Some(solved.version)), true).unwrap();
    assert_eq!(fs::read_to_string(root.join("untracked 中文.txt")).unwrap(), "keep\n");
}
