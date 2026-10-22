//! Bounded file sinks owned by the independent host, including native build
//! messages. Child processes write to pipes, never to a renamed file handle.
use serde::{Deserialize, Serialize};
use std::{fs, io::{self, Read, Seek, SeekFrom, Write}, path::{Path, PathBuf}, sync::Mutex, thread};

pub(super) const MAX_BYTES: u64 = 10 * 1024 * 1024;
const BACKUPS: usize = 3;
static WRITER: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct Rotation { pub generation: String, pub rotating: bool }

pub(super) fn suffix(path: &Path, value: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned(); name.push(value); PathBuf::from(name)
}
pub(super) fn rotation(path: &Path) -> io::Result<Option<Rotation>> {
    match fs::File::open(suffix(path, ".rotation.json")) {
        Ok(file) => {
            if file.metadata()?.len() > 4096 { return Err(io::Error::other("日志轮转标记过大")); }
            serde_json::from_reader(file).map(Some).map_err(io::Error::other)
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}
fn marker(path: &Path, rotation: &Rotation) -> io::Result<()> {
    let temporary = suffix(path, &format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = fs::write(&temporary, serde_json::to_vec(rotation).map_err(io::Error::other)?)
        .and_then(|_| fs::rename(&temporary, suffix(path, ".rotation.json")));
    if result.is_err() { let _ = fs::remove_file(temporary); }
    result
}
fn rotate(path: &Path) -> io::Result<()> {
    let mut generation = Rotation { generation: uuid::Uuid::new_v4().to_string(), rotating: true };
    marker(path, &generation)?;
    let result = (|| {
        let oldest = suffix(path, &format!(".{BACKUPS}"));
        if oldest.exists() { fs::remove_file(oldest)?; }
        for index in (1..BACKUPS).rev() {
            let from = suffix(path, &format!(".{index}"));
            if from.exists() { fs::rename(from, suffix(path, &format!(".{}", index + 1)))?; }
        }
        if path.exists() { fs::rename(path, suffix(path, ".1"))?; }
        Ok(())
    })();
    generation.rotating = false;
    let completed = marker(path, &generation);
    result.and(completed)
}
pub(super) fn append(directory: &Path, stream: &str, bytes: &[u8]) -> io::Result<()> {
    let path = directory.join(format!("{stream}.log"));
    append_with_limit(&path, bytes, MAX_BYTES)
}
fn append_with_limit(path: &Path, mut bytes: &[u8], limit: u64) -> io::Result<()> {
    let _guard = WRITER.lock().map_err(|_| io::Error::other("日志写入锁不可用"))?;
    if rotation(path)?.is_some_and(|state| state.rotating) {
        // Recover a host interrupted between the two rotation markers.
        marker(path, &Rotation { generation: uuid::Uuid::new_v4().to_string(), rotating: false })?;
    }
    if bytes.is_empty() { fs::OpenOptions::new().append(true).create(true).open(path)?; }
    while !bytes.is_empty() {
        let length = match path.metadata() { Ok(meta) => meta.len(), Err(err) if err.kind() == io::ErrorKind::NotFound => 0, Err(err) => return Err(err) };
        if length >= limit { rotate(path)?; continue; }
        let mut count = bytes.len().min((limit - length) as usize);
        // UTF-8 continuation bytes stay with their leading byte across files.
        while count > 0 && count < bytes.len() && bytes[count] & 0xc0 == 0x80 { count -= 1; }
        if count == 0 { rotate(path)?; continue; }
        fs::OpenOptions::new().append(true).create(true).open(path)?.write_all(&bytes[..count])?;
        bytes = &bytes[count..];
    }
    let _ = fs::remove_file(suffix(path, ".error"));
    Ok(())
}

/// Drain even after a sink failure so a full/locked disk cannot deadlock the
/// application on its stdout pipe. Surface the error through a separate marker.
pub(super) fn pipe(mut input: impl Read + Send + 'static, directory: PathBuf, stream: &'static str) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let path = directory.join(format!("{stream}.log"));
        let mut buffer = [0; 16384];
        let mut pending = Vec::new();
        loop {
            let count = match input.read(&mut buffer) {
                Ok(0) => { if !pending.is_empty() { save(&directory, stream, &pending); } break; }
                Ok(count) => count,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => { let _ = fs::write(suffix(&path, ".error"), err.to_string()); break; }
            };
            pending.extend_from_slice(&buffer[..count]);
            let complete = pending.len() - incomplete_tail(&pending);
            save(&directory, stream, &pending[..complete]);
            pending.drain(..complete);
        }
    })
}
fn save(directory: &Path, stream: &str, bytes: &[u8]) {
    if bytes.is_empty() { return; }
    if let Err(err) = append(directory, stream, bytes) {
        let _ = fs::write(directory.join(format!("{stream}.log.error")), format!("{stream} 日志写入失败，部分输出未保存: {err}"));
    }
}
fn incomplete_tail(bytes: &[u8]) -> usize {
    let start = bytes.len().saturating_sub(4);
    for index in (start..bytes.len()).rev() {
        let expected = match bytes[index] { 0xc2..=0xdf => 2, 0xe0..=0xef => 3, 0xf0..=0xf4 => 4, 0x80..=0xbf => continue, _ => return 0 };
        let remaining = bytes.len() - index;
        return if remaining < expected { remaining } else { 0 };
    }
    0
}

