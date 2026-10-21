//! Opt-in scenarios use local UUID-owned fixtures only; no external network.
use super::*;

fn page(service: &WorkspaceService, roots: &[PathBuf], cursor: Option<String>) -> GitHistoryOverviewRequest {
    GitHistoryOverviewRequest { expected_revision: service.snapshot().unwrap().revision, repositories: roots.iter().map(|root| display_path(root)).collect(), reference: Some("all".into()), query: None, limit: Some(1), cursor }
}
fn sync(service: &WorkspaceService, root: &Path) -> GitSyncInfo {
    service.git_sync(GitSyncRequest { expected_revision: service.snapshot().unwrap().revision, repositories: vec![display_path(root)] }, true).unwrap().repositories.remove(0)
}

#[test]
#[ignore = "requires Git; UUID local repositories only"]
fn overview_keeps_identical_shas_in_separate_roots_and_pages_without_gaps() {
    let fixture = Fixture::new(); let first = fixture.init("父仓库 空格");
    fs::write(first.join("a.txt"), "second\n").unwrap(); fixture.git(&first, &["commit", "-am", "共同更新"]);
    let second = fixture.0.join("子项目 中文");
    fixture.git(&fixture.0, &["clone", first.to_str().unwrap(), second.to_str().unwrap()]);
    fixture.configure(&second);
    let service = fixture.service(&fixture.0); let roots = vec![first.clone(), second.clone()];
    let expected = fixture.git(&first, &["log", "--format=%H"]);
    let mut records: Vec<OverviewCommit> = Vec::new(); let mut cursor = None;
    for _ in 0..8 {
        let result = service.git_history_overview(page(&service, &roots, cursor), true).unwrap();
        records.extend(result.commits); cursor = result.next_cursor;
        if cursor.is_none() { break; }
    }
    assert!(cursor.is_none()); assert_eq!(records.len(), 4);
    let keys: HashSet<_> = records.iter().map(|row| (&row.repository, &row.commit.id)).collect();
    assert_eq!(keys.len(), 4);
    for root in &roots {
        let ids: Vec<_> = records.iter().filter(|row| row.repository == display_path(root)).map(|row| row.commit.id.as_str()).collect();
        assert_eq!(ids, expected.lines().collect::<Vec<_>>());
    }
    let mut search = page(&service, &roots, None); search.query = Some("共同更新".into()); search.limit = Some(50);
    assert_eq!(service.git_history_overview(search, true).unwrap().commits.len(), 2);
    assert!(service.git_history_overview(page(&service, &roots, None), false).is_err());
    let mut stale = page(&service, &roots, None); stale.expected_revision += 1;
    assert!(service.git_history_overview(stale, true).is_err());
    let initial = service.git_history_overview(page(&service, &roots, None), true).unwrap();
    let old_cursor = initial.next_cursor.unwrap();
    assert!(service.git_history_overview(page(&service, &roots[..1], Some(old_cursor.clone())), true).is_err());
    let mut changed_filter = page(&service, &roots, Some(old_cursor.clone())); changed_filter.query = Some("different".into());
    assert!(service.git_history_overview(changed_filter, true).is_err());
    fixture.git(&second, &["tag", "changed-between-pages"]);
    assert!(service.git_history_overview(page(&service, &roots, Some(old_cursor)), true).is_err());
    let scoped = fixture.service(&first);
    assert!(scoped.git_history_overview(page(&scoped, &[second], None), true).is_err());
}

