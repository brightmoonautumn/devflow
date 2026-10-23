//! Short Java launch commands; the main class and application args stay literal.
use crate::workspace::{WorkspaceError, display_path};
use std::{fs, path::Path, process::Command};

type Result<T> = std::result::Result<T, WorkspaceError>;
fn error(message: impl Into<String>) -> WorkspaceError { WorkspaceError { code: "io_error", message: message.into() } }

fn quote(value: &str) -> Result<String> {
    if value.contains('\0') { return Err(error("Java classpath 含无效字符")); }
    Ok(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t").replace('\u{000c}', "\\f")))
}

pub(super) fn apply(command: &mut Command, directory: &Path, classpath: &str) -> Result<&'static str> {
    if classpath.is_ascii() && !command.get_args().any(|arg| arg == "--disable-@files") {
        let file = directory.join("java-classpath.args");
        fs::write(&file, format!("-cp\n{}\n", quote(classpath)?))?;
        let file = file.canonicalize()?;
        command.arg(format!("@{}", display_path(&file)));
        Ok("参数文件")
    } else {
        // JDK 11/17 Windows launchers decode @files using the native codepage,
        // not necessarily UTF-8. File URLs in a manifest are ASCII percent-
        // encoded, so Chinese/space paths work without writing non-UTF8 text.
        let manifest = manifest(classpath)?;
        let file = directory.join("java-classpath.jar");
        fs::write(&file, manifest_jar(manifest.as_bytes())?)?;
        command.arg("-cp").arg(display_path(&file.canonicalize()?));
        Ok("类路径清单 JAR（参数文件兼容回退）")
    }
}

fn manifest(classpath: &str) -> Result<String> {
    let mut urls = Vec::new();
    for path in std::env::split_paths(classpath) {
        if !path.is_absolute() { return Err(error("Java classpath 必须使用绝对路径")); }
        let native = display_path(&path);
        let mut url = url::Url::from_file_path(Path::new(&native)).map_err(|_| error("无法转换 Java classpath 路径"))?;
        if path.is_dir() && !url.path().ends_with('/') { url.set_path(&format!("{}/", url.path())); }
        urls.push(url.to_string());
    }
    let header = format!("Class-Path: {}", urls.join(" "));
    if !header.is_ascii() { return Err(error("Java 类路径清单编码无效")); }
    let mut result = String::from("Manifest-Version: 1.0\r\n");
    for (index, chunk) in header.as_bytes().chunks(70).enumerate() {
        if index > 0 { result.push(' '); }
        result.push_str(std::str::from_utf8(chunk).unwrap()); result.push_str("\r\n");
    }
    result.push_str("\r\n");
    Ok(result)
}

