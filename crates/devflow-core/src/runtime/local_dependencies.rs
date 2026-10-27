//! Machine-local launch dependencies, never written into managed repositories.
use crate::workspace::{WorkspaceError, display_path};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

type Result<T> = std::result::Result<T, WorkspaceError>;
fn error(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError { code: "invalid_dependencies", message: message.into() }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalDependency {
    pub value: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
}
fn enabled() -> bool { true }
#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    #[serde(default)]
    project_defaults: BTreeMap<String, Vec<LocalDependency>>,
    #[serde(default)]
    services: BTreeMap<String, Vec<LocalDependency>>,
}
fn file() -> Result<PathBuf> { Ok(std::env::current_dir()?.join(".devflow/local-java-dependencies.json")) }
fn read() -> Result<Settings> {
    match fs::read_to_string(file()?) {
        Ok(value) => serde_json::from_str(&value).map_err(|e| error(format!("本地依赖配置损坏: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.into()),
    }
}
fn key(path: &Path) -> String { display_path(path).replace('\\', "/").trim_end_matches('/').to_lowercase() }
fn select(settings: &Settings, path: &str) -> Vec<LocalDependency> {
    let path = key(Path::new(path));
    if let Some(value) = settings.services.get(&path) { return value.clone(); }
    settings.project_defaults.iter().filter(|(root, _)| path == **root || path.strip_prefix(root.as_str()).is_some_and(|tail| tail.starts_with('/')))
        .max_by_key(|(root, _)| root.len()).map(|(_, value)| value.clone()).unwrap_or_default()
}
pub fn get(path: &str) -> Result<Vec<LocalDependency>> { Ok(select(&read()?, path)) }
pub fn save(path: &str, values: Vec<LocalDependency>, boundary: &Path) -> Result<()> {
    validate(&values, boundary)?;
    let mut settings = read()?;
    settings.services.insert(key(Path::new(path)), values);
    let file = file()?;
    fs::create_dir_all(file.parent().unwrap())?;
    let temporary = file.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(&settings).map_err(|e| error(e.to_string()))?)?;
    fs::rename(temporary, file)?;
    Ok(())
}
pub(crate) fn coordinate(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.len() <= 128 && part.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
}
pub(crate) fn validate(values: &[LocalDependency], boundary: &Path) -> Result<Vec<String>> {
    if values.len() > 64 { return Err(error("本地依赖最多 64 项")); }
    let boundary = boundary.canonicalize()?;
    let mut result = Vec::new();
    let mut versions = BTreeMap::new();
    for item in values {
        let value = item.value.trim();
        if value.is_empty() || value.len() > 4096 || value.contains(['\0', '\n', '\r']) { return Err(error("依赖必须是 Maven 坐标或 JAR 绝对路径")); }
        if !item.enabled { continue; }
        let value = if coordinate(value) {
            let (artifact, version) = value.rsplit_once(':').unwrap();
            if versions.insert(artifact, version).is_some_and(|old| old != version) { return Err(error(format!("本地依赖存在版本冲突: {artifact}"))); }
            value.to_owned()
        } else {
            let path = Path::new(value);
            if !path.is_absolute() || !path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("jar")) { return Err(error(format!("无效 Maven 坐标或 JAR 路径: {value}"))); }
            let path = path.canonicalize()?;
            if !path.starts_with(&boundary) || !path.is_file() { return Err(error("本地 JAR 必须是当前工作区项目目录内的文件")); }
            display_path(&path)
        };
        if !result.contains(&value) { result.push(value); }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dependency(value: &str) -> LocalDependency { LocalDependency { value: value.into(), enabled: true } }
    #[test]
    fn defaults_match_windows_directory_boundaries_and_allow_empty_override() {
        let mut settings = Settings::default();
        settings.project_defaults.insert("d:/companyproject/extreme-plus".into(), vec![dependency("dm.jdbc.driver:dm8:20250513")]);
        assert_eq!(select(&settings, "D:/CompanyProject/extreme-plus/module").len(), 1);
        assert!(select(&settings, "D:/CompanyProject/extreme-plus-other/module").is_empty());
        settings.services.insert("d:/companyproject/extreme-plus/module".into(), vec![]);
        assert!(select(&settings, "D:/CompanyProject/extreme-plus/module").is_empty());
    }
    #[test]
    fn rejects_coordinate_arguments_and_conflicting_versions() {
        assert!(coordinate("dm.jdbc.driver:dm8:20250513"));
        for value in ["-Dfoo=x", "g:a:1:jar", "g:a:1\n-Dx=y", "g:a:[1,2)"] { assert!(!coordinate(value)); }
        assert!(validate(&[dependency("g:a:1"), dependency("g:a:2")], &std::env::current_dir().unwrap()).is_err());
    }
    #[test]
    fn local_jars_preserve_unicode_and_spaces_and_reject_outside_paths() {
        let root = std::env::temp_dir().join(format!("devflow-local-dependencies-{}", uuid::Uuid::new_v4()));
        let inside = root.join("工作区");
        fs::create_dir_all(&inside).unwrap();
        let jar = inside.join("达梦 driver.jar");
        fs::write(&jar, b"jar fixture").unwrap();
        let value = display_path(&jar);
        let values = validate(&[dependency(&value), dependency(&value)], &inside).unwrap();
        assert_eq!(values, vec![display_path(&jar.canonicalize().unwrap())]);
        let outside = root.join("outside.jar");
        fs::write(&outside, b"outside").unwrap();
        assert!(validate(&[dependency(&display_path(&outside))], &inside).is_err());
        assert!(validate(&[dependency("relative.jar")], &inside).is_err());
        let mut disabled = dependency(&display_path(&outside));
        disabled.enabled = false;
        assert!(validate(&[disabled], &inside).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
