//! Read-only listener discovery. No port probing or user-provided commands.
use std::collections::{BTreeSet, HashMap};

pub(super) fn parse_windows_listeners(output: &str) -> HashMap<u32, BTreeSet<u16>> {
    let mut ports: HashMap<u32, BTreeSet<u16>> = HashMap::new();
    for line in output.lines() {
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        if fields.len() != 5 || fields[0] != "TCP" || fields[3] != "LISTENING" {
            continue;
        }
        let Some((_, port)) = fields[1].rsplit_once(':') else {
            continue;
        };
        if let (Ok(pid), Ok(port)) = (fields[4].parse::<u32>(), port.parse::<u16>()) {
            if pid != 0 && port != 0 {
                ports.entry(pid).or_default().insert(port);
            }
        }
    }
    ports
}

#[cfg(windows)]
pub(super) fn listeners() -> Option<HashMap<u32, BTreeSet<u16>>> {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    let program = PathBuf::from(std::env::var_os("SystemRoot")?).join("System32/netstat.exe");
    let mut child = Command::new(program)
        .arg("-ano")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let read = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= 4 * 1024 * 1024).then_some(bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = read.join().ok().flatten()?;
    if !success {
        return None;
    }
    Some(parse_windows_listeners(&String::from_utf8_lossy(&bytes)))
}

#[cfg(not(windows))]
pub(super) fn listeners() -> Option<HashMap<u32, BTreeSet<u16>>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collects_only_listening_tcp_ports_per_pid_and_deduplicates_ipv6() {
        let ports = parse_windows_listeners(
            "\
TCP 0.0.0.0:8001 0.0.0.0:0 LISTENING 123\n\
TCP [::]:8001 [::]:0 LISTENING 123\n\
TCP [::1]:5005 [::]:0 LISTENING 123\n\
TCP 127.0.0.1:8888 127.0.0.1:9000 ESTABLISHED 123\n\
UDP 0.0.0.0:5353 *:* 123\n\
TCP 0.0.0.0:9000 0.0.0.0:0 LISTENING 456\n\
TCP invalid 0.0.0.0:0 LISTENING 123\n\
TCP 0.0.0.0:70000 0.0.0.0:0 LISTENING 123",
        );
        assert_eq!(
            ports[&123].iter().copied().collect::<Vec<_>>(),
            vec![5005, 8001]
        );
        assert_eq!(ports[&456].iter().copied().collect::<Vec<_>>(), vec![9000]);
    }
}
