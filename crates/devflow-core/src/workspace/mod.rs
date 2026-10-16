use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProject {
    pub name: String,
    pub kind: ProjectKind,
    pub path: String,
    pub markers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Maven,
    Node,
    MavenAndNode,
}

/// Only inspect paths beneath an explicitly configured local workspace root.
pub fn detect_projects(root: &Path) -> std::io::Result<Vec<DetectedProject>> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    inspect(&root, &mut result);
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path: PathBuf = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || ["node_modules", "target", "dist", "build"].contains(&name.as_ref())
        {
            continue;
        }
        inspect(&path, &mut result);
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn inspect(path: &Path, result: &mut Vec<DetectedProject>) {
    let maven = path.join("pom.xml").is_file();
    let node = path.join("package.json").is_file();
    if !maven && !node {
        return;
    }
    let kind = match (maven, node) {
        (true, true) => ProjectKind::MavenAndNode,
        (true, false) => ProjectKind::Maven,
        (false, true) => ProjectKind::Node,
        _ => unreachable!(),
    };
    let mut markers = Vec::new();
    if maven {
        markers.push("pom.xml".to_owned());
    }
    if node {
        markers.push("package.json".to_owned());
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_owned());
    result.push(DetectedProject {
        name,
        kind,
        path: path.display().to_string(),
        markers,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_kind_serialization() {
        assert_eq!(
            serde_json::to_string(&ProjectKind::MavenAndNode).unwrap(),
            "\"maven_and_node\""
        );
    }
}