/// Only called for verified inactive records. Bound pre-policy log files too,
/// retaining their newest bytes without importing an oversized archive.
pub(super) fn compact_legacy(directory: &Path) -> io::Result<()> {
    let _guard = WRITER.lock().map_err(|_| io::Error::other("日志写入锁不可用"))?;
    for stream in ["stdout", "stderr"] {
        let base = directory.join(format!("{stream}.log"));
        for index in 0..=BACKUPS {
            let path = if index == 0 { base.clone() } else { suffix(&base, &format!(".{index}")) };
            let length = match fs::symlink_metadata(&path) { Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => meta.len(), Ok(_) => continue, Err(err) if err.kind() == io::ErrorKind::NotFound => continue, Err(err) => return Err(err) };
            let canonical = path.canonicalize()?;
            if canonical.parent() != Some(directory.canonicalize()?.as_path()) { continue; }
            if length <= MAX_BYTES { continue; }
            let mut file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
            let modified = file.metadata()?.modified()?;
            file.seek(SeekFrom::Start(length - MAX_BYTES))?;
            let mut tail = Vec::new();
            (&mut file).take(MAX_BYTES).read_to_end(&mut tail)?;
            let start = tail.iter().take(3).take_while(|byte| **byte & 0xc0 == 0x80).count();
            let mut generation = Rotation { generation: uuid::Uuid::new_v4().to_string(), rotating: true };
            if index == 0 { marker(&base, &generation)?; }
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&tail[start..])?;
            file.set_len((tail.len() - start) as u64)?;
            // Compaction must not make a legacy run appear newly finished.
            file.set_times(fs::FileTimes::new().set_modified(modified))?;
            if index == 0 { generation.rotating = false; marker(&base, &generation)?; }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self { let path = std::env::temp_dir().join(format!("devflow-日志 空格-{}", uuid::Uuid::new_v4())); fs::create_dir_all(&path).unwrap(); Self(path) }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn oversized_writes_rotate_with_bounded_files_and_valid_chinese() {
        let f = Fixture::new(); let path = f.0.join("stdout.log");
        append_with_limit(&path, "中文日志\n".repeat(50).as_bytes(), 32).unwrap();
        for suffix_value in ["", ".1", ".2", ".3"] {
            let bytes = fs::read(suffix(&path, suffix_value)).unwrap();
            assert!(bytes.len() <= 32); assert!(std::str::from_utf8(&bytes).is_ok());
        }
        assert!(!suffix(&path, ".4").exists());
        assert!(!rotation(&path).unwrap().unwrap().rotating);
    }
    #[test]
    fn pipes_preserve_split_utf8_without_an_unbounded_line_buffer() {
        assert_eq!(incomplete_tail(&[0xe4]), 1);
        assert_eq!(incomplete_tail(&[0xe4, 0xb8]), 2);
        assert_eq!(incomplete_tail("中".as_bytes()), 0);
        let f = Fixture::new(); let input = std::io::Cursor::new("中文输出".repeat(10000).into_bytes());
        pipe(input, f.0.clone(), "stdout").join().unwrap();
        assert_eq!(fs::read_to_string(f.0.join("stdout.log")).unwrap(), "中文输出".repeat(10000));
    }
    #[test]
    fn inactive_legacy_files_retain_bounded_utf8_tail() {
        let f = Fixture::new(); let path = f.0.join("stdout.log");
        fs::write(&path, "中文".repeat(MAX_BYTES as usize / 6 + 100)).unwrap();
        compact_legacy(&f.0).unwrap();
        assert!(path.metadata().unwrap().len() <= MAX_BYTES);
        let retained = fs::read_to_string(&path).unwrap();
        assert!(retained.ends_with("中文"));
        assert!(rotation(&path).unwrap().is_some_and(|state| !state.rotating));
    }
}