#[test]
#[ignore = "requires Git; UUID local repositories only"]
fn fetch_updates_known_upstream_and_counts_without_changing_head_index_or_worktree() {
    let fixture = Fixture::new(); let origin = fixture.init("origin 中文"); let root = fixture.0.join("local 空格");
    fixture.git(&fixture.0, &["clone", origin.to_str().unwrap(), root.to_str().unwrap()]);
    fixture.configure(&root); fixture.git(&root, &["config", "protocol.file.allow", "always"]);
    let service = fixture.service(&root); let revision = service.snapshot().unwrap().revision;
    assert!(service.git_sync(GitSyncRequest { expected_revision: revision, repositories: vec![display_path(&root)] }, false).is_err());
    assert!(service.git_sync(GitSyncRequest { expected_revision: revision, repositories: vec![display_path(&origin)] }, true).is_err());
    let info = sync(&service, &root);
    assert_eq!((info.ahead, info.behind), (Some(0), Some(0))); assert!(info.can_fetch); assert!(info.last_fetched_at.is_none());
    let head = fixture.git(&root, &["rev-parse", "HEAD"]); let index = fs::read(root.join(".git/index")).unwrap();
    fs::write(root.join("a.txt"), "本地未提交，不被 Fetch 修改\n").unwrap();
    fs::write(origin.join("b.txt"), "remote change\n").unwrap(); fixture.git(&origin, &["commit", "-am", "remote advance"]);
    // Local-only status cannot see a server advance before a fetch.
    assert_eq!(sync(&service, &root).behind, Some(0));
    let fetch = || GitFetchRequest { expected_revision: revision, repository: display_path(&root), version: info.version.clone() };
    assert!(service.git_fetch(fetch(), false).is_err());
    let mut invalid_revision = fetch(); invalid_revision.expected_revision += 1;
    assert!(service.git_fetch(invalid_revision, true).is_err());
    let fetched = service.git_fetch(fetch(), true).unwrap();
    assert_eq!((fetched.ahead, fetched.behind), (Some(0), Some(1))); assert!(fetched.last_fetched_at.is_some());
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "本地未提交，不被 Fetch 修改\n");
    assert!(service.git_fetch(fetch(), true).is_err());
    fixture.git(&root, &["commit", "-am", "local advance"]);
    let diverged = sync(&service, &root);
    assert_eq!((diverged.ahead, diverged.behind), (Some(1), Some(1)));
    // A rewritten remote may update a tracking ref, never the local branch.
    fixture.git(&origin, &["update-ref", "refs/heads/main", head.trim()]);
    let local = fixture.git(&root, &["rev-parse", "HEAD"]);
    let fetched = service.git_fetch(GitFetchRequest { expected_revision: revision, repository: display_path(&root), version: diverged.version }, true).unwrap();
    assert_eq!((fetched.ahead, fetched.behind), (Some(1), Some(0)));
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]), local);
    fixture.git(&root, &["config", "branch.main.merge", "refs/heads/another"]);
    let missing = sync(&service, &root);
    assert!(missing.last_fetched_at.is_none() && missing.can_fetch && missing.behind.is_none());
    assert!(service.git_fetch(GitFetchRequest { expected_revision: revision, repository: display_path(&root), version: missing.version }, true).is_err());
    assert_eq!(fixture.git(&root, &["rev-parse", "HEAD"]), local);
    fixture.git(&root, &["checkout", "--detach"]);
    assert!(!sync(&service, &root).can_fetch);
    fixture.git(&root, &["checkout", "main"]);
    fixture.git(&root, &["config", "--unset", "branch.main.remote"]);
    fixture.git(&root, &["config", "--unset", "branch.main.merge"]);
    let unconfigured = sync(&service, &root);
    assert!(unconfigured.upstream.is_none() && !unconfigured.can_fetch);
}

#[test]
#[ignore = "requires Git; UUID local repositories only"]
fn gitlink_details_link_to_authorized_child_history_without_checkout() {
    let fixture = Fixture::new(); let parent = fixture.init("parent"); let source = fixture.init("child source");
    fixture.git(&parent, &["submodule", "add", source.to_str().unwrap(), "modules/子项目"]);
    fixture.git(&parent, &["commit", "-am", "add child"]);
    let service = fixture.service(&parent); let revision = service.snapshot().unwrap().revision;
    let head = fixture.git(&parent, &["rev-parse", "HEAD"]).trim().to_owned();
    let child = parent.join("modules/子项目"); let child_head = fixture.git(&child, &["rev-parse", "HEAD"]).trim().to_owned();
    let detail = service.git_history_commit(GitHistoryCommitRequest { expected_revision: revision, repository: display_path(&parent), commit: head, parent: None }, true).unwrap();
    let link = detail.files.iter().find(|file| file.submodule).unwrap();
    assert_eq!(link.linked_commit.as_deref(), Some(child_head.as_str()));
    assert_eq!(Path::new(link.linked_repository.as_ref().unwrap()).canonicalize().unwrap(), child.canonicalize().unwrap());
    assert_eq!(fixture.git(&child, &["rev-parse", "HEAD"]).trim(), child_head);
}
