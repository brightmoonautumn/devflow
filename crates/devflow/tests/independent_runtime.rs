//! Opt-in native-process acceptance test; no Java/Node installations required.
use serde_json::{Value, json};
use std::{ffi::OsString, fs, io::Write, path::{Path, PathBuf}, process::{Command, Stdio}, thread, time::{Duration, Instant}};

fn record(directory: &Path) -> Value {
    fs::read(directory.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}
fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() { assert!(Instant::now() < deadline, "runtime acceptance timed out"); thread::sleep(Duration::from_millis(50)); }
}
fn hidden(command: &mut Command) {
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let _ = command;
}

#[test]
#[ignore = "spawns a real durable host and service process tree"]
fn worker_survives_parent_exit_keeps_logging_and_stops_descendants() {
    let root = std::env::temp_dir().join(format!("devflow-independent-中文 空格-{}", uuid::Uuid::new_v4()));
    let id = uuid::Uuid::new_v4().to_string();
    let directory = root.join(&id);
    fs::create_dir_all(&directory).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::write(self.0.join("stop"), []);
            // Give the host time to reap its children before removing files.
            thread::sleep(Duration::from_millis(500));
            if let Some(root) = self.0.parent() { let _ = fs::remove_dir_all(root); }
        }
    }
    let _cleanup = Cleanup(directory.clone());
    fs::write(directory.join("state.json"), json!({"version":1,"kind":"node","key":"fixture","id":id,"created_ms":0,"worker":null,"pid":null,"state":"starting","exit":null}).to_string()).unwrap();
    fs::write(directory.join("plan.json"), json!({"Node":{"command":{
        "program":std::env::current_exe().unwrap().into_os_string(),
        "args":[OsString::from("--ignored"),OsString::from("--exact"),OsString::from("application_fixture"),OsString::from("--nocapture")],
        "cwd":directory,"env":[[OsString::from("DEVFLOW_TEST_APPLICATION"),Some(OsString::from("1"))]]
    }}}).to_string()).unwrap();
    let mut parent = Command::new(std::env::current_exe().unwrap());
    parent.args(["--ignored", "--exact", "parent_fixture", "--nocapture"])
        .env("DEVFLOW_TEST_DIRECTORY", &directory).env("DEVFLOW_TEST_BINARY", env!("CARGO_BIN_EXE_devflow"))
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    hidden(&mut parent);
    assert!(parent.spawn().unwrap().wait().unwrap().success());
    wait_until(|| record(&directory)["state"] == "running" && directory.join("descendant.pid").exists());
    let before = directory.join("stdout.log").metadata().unwrap().len();
    // Parent is gone; both the service and log destination remain usable.
    wait_until(|| directory.join("stdout.log").metadata().is_ok_and(|meta| meta.len() > before));
    let descendant: u32 = fs::read_to_string(directory.join("descendant.pid")).unwrap().parse().unwrap();
    fs::write(directory.join("stop"), []).unwrap();
    wait_until(|| record(&directory)["state"] == "stopped");
    wait_until(|| {
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(descendant)]), true);
        system.process(sysinfo::Pid::from_u32(descendant)).is_none()
    });
    assert!(fs::read_to_string(directory.join("stdout.log")).unwrap().contains("中文服务输出"));
}

#[test]
#[ignore = "helper for native runtime acceptance"]
fn parent_fixture() {
    let Some(directory) = std::env::var_os("DEVFLOW_TEST_DIRECTORY") else { return; };
    let mut host = Command::new(std::env::var_os("DEVFLOW_TEST_BINARY").unwrap());
    host.arg("--devflow-runtime-worker").arg(&directory).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; host.creation_flags(0x00000008 | 0x00000200); }
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; host.process_group(0); }
    let _child = host.spawn().unwrap();
    wait_until(|| record(Path::new(&directory))["worker"].is_array());
}

#[test]
#[ignore = "helper for native runtime acceptance"]
fn application_fixture() {
    if std::env::var("DEVFLOW_TEST_APPLICATION").as_deref() != Ok("1") { return; }
    let mut descendant = Command::new(std::env::current_exe().unwrap());
    descendant.args(["--ignored", "--exact", "descendant_fixture", "--nocapture"]).env("DEVFLOW_TEST_DESCENDANT", "1");
    hidden(&mut descendant);
    let mut descendant = descendant.spawn().unwrap();
    fs::write("descendant.pid", descendant.id().to_string()).unwrap();
    for _ in 0..300 {
        println!("中文服务输出"); std::io::stdout().flush().unwrap();
        eprintln!("stderr remains writable");
        thread::sleep(Duration::from_millis(100));
    }
    let _ = descendant.kill(); let _ = descendant.wait();
}

#[test]
#[ignore = "helper for native runtime acceptance"]
fn descendant_fixture() {
    if std::env::var("DEVFLOW_TEST_DESCENDANT").as_deref() == Ok("1") { thread::sleep(Duration::from_secs(30)); }
}
