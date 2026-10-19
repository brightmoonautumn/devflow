use serde::Serialize;
use std::path::Path;

mod service;
pub use service::*;
mod discovery;
pub(crate) use discovery::{Discovery, discover_services};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProject {
    pub name: String,
    pub kind: ProjectKind,
    pub path: String,
    pub markers: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub main_classes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub launch_scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Maven,
    Node,
    MavenAndNode,
}

/// Discover launchable applications, not every build descriptor or dependency.
pub fn detect_projects(root: &Path) -> std::io::Result<Vec<DetectedProject>> {
    Ok(discover_services(root)?.projects)
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