// One uncompressed ZIP entry. No external jar executable or new crate is needed.
fn manifest_jar(content: &[u8]) -> Result<Vec<u8>> {
    fn u16le(out: &mut Vec<u8>, value: u16) { out.extend(value.to_le_bytes()); }
    fn u32le(out: &mut Vec<u8>, value: u32) { out.extend(value.to_le_bytes()); }
    let size = u32::try_from(content.len()).map_err(|_| error("Java 类路径清单过大"))?;
    let name = b"META-INF/MANIFEST.MF";
    let mut crc = !0u32;
    for byte in content { crc ^= u32::from(*byte); for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1)); } }
    let crc = !crc;
    let mut out = Vec::new();
    u32le(&mut out, 0x04034b50); u16le(&mut out, 20);
    for _ in 0..4 { u16le(&mut out, 0); }
    u32le(&mut out, crc); u32le(&mut out, size); u32le(&mut out, size);
    u16le(&mut out, name.len() as u16); u16le(&mut out, 0); out.extend(name); out.extend(content);
    let offset = out.len() as u32;
    u32le(&mut out, 0x02014b50); u16le(&mut out, 20); u16le(&mut out, 20);
    for _ in 0..4 { u16le(&mut out, 0); }
    u32le(&mut out, crc); u32le(&mut out, size); u32le(&mut out, size);
    u16le(&mut out, name.len() as u16);
    for _ in 0..4 { u16le(&mut out, 0); }
    u32le(&mut out, 0); u32le(&mut out, 0); out.extend(name);
    let central_size = out.len() as u32 - offset;
    u32le(&mut out, 0x06054b50); u16le(&mut out, 0); u16le(&mut out, 0);
    u16le(&mut out, 1); u16le(&mut out, 1); u32le(&mut out, central_size); u32le(&mut out, offset); u16le(&mut out, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argument_file_quotes_windows_paths_and_control_characters() {
        assert_eq!(quote("C:\\Program Files\\lib\\").unwrap(), "\"C:\\\\Program Files\\\\lib\\\\\"");
        assert_eq!(quote("a\"b\n\t#'x").unwrap(), "\"a\\\"b\\n\\t#'x\"");
        assert!(quote("a\0b").is_err());
    }
    #[test]
    fn long_classpath_is_not_in_os_command_and_literal_args_are_preserved() {
        let directory = std::env::temp_dir().join(format!("devflow-argfile-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let classpath = "C:/repository/dependency.jar;".repeat(2000);
        let mut command = Command::new("java");
        command.arg("-Dexample=a b");
        assert_eq!(apply(&mut command, &directory, &classpath).unwrap(), "参数文件");
        command.args(["example.Main", "a&b", "中文", "@literal", "--spring.profiles.active=dev"]);
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args[1].starts_with('@')); assert!(!args.iter().any(|arg| arg == &classpath));
        assert_eq!(&args[2..], &["example.Main", "a&b", "中文", "@literal", "--spring.profiles.active=dev"]);
        assert_eq!(fs::read_to_string(directory.join("java-classpath.args")).unwrap(), format!("-cp\n{}\n", quote(&classpath).unwrap()));
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn unicode_manifest_uses_file_urls_and_valid_zip_offsets() {
        let directory = std::env::temp_dir().join(format!("devflow-中文 空格-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let classpath = std::env::join_paths([directory.canonicalize().unwrap()]).unwrap().to_string_lossy().into_owned();
        let text = manifest(&classpath).unwrap();
        assert!(text.is_ascii()); assert!(text.contains("%20"));
        assert!(text.lines().all(|line| line.len() <= 71));
        assert!(text.replace("\r\n ", "").contains("/\r\n\r\n"));
        let zip = manifest_jar(text.as_bytes()).unwrap();
        let end = zip.len() - 22;
        assert_eq!(&zip[end..end+4], b"PK\x05\x06");
        let offset = u32::from_le_bytes(zip[end+16..end+20].try_into().unwrap()) as usize;
        assert_eq!(&zip[offset..offset+4], b"PK\x01\x02");
        assert_eq!(&zip[49..offset], text.as_bytes());
        let mut command = Command::new("java");
        assert!(apply(&mut command, &directory, &classpath).unwrap().contains("清单 JAR"));
        assert_eq!(command.get_args().next().unwrap(), "-cp");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires a full JDK 11+; compiles Java source in memory and launches a process"]
    fn real_jdk_loads_resources_through_long_ascii_and_unicode_classpaths() {
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
        let fixture = Fixture(std::env::temp_dir().join(format!("devflow-classpath-{}", uuid::Uuid::new_v4())));
        fs::create_dir_all(&fixture.0).unwrap();
        let source = fixture.0.join("Probe.java");
        fs::write(&source, r#"class Probe {
            public static void main(String[] args) throws Exception {
                try (var input = ClassLoader.getSystemResourceAsStream("probe.txt")) {
                    if (input == null || input.read() != 65 || !args[0].equals("a&b")) throw new Exception("classpath or literal argument lost");
                }
            }
        }"#).unwrap();
        for name in ["ascii space #", "中文 空格 #"] {
            let resource = fixture.0.join(name);
            fs::create_dir_all(&resource).unwrap(); fs::write(resource.join("probe.txt"), b"A").unwrap();
            let resource = resource.canonicalize().unwrap();
            let classpath = std::env::join_paths(std::iter::repeat_n(resource, 1500)).unwrap().to_string_lossy().into_owned();
            assert!(classpath.len() > 32767);
            let mut command = Command::new(super::super::java::java_executable());
            command.arg("-Dfile.encoding=UTF-8");
            apply(&mut command, &fixture.0, &classpath).unwrap();
            let output = command.arg(&source).arg("a&b").output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        }
    }
}
