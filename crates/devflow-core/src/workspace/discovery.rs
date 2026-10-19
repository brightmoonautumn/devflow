//! Runnable-app discovery follows the main-class distinction used by VS Code's
//! Spring Boot Dashboard. XML and Java are parsed by roxmltree and Tree-sitter;
//! no Maven invocation, dependency resolution, archive scanning, or JDTLS is run.
use super::{DetectedProject, ProjectKind, display_path};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};
use tree_sitter::{Node, Parser};

#[derive(Clone, Default)]
pub(crate) struct Discovery {
    pub projects: Vec<DetectedProject>,
    pub warnings: Vec<String>,
}
struct Scanner {
    root: PathBuf,
    result: Discovery,
    seen: HashSet<PathBuf>,
    files: usize,
    bytes: usize,
    entries: usize,
    parser: Parser,
}
fn excluded(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::Normal(n) if ["node_modules", "target", ".git", "build", "dist"].iter().any(|v| n.eq_ignore_ascii_case(v))))
}
impl Scanner {
    fn bounded_path(&self, path: &Path) -> Option<PathBuf> {
        let candidate = super::service::comparison_path(path);
        let root = super::service::comparison_path(&self.root);
        if candidate != root && !candidate.starts_with(&format!("{root}/")) {
            return None;
        }
        let canonical = path.canonicalize().ok()?;
        (canonical.starts_with(&self.root) && !excluded(&canonical)).then_some(canonical)
    }
    fn warn(&mut self, path: &Path, message: impl std::fmt::Display) {
        if self.result.warnings.len() < 32 {
            self.result
                .warnings
                .push(format!("{}: {message}", display_path(path)));
        }
    }
    fn read(&mut self, path: &Path, max_bytes: u64) -> io::Result<String> {
        let path = self.bounded_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "路径超出工程边界或属于忽略目录",
            )
        })?;
        if self.files >= 20_000 || self.bytes >= 64 * 1024 * 1024 {
            return Err(io::Error::other("服务发现达到读取预算，请直接打开目标模块"));
        }
        let file = fs::File::open(path)?;
        if file.metadata()?.len() > max_bytes {
            return Err(io::Error::other("文件超出服务发现读取限制"));
        }
        let mut text = String::new();
        file.take(max_bytes + 1).read_to_string(&mut text)?;
        if text.len() as u64 > max_bytes {
            return Err(io::Error::other("文件超出服务发现读取限制"));
        }
        self.files += 1;
        self.bytes += text.len();
        Ok(text)
    }
    fn candidate(&mut self, path: &Path, inherited: &HashMap<String, String>) {
        let Some(path) = self.bounded_path(path) else {
            return;
        };
        if !path.is_dir() || self.seen.contains(&path) {
            return;
        }
        if path
            .strip_prefix(&self.root)
            .is_ok_and(|p| p.components().count() > 32)
        {
            self.warn(&path, "模块嵌套超过 32 层，请直接打开目标模块");
            return;
        }
        if !path.join("pom.xml").is_file() && !path.join("package.json").is_file() {
            return;
        }
        if self.seen.len() >= 512 {
            self.warn(&path, "模块数量超过 512，请缩小工作区范围");
            return;
        }
        self.seen.insert(path.clone());
        let mut main_classes = Vec::new();
        let mut module_paths = Vec::new();
        let mut properties = inherited.clone();
        if path.join("pom.xml").is_file() {
            match self.maven(&path, &mut properties) {
                Ok((classes, modules)) => {
                    main_classes = classes;
                    module_paths = modules;
                }
                Err(err) => self.warn(&path.join("pom.xml"), err),
            }
        }
        let scripts = if path.join("package.json").is_file() {
            match self.node_scripts(&path) {
                Ok(scripts) => scripts,
                Err(err) => {
                    self.warn(&path.join("package.json"), err);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        if !main_classes.is_empty() || !scripts.is_empty() {
            let kind = match (main_classes.is_empty(), scripts.is_empty()) {
                (false, false) => ProjectKind::MavenAndNode,
                (false, true) => ProjectKind::Maven,
                _ => ProjectKind::Node,
            };
            let mut markers = Vec::new();
            if !main_classes.is_empty() {
                markers.push("pom.xml".into());
            }
            if !scripts.is_empty() {
                markers.push("package.json".into());
            }
            let project = DetectedProject {
                name: path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                kind,
                path: display_path(&path),
                markers,
                main_classes,
                launch_scripts: scripts,
                node_script: None,
                node_selector: None,
            };
            let aliases: Vec<_> = project
                .launch_scripts
                .iter()
                .filter(|s| !["dev", "start", "serve"].contains(&s.as_str()))
                .cloned()
                .collect();
            if aliases.is_empty() && project.main_classes.is_empty()
                || project.launch_scripts.is_empty()
            {
                self.result.projects.push(project);
            } else {
                let aliases = if aliases.is_empty() {
                    project.launch_scripts.clone()
                } else {
                    aliases
                };
                if !project.main_classes.is_empty() {
                    let mut java = project.clone();
                    java.kind = ProjectKind::Maven;
                    java.launch_scripts.clear();
                    java.markers.retain(|m| m != "package.json");
                    self.result.projects.push(java);
                }
                let manifest = self
                    .read(&path.join("package.json"), 2 * 1024 * 1024)
                    .ok()
                    .and_then(|text| {
                        serde_json::from_str::<serde_json::Value>(
                            text.trim_start_matches('\u{feff}'),
                        )
                        .ok()
                    });
                for script in aliases {
                    let mut service = project.clone();
                    service.name = format!("{} / {script}", project.name);
                    service.kind = ProjectKind::Node;
                    service.main_classes.clear();
                    service.markers = vec!["package.json".into()];
                    service.launch_scripts = vec![script.clone()];
                    service.node_selector = manifest
                        .as_ref()
                        .and_then(|value| value.get("scripts")?.get(&script)?.as_str())
                        .and_then(|command| {
                            command.split_whitespace().find_map(|arg| {
                                arg.strip_prefix("--project=")
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_owned)
                            })
                        });
                    service.node_script = Some(script);
                    self.result.projects.push(service);
                }
            }
        }
        for module in module_paths {
            self.candidate(&module, &properties);
        }
    }
    fn maven(
        &mut self,
        path: &Path,
        properties: &mut HashMap<String, String>,
    ) -> io::Result<(Vec<String>, Vec<PathBuf>)> {
        let text = self.read(&path.join("pom.xml"), 2 * 1024 * 1024)?;
        let doc = roxmltree::Document::parse_with_options(
            text.trim_start_matches('\u{feff}'),
            roxmltree::ParsingOptions {
                nodes_limit: 100_000,
                ..Default::default()
            },
        )
        .map_err(io::Error::other)?;
        let project = doc.root_element();
        if !project.has_tag_name("project") {
            return Err(io::Error::other("POM 根节点必须是 project"));
        }
        if let Some(props) = child(project, "properties") {
            for property in props.children().filter(|n| n.is_element()) {
                properties.insert(
                    property.tag_name().name().into(),
                    property.text().unwrap_or("").trim().into(),
                );
            }
        }
        properties.insert("project.basedir".into(), display_path(path));
        properties.insert("basedir".into(), display_path(path));
        let mut modules = Vec::new();
        let mut module_groups: Vec<_> = child(project, "modules").into_iter().collect();
        if let Some(profiles) = child(project, "profiles") {
            for profile in profiles.children().filter(|n| n.has_tag_name("profile")) {
                if child(profile, "activation")
                    .and_then(|a| child(a, "activeByDefault"))
                    .and_then(|n| n.text())
                    == Some("true")
                {
                    module_groups.extend(child(profile, "modules"));
                }
            }
        }
        for group in module_groups {
            for module in group.children().filter(|n| n.has_tag_name("module")) {
                let name = expand(module.text().unwrap_or("").trim(), properties);
                if name.is_empty() || name.contains("${") {
                    self.warn(path, "模块路径含未解析的 Maven 属性，未自动扫描");
                    continue;
                }
                let module = path.join(name);
                if self.bounded_path(&module).is_some() {
                    modules.push(module);
                } else {
                    self.warn(&module, "声明的模块不存在或超出导入目录边界");
                }
            }
        }
        if child(project, "packaging")
            .and_then(|n| n.text())
            .is_some_and(|p| p.trim() == "pom")
        {
            return Ok((Vec::new(), modules));
        }
        let mut configured = HashSet::new();
        if let Some(main) = properties.get("start-class") {
            configured.insert(expand(main, properties));
        }
        if let Some(plugins) = child(project, "build").and_then(|b| child(b, "plugins")) {
            for plugin in plugins.children().filter(|n| n.has_tag_name("plugin")) {
                if ![
                    "spring-boot-maven-plugin",
                    "maven-jar-plugin",
                    "maven-shade-plugin",
                    "exec-maven-plugin",
                ]
                .contains(
                    &child(plugin, "artifactId")
                        .and_then(|n| n.text())
                        .unwrap_or(""),
                ) {
                    continue;
                }
                for main in plugin.descendants().filter(|n| n.has_tag_name("mainClass")) {
                    if let Some(main) = main.text() {
                        configured.insert(expand(main.trim(), properties));
                    }
                }
            }
        }
        let source = child(project, "build")
            .and_then(|b| child(b, "sourceDirectory"))
            .and_then(|n| n.text())
            .map(|s| expand(s.trim(), properties))
            .unwrap_or_else(|| "src/main/java".into());
        let source = path.join(source);
        let classes = if let Some(source) = self
            .bounded_path(&source)
            .filter(|s| s.starts_with(path) && s.is_dir())
        {
            self.java_sources(&source, &configured)
        } else {
            Vec::new()
        };
        Ok((classes, modules))
    }
    fn java_sources(&mut self, source: &Path, configured: &HashSet<String>) -> Vec<String> {
        let mut pending = vec![source.to_owned()];
        let mut classes = Vec::new();
        while let Some(directory) = pending.pop() {
            if self.entries >= 50_000 || self.files >= 20_000 || self.bytes >= 64 * 1024 * 1024 {
                self.warn(source, "源码发现达到预算，请直接打开目标模块");
                break;
            }
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(err) => {
                    self.warn(&directory, err);
                    continue;
                }
            };
            for entry in entries {
                self.entries += 1;
                if self.entries > 50_000 {
                    break;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        self.warn(&directory, err);
                        continue;
                    }
                };
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(_) => continue,
                };
                if kind.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if kind.is_dir() && !excluded(&path) {
                    pending.push(path);
                } else if kind.is_file() && path.extension().is_some_and(|e| e == "java") {
                    match self.read(&path, 512 * 1024) {
                        Ok(text) => {
                            // Cheap prefilter only. Syntax-tree checks below make the decision.
                            if text.contains("main")
                                && (!configured.is_empty()
                                    || text.contains("SpringBootApplication")
                                    || text.contains("SpringApplication")
                                    || text.contains("SpringBootConfiguration"))
                            {
                                classes.extend(java_entry_points(
                                    &mut self.parser,
                                    &text,
                                    configured,
                                ));
                            }
                        }
                        Err(err) => self.warn(&path, err),
                    }
                }
            }
        }
        classes.sort();
        classes.dedup();
        classes
    }
    fn node_scripts(&mut self, path: &Path) -> io::Result<Vec<String>> {
        let text = self.read(&path.join("package.json"), 2 * 1024 * 1024)?;
        let value: serde_json::Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(io::Error::other)?;
        Ok(node_launch_scripts(&value))
    }
}

