use super::*;
fn suite() -> Suite {
    serde_json::from_str(include_str!("extreme.json")).unwrap()
}
#[test]
fn references_expand_once_and_reject_cycles() {
    let mut suite = suite();
    validate(&suite).unwrap();
    suite.scenarios[2].uses.push("admin-login".into());
    let plan = execution_plan(&suite, "account-profile").unwrap();
    assert_eq!(plan.len(), 8);
    assert_eq!(
        plan.iter()
            .filter(|(_, c)| c.id == "admin-login-call")
            .count(),
        1
    );
    suite.scenarios[0].uses.push("account-profile".into());
    assert!(execution_plan(&suite, "account-profile").is_err());
}
#[test]
fn preserves_numeric_ids_and_rejects_missing_variables() {
    let vars = BTreeMap::from([
        ("id".into(), json!(2099691020244553729u64)),
        ("name".into(), json!("a/b?x=1")),
    ]);
    assert_eq!(
        render(&json!({"id":"{{id}}"}), &vars, false).unwrap()["id"],
        json!(2099691020244553729u64)
    );
    assert!(render(&json!("{{absent}}"), &vars, false).is_err());
    assert_eq!(
        render(&json!("info?name={{name}}"), &vars, true).unwrap(),
        json!("info?name=a%2Fb%3Fx%3D1")
    );
}
#[test]
fn endpoint_cannot_escape_origin_or_environment_prefix() {
    for path in [
        "http://127.0.0.1:31080/api/session",
        "//evil.example/",
        "../other",
        "%2e%2e/other",
        "a\\b",
        "/login",
        "info#secret",
    ] {
        assert!(
            endpoint("http://192.168.1.200/prod-api/system/", path).is_err(),
            "{path}"
        );
    }
    assert!(endpoint("file:///tmp/", "x").is_err());
    assert!(endpoint("http://169.254.169.254/", "x").is_err());
}
#[test]
fn redacts_credentials_raw_login_token_and_long_ids() {
    let data = json!({"data":"secret-value","initialPassword":"pw","Authorization":"token","id":2099691020244553729u64});
    let redacted = redact(&data, &["secret-value".into()]);
    assert_eq!(redacted["data"], "[REDACTED]");
    assert_eq!(redacted["initialPassword"], "[REDACTED]");
    assert_eq!(redacted["id"], "2099691020244553729");
}
#[test]
fn asserts_business_fields_even_with_http_success() {
    let check = Assertion {
        path: "/code".into(),
        op: "equals".into(),
        value: json!(200),
    };
    assert!(
        !assertion(
            &json!({"code":401,"success":false}),
            &check,
            &BTreeMap::new()
        )
        .unwrap()
    );
}
#[test]
fn storage_round_trip_scope_and_corruption() {
    let root = std::env::temp_dir().join(format!("devflow-qa-中文-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("qa.json");
    let store = Snapshot {
        workspace_key: "D:/中文 工作区".into(),
        revision: 1,
        suite: Some(suite()),
        runs: vec![],
    };
    save(&path, &store).unwrap();
    save(&path, &store).unwrap();
    assert_eq!(load(&path, &store.workspace_key).unwrap().revision, 1);
    assert!(load(&path, "other").is_err());
    let _lock = storage_lock(&path).unwrap();
    assert!(storage_lock(&path).is_err());
    drop(_lock);
    fs::write(&path, "invalid").unwrap();
    assert!(load(&path, &store.workspace_key).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "invalid");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn auth_and_revision_fail_before_network() {
    let root = std::env::temp_dir().join(format!("devflow-qa-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let e = DevFlowEngine::new(vec![root.clone()], Some(root.clone())).unwrap();
    let key = e.workspace.snapshot().unwrap().workspace_root.unwrap();
    assert_eq!(
        e.qa_get(
            Query {
                workspace_key: key.clone(),
                expected_revision: 0
            },
            false
        )
        .err()
        .unwrap()
        .code,
        "unauthorized"
    );
    assert_eq!(
        e.qa_get(
            Query {
                workspace_key: key,
                expected_revision: 99
            },
            true
        )
        .err()
        .unwrap()
        .code,
        "conflict"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn saved_suites_reject_literal_credentials() {
    let mut s = suite();
    s.variables.insert("adminPassword".into(), json!("secret"));
    assert!(validate(&s).is_err());
    let mut s = suite();
    s.scenarios[0].actions[0].calls[0].body["password"] = json!("secret");
    assert!(validate(&s).is_err());
}
#[test]
fn restart_marks_inflight_run_interrupted_without_replay() {
    let root = std::env::temp_dir().join(format!("devflow-qa-中文-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let e = DevFlowEngine::new(vec![root.clone()], Some(root.clone())).unwrap();
    e.workspace
        .configure_recent(root.join(".devflow/recent-workspace.json"), None, false)
        .unwrap();
    let ws = e.workspace.snapshot().unwrap();
    let key = ws.workspace_root.unwrap();
    let path = e.workspace.qa_location(&key, ws.revision).unwrap();
    let store = Snapshot {
        workspace_key: key.clone(),
        revision: 1,
        suite: Some(suite()),
        runs: vec![Run {
            id: "one".into(),
            scenario_id: "account-profile".into(),
            suite_revision: 1,
            status: "running".into(),
            created_at: now(),
            updated_at: now(),
            account_name: "test".into(),
            current_call: None,
            results: vec![],
            variables: BTreeMap::new(),
            error: None,
        }],
    };
    save(&path, &store).unwrap();
    let read = e
        .qa_get(
            Query {
                workspace_key: key,
                expected_revision: ws.revision,
            },
            true,
        )
        .unwrap();
    assert_eq!(read.runs[0].status, "interrupted");
    assert!(read.runs[0].results.is_empty());
    fs::remove_dir_all(root).unwrap();
}
