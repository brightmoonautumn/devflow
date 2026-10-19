//! Read-only listener discovery. No port probing or user-provided commands.
use std::{
    collections::{BTreeSet, HashMap},
    io,
};

pub(super) type Listeners = HashMap<u32, BTreeSet<u16>>;

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
pub(super) fn listeners() -> io::Result<Listeners> {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    let system_root = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::other("未找到 Windows SystemRoot，无法定位 netstat"))?;
    let program = PathBuf::from(system_root).join("System32/netstat.exe");
    let mut child = Command::new(program)
        .args(["-ano", "-p", "tcp"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|err| io::Error::new(err.kind(), format!("无法启动 netstat：{err}")))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let read = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(io::Error::other("netstat 输出超过 4 MiB 限制"));
        }
        Ok(bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("netstat 执行失败：{status}")))
                };
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match result {
                    Err(err) => io::Error::new(err.kind(), format!("无法读取 netstat 状态：{err}")),
                    _ => io::Error::new(io::ErrorKind::TimedOut, "netstat 采集超时（2 秒）"),
                });
            }
        }
    };
    let bytes = read
        .join()
        .map_err(|_| io::Error::other("netstat 输出读取线程异常"))?;
    status?;
    let bytes =
        bytes.map_err(|err| io::Error::new(err.kind(), format!("无法读取 netstat 输出：{err}")))?;
    Ok(parse_windows_listeners(&String::from_utf8_lossy(&bytes)))
}

#[cfg(not(windows))]
pub(super) fn listeners() -> io::Result<Listeners> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "当前系统尚未支持 TCP 监听端口采集",
    ))
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