pub(crate) fn node_launch_scripts(value: &serde_json::Value) -> Vec<String> {
    fn launch(
        name: &str,
        scripts: &serde_json::Map<String, serde_json::Value>,
        visited: &mut HashSet<String>,
    ) -> bool {
        if name.is_empty()
            || name.len() > 128
            || name.starts_with('-')
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:@".contains(c))
            || visited.len() >= 16
            || !visited.insert(name.into())
        {
            return false;
        }
        let Some(command) = scripts
            .get(name)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            return false;
        };
        if ["dev", "start", "serve"].contains(&name)
            || ["dev:", "start:", "serve:"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
        {
            return true;
        }
        if [
            "build", "test", "lint", "check", "scan", "report", "format", "install", "prepare",
            "pre", "post",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        {
            return false;
        }
        let parts: Vec<_> = command.split_whitespace().collect();
        let target = match parts.as_slice() {
            ["npm", "run" | "run-script", target, ..] => Some(*target),
            ["pnpm" | "yarn", "run", target, ..] => Some(*target),
            ["pnpm" | "yarn", target, ..] if !target.starts_with('-') => Some(*target),
            _ => None,
        };
        target.is_some_and(|target| launch(target, scripts, visited))
    }
    let Some(scripts) = value.get("scripts").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut result: Vec<_> = scripts
        .keys()
        .filter(|name| launch(name, scripts, &mut HashSet::new()))
        .cloned()
        .collect();
    result.sort_by_key(|name| match name.as_str() {
        "dev" => (0, name.clone()),
        "start" => (1, name.clone()),
        "serve" => (2, name.clone()),
        _ => (3, name.clone()),
    });
    result.truncate(128);
    result
}
pub(crate) fn discover_services(root: &Path) -> io::Result<Discovery> {
    let root = root.canonicalize()?;
    if !root.is_dir() || excluded(&root) {
        return Ok(Discovery::default());
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .map_err(io::Error::other)?;
    let mut scanner = Scanner {
        root: root.clone(),
        result: Discovery::default(),
        seen: HashSet::new(),
        files: 0,
        bytes: 0,
        entries: 0,
        parser,
    };
    scanner.candidate(&root, &HashMap::new());
    // Support a container folder; deeper traversal follows only declared Maven modules.
    for (index, entry) in fs::read_dir(&root)?.enumerate() {
        if root.join("pom.xml").is_file() {
            break;
        }
        if index >= 2000 {
            scanner.warn(&root, "直属目录过多，发现范围已截断");
            break;
        }
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && !entry.file_name().to_string_lossy().starts_with('.')
            && !excluded(&entry.path())
        {
            scanner.candidate(&entry.path(), &HashMap::new());
        }
    }
    scanner.result.projects.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(scanner.result)
}
fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|n| n.has_tag_name(name))
}
fn expand(value: &str, properties: &HashMap<String, String>) -> String {
    let mut value = value.to_owned();
    for _ in 0..8 {
        let previous = value.clone();
        for (key, replacement) in properties {
            let token = format!("${{{key}}}");
            let count = value.matches(&token).count();
            if value
                .len()
                .saturating_add(count.saturating_mul(replacement.len()))
                > 32_768
            {
                return "${devflow.unresolved.propertyExpansionLimit}".into();
            }
            value = value.replace(&token, replacement);
        }
        if value == previous {
            break;
        }
    }
    value
}
fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}
fn is_main(method: Node<'_>, source: &str) -> bool {
    if method.kind() != "method_declaration"
        || method
            .child_by_field_name("name")
            .is_none_or(|n| text(n, source) != "main")
        || method
            .child_by_field_name("type")
            .is_none_or(|n| text(n, source) != "void")
    {
        return false;
    }
    let mut cursor = method.walk();
    let Some(modifiers) = method
        .named_children(&mut cursor)
        .find(|n| n.kind() == "modifiers")
    else {
        return false;
    };
    let mut cursor = modifiers.walk();
    let tokens: Vec<_> = modifiers.children(&mut cursor).map(|n| n.kind()).collect();
    if !tokens.contains(&"public") || !tokens.contains(&"static") {
        return false;
    }
    let Some(parameters) = method.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = parameters.walk();
    let parameters: Vec<_> = parameters
        .named_children(&mut cursor)
        .filter(|n| !["line_comment", "block_comment"].contains(&n.kind()))
        .collect();
    if parameters.len() != 1 {
        return false;
    }
    let parameter = parameters[0];
    if parameter.kind() == "formal_parameter" {
        let Some(ty) = parameter.child_by_field_name("type") else {
            return false;
        };
        let mut ty = text(ty, source).split_whitespace().collect::<String>();
        if let Some(dimensions) = parameter.child_by_field_name("dimensions") {
            ty.push_str(text(dimensions, source));
        }
        ty == "String[]" || ty == "java.lang.String[]"
    } else if parameter.kind() == "spread_parameter" {
        let mut cursor = parameter.walk();
        parameter.named_children(&mut cursor).any(|n| {
            ["type_identifier", "scoped_type_identifier"].contains(&n.kind())
                && ["String", "java.lang.String"].contains(&text(n, source))
        })
    } else {
        false
    }
}
fn boot_call(method: Node<'_>, source: &str) -> bool {
    let mut pending = vec![method];
    while let Some(node) = pending.pop() {
        if node.kind() == "method_invocation"
            && node
                .child_by_field_name("name")
                .is_some_and(|n| text(n, source) == "run")
            && node.child_by_field_name("object").is_some_and(|n| {
                text(n, source) == "SpringApplication"
                    || text(n, source) == "org.springframework.boot.SpringApplication"
            })
        {
            return true;
        }
        if node.kind() == "object_creation_expression"
            && node.child_by_field_name("type").is_some_and(|n| {
                [
                    "SpringApplication",
                    "SpringApplicationBuilder",
                    "org.springframework.boot.builder.SpringApplicationBuilder",
                ]
                .contains(&text(n, source))
            })
        {
            return true;
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    false
}
fn java_entry_points(
    parser: &mut Parser,
    source: &str,
    configured: &HashSet<String>,
) -> Vec<String> {
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    if root.has_error() {
        return Vec::new();
    }
    let mut cursor = root.walk();
    let package = root
        .named_children(&mut cursor)
        .find(|n| n.kind() == "package_declaration")
        .and_then(|n| {
            let mut cursor = n.walk();
            n.named_children(&mut cursor)
                .find(|n| ["identifier", "scoped_identifier"].contains(&n.kind()))
                .map(|n| text(n, source))
        })
        .unwrap_or("");
    let mut pending = vec![root];
    let mut result = Vec::new();
    while let Some(class) = pending.pop() {
        let mut cursor = class.walk();
        pending.extend(class.named_children(&mut cursor));
        if !["class_declaration", "record_declaration"].contains(&class.kind()) {
            continue;
        }
        let Some(name) = class.child_by_field_name("name") else {
            continue;
        };
        let mut class_names = vec![text(name, source)];
        let mut parent = class.parent();
        while let Some(ancestor) = parent {
            if ["class_declaration", "record_declaration"].contains(&ancestor.kind()) {
                if let Some(name) = ancestor.child_by_field_name("name") {
                    class_names.push(text(name, source));
                }
            }
            parent = ancestor.parent();
        }
        class_names.reverse();
        let main_class = format!(
            "{}{}{}",
            package,
            if package.is_empty() { "" } else { "." },
            class_names.join("$")
        );
        let mut cursor = class.walk();
        let annotation = class
            .named_children(&mut cursor)
            .find(|n| n.kind() == "modifiers")
            .is_some_and(|modifiers| {
                let mut cursor = modifiers.walk();
                modifiers
                    .named_children(&mut cursor)
                    .filter(|n| ["marker_annotation", "annotation"].contains(&n.kind()))
                    .any(|n| {
                        n.child_by_field_name("name").is_some_and(|name| {
                            ["SpringBootApplication", "SpringBootConfiguration"]
                                .contains(&text(name, source).rsplit('.').next().unwrap_or(""))
                        })
                    })
            });
        if let Some(body) = class.child_by_field_name("body") {
            let mut cursor = body.walk();
            if body.named_children(&mut cursor).any(|method| {
                is_main(method, source)
                    && (annotation || configured.contains(&main_class) || boot_call(method, source))
            }) {
                result.push(main_class);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("devflow-services-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self(root.canonicalize().unwrap())
        }
        fn write(&self, relative: &str, text: &str) {
            let target = self.0.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, text).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self
                .0
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("devflow-services-test-"))
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
    const BOOT: &str = "package example; @org.springframework.boot.autoconfigure.SpringBootApplication public class Application { public static void main(String[] args) { org.springframework.boot.SpringApplication.run(Application.class,args); } }";
    #[test]
    fn follows_declared_maven_modules_and_excludes_libraries_aggregators_and_tests() {
        let fixture = Fixture::new();
        fixture.write("pom.xml", r#"<project xmlns="http://maven.apache.org/POM/4.0.0"><packaging>pom</packaging><properties><app.folder>app</app.folder></properties><modules><module>modules</module><module>library</module><module>test-only</module><module>config-only</module><module>fake</module></modules><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>"#);
        fixture.write("modules/pom.xml", "<project><packaging>pom</packaging><modules><module>${app.folder}</module></modules></project>");
        fixture.write("modules/app/pom.xml", "<project><dependencies><dependency><artifactId>spring-boot-starter-web</artifactId></dependency></dependencies></project>");
        fixture.write("modules/app/src/main/java/example/Application.java", BOOT);
        fixture.write("library/pom.xml", "<project><dependencies><dependency><artifactId>spring-boot-starter</artifactId></dependency></dependencies></project>");
        fixture.write(
            "library/src/main/java/Util.java",
            "public class Util { public static void main(String[] args) {} }",
        );
        fixture.write("test-only/pom.xml", "<project/>");
        fixture.write("test-only/src/test/java/Application.java", BOOT);
        fixture.write("config-only/pom.xml", "<project/>");
        fixture.write(
            "config-only/src/main/java/Configuration.java",
            "@SpringBootApplication public class Configuration {}",
        );
        fixture.write("fake/pom.xml", "<project/>");
        fixture.write("fake/src/main/java/Fake.java", "/* @SpringBootApplication public class Fake { public static void main(String[] args) {} } */ class Fake { String text = \"SpringApplication.run()\"; }");
        fixture.write("target/generated/pom.xml", "<project/>");
        fixture.write("target/generated/src/main/java/Application.java", BOOT);
        fixture.write("not-in-reactor/pom.xml", "<project/>");
        fixture.write("not-in-reactor/src/main/java/Application.java", BOOT);
        let result = discover_services(&fixture.0).unwrap();
        assert_eq!(result.projects.len(), 1, "{:?}", result.projects);
        assert_eq!(
            result.projects[0].path,
            display_path(&fixture.0.join("modules/app"))
        );
        assert_eq!(result.projects[0].main_classes, vec!["example.Application"]);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }
    #[test]
    fn requires_a_real_public_static_main_in_the_boot_class() {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .unwrap();
        for source in [
            "@SpringBootApplication class App { public void main(String[] a) {} }",
            "@SpringBootApplication class App { static void main(String[] a) {} }",
            "@SpringBootApplication class App { public static void main(int a) {} }",
            "@SpringBootApplication class Config {} class Tool { public static void main(String[] args) {} }",
            "class App { String s = \"@SpringBootApplication\"; public static void main(String[] a) {} }",
        ] {
            assert!(
                java_entry_points(&mut parser, source, &HashSet::new()).is_empty(),
                "{source}"
            );
        }
        assert_eq!(
            java_entry_points(
                &mut parser,
                "package sample; class App { public static void main(String... args) { SpringApplication.run(App.class,args); } }",
                &HashSet::new()
            ),
            vec!["sample.App"]
        );
        assert_eq!(
            java_entry_points(
                &mut parser,
                "@SpringBootApplication class App { public static void main(java.lang.String args[]) {} }",
                &HashSet::new()
            ),
            vec!["App"]
        );
    }
    #[test]
    fn recognizes_explicit_java_launcher_but_not_plugin_presence_alone() {
        let fixture = Fixture::new();
        fixture.write("pom.xml", "<project><build><plugins><plugin><artifactId>exec-maven-plugin</artifactId><configuration><mainClass>example.Tool</mainClass></configuration></plugin></plugins></build></project>");
        fixture.write(
            "src/main/java/example/Tool.java",
            "package example; public class Tool { public static void main(String[] args) {} }",
        );
        assert_eq!(
            discover_services(&fixture.0).unwrap().projects[0].main_classes,
            vec!["example.Tool"]
        );
        fixture.write("pom.xml", "<project><build><plugins><plugin><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>");
        assert!(discover_services(&fixture.0).unwrap().projects.is_empty());
    }
    #[test]
    fn node_libraries_without_launch_scripts_are_not_services() {
        let fixture = Fixture::new();
        fixture.write(
            "library/package.json",
            r#"{"name":"sdk","scripts":{"build":"tsc","test":"vitest"}}"#,
        );
        fixture.write("frontend/package.json", r#"{"name":"frontend","scripts":{"dev":"vite","build":"vite build","start":"node server.js"}}"#);
        fixture.write(
            "node_modules/dependency/package.json",
            r#"{"scripts":{"start":"node index.js"}}"#,
        );
        let result = discover_services(&fixture.0).unwrap();
        assert_eq!(result.projects.len(), 1);
        assert_eq!(result.projects[0].name, "frontend");
        assert_eq!(result.projects[0].launch_scripts, vec!["dev", "start"]);
    }
    #[test]
    fn rejects_external_modules_and_dtd_and_reports_bad_descriptors() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        outside.write("pom.xml", "<project/>");
        outside.write("src/main/java/Application.java", BOOT);
        fixture.write("pom.xml", &format!("<project><packaging>pom</packaging><modules><module>{}</module><module>.</module></modules></project>", display_path(&outside.0)));
        let result = discover_services(&fixture.0).unwrap();
        assert!(result.projects.is_empty());
        assert!(!result.warnings.is_empty());
        fixture.write("pom.xml", "<!DOCTYPE project [<!ENTITY external SYSTEM 'file:///etc/passwd'>]><project><name>&external;</name></project>");
        assert!(!discover_services(&fixture.0).unwrap().warnings.is_empty());
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "Read-only local Spring Boot service discovery; run explicitly"]
    fn real_extreme_workspace_contains_only_application_modules() {
        let result = discover_services(Path::new(r"D:\CompanyProject\extreme-plus")).unwrap();
        // There are 12 business modules plus two genuine Boot entry points in
        // common-docker and third-oa. Do not infer "library" from folder names.
        assert_eq!(result.projects.len(), 14, "{:?}", result.projects);
        assert!(result.projects.iter().any(|p| {
            p.name == "extreme-plus-system"
                && p.main_classes
                    .contains(&"ext.plus.module.system.SystemApplication".into())
        }));
        for project in result.projects {
            assert!(!project.main_classes.is_empty());
            assert!(
                ![
                    "extreme-plus",
                    "extreme-common",
                    "extreme-module",
                    "extreme-api",
                    "extreme-web",
                    "extreme-plugin"
                ]
                .contains(&project.name.as_str())
            );
            println!("{}: {}", project.name, project.main_classes.join(", "));
        }
    }

    #[test]
    fn aggregate_node_scripts_are_distinct_services() {
        let fixture = Fixture::new();
        fixture.write("package.json",r#"{"scripts":{"serve":"vite","system":"npm run serve --project=extreme-system","file":"pnpm run serve --project=extreme-file","admin":"yarn serve --project=admin","loop":"npm run loop","build":"vite build","test":"npm run serve","check":"npm run serve","empty":""}}"#);
        let result = discover_services(&fixture.0).unwrap();
        assert_eq!(result.projects.len(), 3);
        let scripts: Vec<_> = result
            .projects
            .iter()
            .map(|p| p.node_script.as_deref().unwrap())
            .collect();
        assert_eq!(scripts, vec!["admin", "file", "system"]);
        let keys: HashSet<_> = result.projects.iter().map(|p| p.runtime_key()).collect();
        assert_eq!(keys.len(), 3);
        let mut alias = result.projects[0].clone();
        alias.path = "D:/项目 空格".into();
        alias.node_script = Some("system".into());
        let mut literal = alias.clone();
        literal.path = "D:/项目 空格#node:system".into();
        literal.node_script = None;
        assert_ne!(alias.runtime_key(), literal.runtime_key());
        assert!(
            result
                .projects
                .iter()
                .all(|p| p.path == display_path(&fixture.0.canonicalize().unwrap()))
        );
    }

    #[test]
    fn mixed_java_node_root_keeps_independent_runtime_identities() {
        let fixture = Fixture::new();
        fixture.write("pom.xml", "<project/>");
        fixture.write("src/main/java/Application.java", BOOT);
        fixture.write(
            "package.json",
            r#"{"scripts":{"dev":"vite","start":"vite --host"}}"#,
        );
        let result = discover_services(&fixture.0).unwrap();
        assert_eq!(result.projects.len(), 3);
        assert!(
            result
                .projects
                .iter()
                .any(|p| matches!(p.kind, ProjectKind::Maven) && p.launch_scripts.is_empty())
        );
        assert_eq!(
            result
                .projects
                .iter()
                .map(|p| p.runtime_key())
                .collect::<HashSet<_>>()
                .len(),
            3
        );
    }
}
