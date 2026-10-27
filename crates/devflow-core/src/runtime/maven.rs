//! On-demand Maven builds shared by all entry points. Maven supplies the graph;
//! native fingerprints never attempt to interpret dependency/version semantics.
use super::managed::CommandSpec;
use crate::workspace::{WorkspaceError, display_path};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fs, io::{Read, Write}, path::{Path, PathBuf}, process::Command, thread, time::Duration};

type Result<T> = std::result::Result<T, WorkspaceError>;
const BRIDGE: &str = include_str!("maven_bridge.java");
// v1 could cache compile-only classpaths with runtime drivers missing.
const VERSION: u32 = 2;

pub(super) fn is_launcher_arg(arg: &str) -> bool {
    arg.contains("org.codehaus.plexus")
        || arg.replace('\\', "/").rsplit('/').next() == Some("DevflowMavenBridge.java")
}

fn error(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError { code: "maven_build", message: message.into() }
}

#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildMode { #[default] Workspace, Install }

#[derive(Default, Debug)]
pub struct BuildOptions {
    pub maven_profiles: Vec<String>,
    pub build_mode: BuildMode,
    pub force_build: bool,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct MavenPlan {
    pub reactor: PathBuf,
    pub configuration: PathBuf,
    pub boundary: PathBuf,
    pub target: PathBuf,
    pub home: PathBuf,
    pub java: CommandSpec,
    pub profiles: Vec<String>,
    pub mode: BuildMode,
    pub force: bool,
    #[serde(default)]
    pub local_dependencies: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Module {
    path: PathBuf,
    name: String,
    packaging: String,
    directory: PathBuf,
    output: PathBuf,
    inputs: Vec<PathBuf>,
    upstream: Vec<PathBuf>,
}
#[derive(Clone, Deserialize, Serialize)]
struct Model {
    modules: Vec<Module>,
    descriptors: Vec<PathBuf>,
    #[serde(default)]
    watched: Vec<PathBuf>,
    #[serde(default)]
    activation: Vec<PathBuf>,
}
fn resolved_path(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() { return Err(error("Maven 构建路径必须为绝对路径")); }
    if path.exists() { return Ok(path.canonicalize()?); }
    let parent = path.parent().ok_or_else(|| error("Maven 构建路径无效"))?;
    Ok(resolved_path(parent)?.join(path.file_name().ok_or_else(|| error("Maven 构建路径无效"))?))
}
fn normalize_model(model: &mut Model) -> Result<()> {
    for path in &mut model.descriptors { *path = resolved_path(path)?; }
    for module in &mut model.modules {
        module.path = resolved_path(&module.path)?;
        module.directory = resolved_path(&module.directory)?;
        module.output = resolved_path(&module.output)?;
        for path in module.inputs.iter_mut().chain(module.upstream.iter_mut()) { *path = resolved_path(path)?; }
    }
    for path in &mut model.watched { *path = resolved_path(path)?; }
    for path in &mut model.activation { *path = resolved_path(path)?; }
    Ok(())
}
#[derive(Clone, Deserialize, Serialize)]
struct Built { input: String, output: String }
#[derive(Clone, Deserialize, Serialize)]
struct Launch {
    model: Model, global: String, classpath: Vec<PathBuf>, artifacts: String,
    #[serde(default)]
    local_dependencies: Vec<String>,
}
#[derive(Default, Deserialize, Serialize)]
struct Cache {
    version: u32,
    modules: BTreeMap<PathBuf, Built>,
    launches: BTreeMap<PathBuf, Launch>,
    #[serde(default)]
    external: BTreeSet<PathBuf>,
    #[serde(default)]
    external_stamp: String,
}

// Two independent, explicitly specified streaming hashes, stable across Rust
// versions and also covering filenames/deletions. Not an authorization token.
struct Fingerprint(u64, u64);
impl Fingerprint {
    fn new() -> Self { Self(0xcbf29ce484222325, 0x84222325cbf29ce4) }
    fn add(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            self.1 = (self.1 ^ u64::from(byte)).rotate_left(7).wrapping_mul(0x9e3779b185ebca87);
        }
        self.0 ^= 0xff;
        self.1 ^= bytes.len() as u64;
    }
    fn finish(&self) -> String { format!("{:016x}{:016x}", self.0, self.1) }
}
fn hash(bytes: &[u8]) -> String { let mut value = Fingerprint::new(); value.add(bytes); value.finish() }

struct Scan { files: usize, bytes: u64, hash: Fingerprint }
impl Scan {
    fn new() -> Self { Self { files: 0, bytes: 0, hash: Fingerprint::new() } }
    fn visit(&mut self, path: &Path, content: bool, depth: usize) -> Result<()> {
        if depth > 48 || self.files > 100_000 || self.bytes > 256 * 1024 * 1024 {
            return Err(error("构建指纹检查超过预算，请缩小 Maven 工作区或输入目录"));
        }
        self.hash.add(path.to_string_lossy().as_bytes());
        let metadata = match fs::symlink_metadata(path) {
            Ok(value) => value,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => { self.hash.add(b"missing"); return Ok(()); }
            Err(err) => return Err(err.into()),
        };
        // Junctions are checked by canonical path in validate_model; symlinks
        // inside source/output trees are rejected rather than silently skipped.
        if metadata.file_type().is_symlink() { return Err(error(format!("构建指纹不支持符号链接: {}", display_path(path)))); }
        self.files += 1;
        if metadata.is_dir() {
            self.hash.add(b"directory");
            let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let excluded = ["node_modules", ".git", "target", "build", "dist", ".devflow"]
                    .iter().any(|name| entry.file_name().to_string_lossy().eq_ignore_ascii_case(name));
                if entry.file_type()?.is_dir() && excluded { continue; }
                // Every entry must stay inside the scanned tree, including
                // Windows junctions whose file_type is not always a symlink.
                if !entry.path().canonicalize()?.starts_with(path.canonicalize()?) {
                    return Err(error("构建输入或输出包含越界链接"));
                }
                self.visit(&entry.path(), content, depth + 1)?;
            }
        } else if metadata.is_file() {
            self.hash.add(&metadata.len().to_le_bytes());
            if content {
                self.bytes += metadata.len();
                if self.bytes > 256 * 1024 * 1024 { return Err(error("构建输入超过 256 MiB 检查预算")); }
                let mut file = fs::File::open(path)?;
                let mut bytes = [0; 32768];
                loop { let count = file.read(&mut bytes)?; if count == 0 { break; } self.hash.add(&bytes[..count]); }
            } else {
                self.hash.add(&metadata.modified()?.duration_since(std::time::UNIX_EPOCH).map_err(|e| error(e.to_string()))?.as_nanos().to_le_bytes());
            }
        }
        Ok(())
    }
}
fn fingerprint(paths: impl IntoIterator<Item = PathBuf>, content: bool) -> Result<String> {
    let paths: BTreeSet<_> = paths.into_iter().collect();
    let mut scan = Scan::new();
    for path in paths { scan.visit(&path, content, 0)?; }
    Ok(scan.hash.finish())
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 16 * 1024 * 1024 { return Err(error("Maven 构建记录过大")); }
    serde_json::from_reader(file).map_err(|e| error(e.to_string()))
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::File::create(&temporary)?;
    serde_json::to_writer(&mut file, value).map_err(|e| error(e.to_string()))?;
    file.flush()?;
    drop(file);
    fs::rename(temporary, path)?;
    Ok(())
}
fn log(directory: &Path, message: impl std::fmt::Display) -> Result<()> {
    super::logs::append(directory, "stdout", format!("[DevFlow] {message}\n").as_bytes())?;
    Ok(())
}

/// Identify aggregators through declared modules only, within an active folder.
/// Profile activation and the actual selected graph are then resolved by Maven.
pub(crate) fn reactor_root(target: &Path, boundary: &Path) -> Result<PathBuf> {
    let mut reactor = target.to_owned();
    for ancestor in target.ancestors().skip(1) {
        if !ancestor.starts_with(boundary) { break; }
        let pom = ancestor.join("pom.xml");
        if !pom.is_file() { continue; }
        if pom.metadata()?.len() > 2 * 1024 * 1024 { return Err(error("POM 超过 2 MiB")); }
        let text = fs::read_to_string(pom)?;
        let doc = roxmltree::Document::parse(text.trim_start_matches('\u{feff}')).map_err(|e| error(e.to_string()))?;
        let declared = doc.descendants().filter(|node| node.has_tag_name("module") && node.parent().is_some_and(|p| p.has_tag_name("modules")))
            .filter_map(|node| node.text()).any(|name| ancestor.join(name.trim()).canonicalize().is_ok_and(|path| path == reactor));
        if declared { reactor = ancestor.to_owned(); }
    }
    Ok(reactor)
}

fn validate_model(model: &Model, plan: &MavenPlan) -> Result<()> {
    if model.modules.is_empty() || model.modules.len() > 4096 || model.descriptors.len() > 4096 || model.watched.len() > 16384 || model.activation.len() > 16384 {
        return Err(error("Maven 模块数量无效或超过预算"));
    }
    for path in model.descriptors.iter().chain(model.modules.iter().flat_map(|m| [&m.path, &m.directory, &m.output].into_iter().chain(m.inputs.iter()))) {
        let mut existing = path.as_path();
        while !existing.exists() { existing = existing.parent().ok_or_else(|| error("构建路径无效"))?; }
        if !path.is_absolute() || !path.starts_with(&plan.boundary) || !existing.canonicalize()?.starts_with(&plan.boundary) {
            return Err(error(format!("构建路径超出工作区: {}", display_path(path))));
        }
    }
    let paths: BTreeSet<_> = model.modules.iter().map(|m| m.path.clone()).collect();
    if paths.len() != model.modules.len() || !paths.contains(&plan.target)
        || model.modules.iter().any(|m| m.upstream.iter().any(|p| !paths.contains(p))) {
        return Err(error("Maven 返回了无效的依赖图"));
    }
    if plan.mode == BuildMode::Workspace && model.modules.iter().any(|m| !["jar", "pom", "maven-plugin"].contains(&m.packaging.as_str())) {
        return Err(error("依赖链包含特殊打包模块，请在启动配置中选择“构建并安装”；默认模式只复用普通 jar 模块的编译输出"));
    }
    for module in &model.modules {
        if module.packaging != "pom" && (!module.directory.starts_with(&module.path) || module.directory == module.path
            || !module.output.starts_with(&module.directory) || module.output == module.directory
            || module.output.components().any(|c| [".git", ".devflow", "node_modules"].iter().any(|name| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(name)))
            || module.inputs.iter().any(|input| input.starts_with(&module.output) || module.output.starts_with(input))) {
            return Err(error("编译输出必须位于模块内部的独立构建目录，且不能覆盖源码、资源或元数据目录"));
        }
        if module.packaging != "pom" && model.modules.iter().any(|other| other.path != module.path
            && (other.path.starts_with(&module.output) || (other.packaging != "pom" && (module.output.starts_with(&other.output) || other.output.starts_with(&module.output))))) {
            return Err(error("多个模块不能共享或嵌套编译输出目录"));
        }
    }
    Ok(())
}

fn global(plan: &MavenPlan, model: &Model) -> Result<String> {
    let mut paths = model.descriptors.clone();
    paths.extend(model.watched.iter().cloned());
    paths.extend([plan.configuration.join(".mvn"), plan.home.join("conf/settings.xml"), plan.home.join("conf/toolchains.xml")]);
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        paths.extend([PathBuf::from(&home).join(".m2/settings.xml"), PathBuf::from(home).join(".m2/toolchains.xml")]);
    }
    let mut value = Fingerprint::new();
    value.add(fingerprint(paths, true)?.as_bytes());
    for path in &model.activation {
        value.add(path.to_string_lossy().as_bytes());
        value.add(if path.exists() { b"exists" } else { b"missing" });
    }
    value.add(BRIDGE.as_bytes());
    value.add(format!("{}:{:?}:{:?}", VERSION, plan.mode, plan.profiles).as_bytes());
    let mut environment: Vec<_> = std::env::vars_os().collect();
    environment.sort();
    for (key, item) in environment {
        value.add(key.to_string_lossy().as_bytes()); value.add(item.to_string_lossy().as_bytes());
    }
    value.add(serde_json::to_string(&plan.java).map_err(|e| error(e.to_string()))?.as_bytes());
    value.add(fingerprint(fs::read_dir(plan.home.join("lib"))?.filter_map(|e| e.ok().map(|e| e.path())), false)?.as_bytes());
    let java = plan.java.command();
    let requested = PathBuf::from(java.get_program());
    let program = if requested.is_absolute() { requested } else {
        std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths)
            .map(|path| path.join(if cfg!(windows) { "java.exe" } else { "java" })).find(|path| path.is_file()))
            .unwrap_or(requested)
    };
    if program.is_absolute() {
        value.add(fingerprint([program.clone()], false)?.as_bytes());
        if let Some(home) = program.parent().and_then(Path::parent) { value.add(fingerprint([home.join("release")], true)?.as_bytes()); }
    }
    Ok(value.finish())
}

fn inputs(model: &Model, global: &str) -> Result<BTreeMap<PathBuf, String>> {
    let mut result: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut pending: Vec<_> = model.modules.iter().collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next = Vec::new();
        for module in pending {
            if module.upstream.iter().any(|p| !result.contains_key(p)) { next.push(module); continue; }
            let mut hash = Fingerprint::new();
            hash.add(global.as_bytes());
            let mut paths = module.inputs.clone();
            paths.push(module.path.join("pom.xml"));
            hash.add(fingerprint(paths, true)?.as_bytes());
            for dependency in &module.upstream { hash.add(result[dependency].as_bytes()); }
            result.insert(module.path.clone(), hash.finish());
        }
        if next.len() == before { return Err(error("Maven 模块依赖图含循环")); }
        pending = next;
    }
    Ok(result)
}
fn output(module: &Module) -> Result<String> {
    if module.packaging == "pom" { return Ok("pom".into()); }
    if !module.output.is_dir() { return Ok("missing".into()); }
    fingerprint([module.output.clone()], false)
}
fn dirty(model: &Model, inputs: &BTreeMap<PathBuf, String>, cache: &Cache, force: bool) -> Result<BTreeSet<PathBuf>> {
    let mut changed = BTreeSet::new();
    for module in &model.modules {
        let stamp = output(module)?;
        if force || stamp == "missing" || cache.modules.get(&module.path).is_none_or(|built| built.input != inputs[&module.path] || built.output != stamp) {
            changed.insert(module.path.clone());
        }
    }
    // Output deletion/change also invalidates dependent modules, even when
    // their source hashes themselves have not changed.
    loop {
        let before = changed.len();
        for module in &model.modules {
            if module.upstream.iter().any(|path| changed.contains(path)) { changed.insert(module.path.clone()); }
        }
        if before == changed.len() { break; }
    }
    Ok(changed)
}

/// The callback uses the durable host's existing cancellation/PID cleanup.
pub(crate) fn build(
    plan: &MavenPlan, directory: &Path,
    mut execute: impl FnMut(Command, &mut dyn FnMut() -> Result<()>) -> Result<Option<i32>>,
) -> Result<Option<String>> {
    let storage = directory.ancestors().nth(2).ok_or_else(|| error("运行记录路径无效"))?.join("maven").join(hash(plan.reactor.to_string_lossy().as_bytes()));
    fs::create_dir_all(&storage)?;
    let lock_directory = plan.reactor.join(".devflow");
    fs::create_dir_all(&lock_directory)?;
    if !lock_directory.canonicalize()?.starts_with(&plan.boundary) { return Err(error("构建锁目录超出工作区")); }
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_directory.join("maven-build.lock"))?;
    let mut waiting = false;
    loop {
        if directory.join("stop").exists() { return Ok(None); }
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) => {
                if !waiting { log(directory, "等待同一 Maven 工程的构建完成…")?; waiting = true; }
                thread::sleep(Duration::from_millis(100));
            }
            Err(std::fs::TryLockError::Error(err)) => return Err(err.into()),
        }
    }
    let flavor = hash(format!("{:?}:{:?}", plan.mode, plan.profiles).as_bytes());
    let cache_file = storage.join(format!("cache-{flavor}.json"));
    let mut cache: Cache = read_json(&cache_file).unwrap_or_default();
    if cache.version != VERSION {
        if cache.version != 0 { log(directory, "Maven 缓存版本已更新，重新解析依赖并建立构建基线…")?; }
        cache = Cache::default(); cache.version = VERSION;
    }
    log(directory, "检查 Maven 构建输入和已有产物…")?;
    for dependency in &plan.local_dependencies { log(directory, &format!("本地运行依赖: {dependency}"))?; }
    let external_changed = fingerprint(cache.external.iter().cloned(), false)? != cache.external_stamp;
    if !plan.force && !external_changed {
        if let Some(launch) = cache.launches.get(&plan.target) {
            if launch.local_dependencies == plan.local_dependencies && validate_model(&launch.model, plan).is_ok() {
                let current_global = global(plan, &launch.model)?;
                let current_inputs = inputs(&launch.model, &current_global)?;
                if current_global == launch.global && dirty(&launch.model, &current_inputs, &cache, false)?.is_empty()
                    && launch.classpath.iter().all(|p| p.exists())
                    && fingerprint(launch.classpath.clone(), false)? == launch.artifacts {
                    log(directory, "源码、配置和依赖产物未变化，跳过 Maven，直接启动 Java。")?;
                    return classpath(&launch.classpath).map(Some);
                }
            }
        }
    }
    log(directory, "解析 Maven 有效模型及目标模块上游依赖…")?;
    fs::write(directory.join("local-dependencies.txt"), plan.local_dependencies.iter().filter(|value| super::local_dependencies::coordinate(value)).cloned().collect::<Vec<_>>().join("\n"))?;
    let bridge = storage.join("DevflowMavenBridge.java");
    if fs::read_to_string(&bridge).ok().as_deref() != Some(BRIDGE) { fs::write(&bridge, BRIDGE)?; }
    let mut command = plan.java.command();
    let libs = std::env::join_paths([PathBuf::from(display_path(&plan.home.join("lib/*"))), PathBuf::from(display_path(&plan.home.join("boot/*")))]).map_err(|e| error(e.to_string()))?;
    command.arg(format!("-Dmaven.home={}", display_path(&plan.home)))
        .arg(format!("-Dmaven.conf={}", display_path(&plan.home.join("conf"))))
        .arg(format!("-Dmaven.multiModuleProjectDirectory={}", display_path(&plan.configuration)))
        .arg("-cp").arg(libs).arg(display_path(&bridge))
        .arg(display_path(directory)).arg(display_path(&plan.target)).arg(display_path(&plan.boundary)).arg((plan.mode == BuildMode::Install).to_string())
        .args(["-B", "-Dstyle.color=never", "-DskipTests", "-pl"])
        .arg(if plan.target == plan.reactor { ".".into() } else { display_path(plan.target.strip_prefix(&plan.reactor).map_err(|e| error(e.to_string()))?) })
        .arg("-am").arg(if plan.mode == BuildMode::Install { "install" } else { "compile" }).current_dir(&plan.reactor);
    if !plan.profiles.is_empty() { command.arg("-P").arg(plan.profiles.join(",")); }
    let mut preparation: Option<(Model, String, BTreeMap<PathBuf, String>, BTreeSet<PathBuf>)> = None;
    let mut tick = || -> Result<()> {
        if preparation.is_some() || !directory.join("model.json").exists() { return Ok(()); }
        let mut model: Model = read_json(&directory.join("model.json"))?;
        normalize_model(&mut model)?;
        validate_model(&model, plan)?;
        let current_global = global(plan, &model)?;
        let current_inputs = inputs(&model, &current_global)?;
        if external_changed { cache.modules.clear(); }
        let mut selected = dirty(&model, &current_inputs, &cache, plan.force)?;
        // The application always participates on a miss; the bridge explicitly
        // resolves its runtime graph after selected outputs have been built.
        selected.insert(plan.target.clone());
        if plan.mode == BuildMode::Install {
            // Packaging plugins may require jars/classifiers from every upstream
            // project; use the complete necessary closure in compatibility mode.
            selected.extend(model.modules.iter().map(|m| m.path.clone()));
        }
        for path in &selected { cache.modules.remove(path); }
        cache.launches.remove(&plan.target);
        write_json(&cache_file, &cache)?; // Failure/cancellation must stay dirty.
        // Rebuild only selected outputs. This removes classes/resources left
        // behind by deleted sources and forces affected downstream compilation
        // even when Maven's timestamp checks would otherwise skip it.
        for module in &model.modules {
            if selected.contains(&module.path) && module.packaging != "pom" && module.output.exists() {
                let output = module.output.canonicalize()?;
                let root = module.path.canonicalize()?;
                if output == root || output == module.directory || !output.starts_with(&module.directory)
                    || !output.starts_with(&root) || !output.starts_with(&plan.boundary) {
                    return Err(error("拒绝清理越界编译输出"));
                }
                fs::remove_dir_all(&output)?;
            }
        }
        let mut lines = String::new();
        for path in &selected {
            let display = display_path(path);
            let path = display.as_str();
            if path.contains(['\n', '\r']) { return Err(error("模块路径包含换行")); }
            lines.push_str(path); lines.push('\n');
        }
        fs::write(directory.join("selection.tmp"), lines)?;
        fs::rename(directory.join("selection.tmp"), directory.join("selection.txt"))?;
        preparation = Some((model, current_global, current_inputs, selected));
        Ok(())
    };
    let code = execute(command, &mut tick)?;
    if directory.join("stop").exists() { return Ok(None); }
    if code != Some(0) { return Err(error(format!("Maven 构建失败（退出码 {code:?}），未启动服务；请查看上方具体模块错误。"))); }
    let (model, before_global, before_inputs, selected) = preparation.ok_or_else(|| error("Maven 未返回构建模型，需要 Maven 3.6+ 和完整 JDK 11+"))?;
    let after_global = global(plan, &model)?;
    let after_inputs = inputs(&model, &after_global)?;
    if before_global != after_global || before_inputs != after_inputs {
        return Err(error("构建期间源码或配置发生变化，本次结果不缓存也不启动，请重新启动服务"));
    }
    let returned: Vec<PathBuf> = read_json(&directory.join("classpath.json"))?;
    let mut paths = returned.iter().map(|path| resolved_path(path)).collect::<Result<Vec<_>>>()?;
    for value in &plan.local_dependencies {
        if !super::local_dependencies::coordinate(value) {
            let path = Path::new(value).canonicalize()?;
            if !path.starts_with(&plan.boundary) || !path.is_file() { return Err(error("本地 JAR 已失效或超出工作区")); }
            if !paths.contains(&path) { paths.push(path); }
        }
    }
    if paths.is_empty() || paths.len() > 16384 || paths.iter().any(|p| !p.is_absolute() || !p.exists()) {
        return Err(error("Maven 返回的运行 classpath 无效或产物缺失"));
    }
    for module in &model.modules {
        if selected.contains(&module.path) {
            let stamp = output(module)?;
            if stamp == "missing" { return Err(error(format!("模块编译输出缺失: {}", module.name))); }
            cache.modules.insert(module.path.clone(), Built { input: after_inputs[&module.path].clone(), output: stamp });
        }
    }
    let artifacts = fingerprint(paths.clone(), false)?;
    let result = classpath(&paths)?;
    cache.external.retain(|path| path.exists());
    for path in &paths {
        if model.modules.iter().all(|module| !path.starts_with(&module.directory)) {
            cache.external.insert(path.clone());
            let pom = path.with_extension("pom");
            if pom.is_file() { cache.external.insert(pom); }
        }
    }
    cache.external_stamp = fingerprint(cache.external.iter().cloned(), false)?;
    cache.launches.insert(plan.target.clone(), Launch { model, global: after_global, classpath: paths, artifacts, local_dependencies: plan.local_dependencies.clone() });
    write_json(&cache_file, &cache)?;
    log(directory, "构建成功，已记录构建指纹和运行 classpath。")?;
    Ok(Some(result))
}

fn classpath(paths: &[PathBuf]) -> Result<String> {
    std::env::join_paths(paths.iter().map(|path| PathBuf::from(display_path(path)))).map_err(|e| error(e.to_string()))?.into_string().map_err(|_| error("classpath 不是 UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("devflow-maven-中文 空格-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap(); Self(path.canonicalize().unwrap())
        }
        fn write(&self, name: &str, value: &str) {
            let path = self.0.join(name); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, value).unwrap();
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn content_fingerprint_tracks_edits_resources_and_deleted_files() {
        let f = Fixture::new(); f.write("src/A.java", "class A {}"); f.write("src/config.yml", "中文: true");
        let before = fingerprint([f.0.join("src")], true).unwrap();
        f.write("src/A.java", "class B {}");
        let edited = fingerprint([f.0.join("src")], true).unwrap(); assert_ne!(before, edited);
        fs::remove_file(f.0.join("src/config.yml")).unwrap();
        assert_ne!(edited, fingerprint([f.0.join("src")], true).unwrap());
    }
    #[test]
    fn root_follows_aggregator_declarations_not_parent_pom_location() {
        let f = Fixture::new();
        f.write("pom.xml", "<project><modules><module>modules</module></modules></project>");
        f.write("modules/pom.xml", "<project><modules><module>system</module></modules></project>");
        f.write("modules/system/pom.xml", "<project/>");
        let target = f.0.join("modules/system");
        assert_eq!(reactor_root(&target, &f.0).unwrap(), f.0);
        assert_eq!(reactor_root(&target, &target).unwrap(), target);
        f.write("pom.xml", "<project/>");
        assert_eq!(reactor_root(&target, &f.0).unwrap(), f.0.join("modules"));
    }
    #[test]
    fn changed_upstream_invalidates_downstream_but_not_siblings() {
        let f = Fixture::new();
        let module = |name: &str, upstream: Vec<PathBuf>| Module { path: f.0.join(name), name: name.into(), packaging: "jar".into(), directory: f.0.join(name).join("target"), output: f.0.join(name).join("target/classes"), inputs: vec![f.0.join(name).join("src")], upstream };
        let core = module("core", vec![]); let system = module("system", vec![core.path.clone()]); let other = module("other", vec![]);
        for m in [&core, &system, &other] { f.write(&format!("{}/src/A.java", m.name), "class A {}"); f.write(&format!("{}/target/classes/A.class", m.name), "bytecode"); }
        let model = Model { modules: vec![core.clone(), system.clone(), other.clone()], descriptors: vec![], watched: vec![], activation: vec![] };
        let before = inputs(&model, "global").unwrap();
        let mut cache = Cache::default();
        for m in &model.modules { cache.modules.insert(m.path.clone(), Built { input: before[&m.path].clone(), output: output(m).unwrap() }); }
        assert!(dirty(&model, &before, &cache, false).unwrap().is_empty());
        f.write("core/src/A.java", "class Changed {}");
        let after = inputs(&model, "global").unwrap();
        assert_eq!(dirty(&model, &after, &cache, false).unwrap(), BTreeSet::from([core.path, system.path]));
        fs::remove_dir_all(&other.output).unwrap();
        assert!(dirty(&model, &after, &cache, false).unwrap().contains(&other.path));
    }
    #[test]
    fn classpath_preserves_windows_drive_letters_spaces_and_chinese() {
        let paths = vec![PathBuf::from(if cfg!(windows) { r"D:\中文 空格\target\classes" } else { "/tmp/中文 空格/target/classes" })];
        let joined = classpath(&paths).unwrap();
        assert_eq!(std::env::split_paths(&joined).collect::<Vec<_>>(), paths);
    }
    #[test]
    fn successful_build_is_reused_and_failed_build_cannot_hit_old_cache() {
        let f = Fixture::new();
        f.write("pom.xml", "<project/>"); f.write("app/pom.xml", "<project/>");
        f.write("app/src/A.java", "class A {}"); f.write("tools/maven/lib/runtime.jar", "fixture");
        let target = f.0.join("app");
        let module = Module { path: target.clone(), name: "app".into(), packaging: "jar".into(), directory: target.join("target"), output: target.join("target/classes"), inputs: vec![target.join("src")], upstream: vec![] };
        let model = Model { modules: vec![module.clone()], descriptors: vec![f.0.join("pom.xml"), target.join("pom.xml")], watched: vec![], activation: vec![] };
        let plan = MavenPlan { reactor: f.0.clone(), configuration: f.0.clone(), boundary: f.0.clone(), target: target.clone(), home: f.0.join("tools/maven"), java: CommandSpec::from(&Command::new("java")), profiles: vec![], mode: BuildMode::Workspace, force: false, local_dependencies: vec![] };
        let directory = |name: &str| { let path = f.0.join("state/runtime/service").join(name); fs::create_dir_all(&path).unwrap(); path };
        let first = directory("first");
        let result = build(&plan, &first, |_, tick| {
            write_json(&first.join("model.json"), &model)?; tick()?;
            assert!(first.join("selection.txt").is_file());
            fs::create_dir_all(&module.output)?; fs::write(module.output.join("A.class"), b"bytecode")?;
            write_json(&first.join("classpath.json"), &vec![module.output.clone()])?;
            Ok(Some(0))
        }).unwrap();
        assert!(result.is_some());
        assert!(build(&plan, &directory("second"), |_, _| panic!("a cache hit must never launch Maven")).unwrap().is_some());
        // A v1 launch can have perfectly matching fingerprints while missing
        // runtime dependencies. It must not survive the cache version upgrade.
        let storage = first.ancestors().nth(2).unwrap().join("maven").join(hash(plan.reactor.to_string_lossy().as_bytes()));
        let flavor = hash(format!("{:?}:{:?}", plan.mode, plan.profiles).as_bytes());
        let cache_file = storage.join(format!("cache-{flavor}.json"));
        let mut legacy: Cache = read_json(&cache_file).unwrap();
        legacy.version = 1;
        write_json(&cache_file, &legacy).unwrap();
        let upgraded = directory("upgraded");
        assert!(build(&plan, &upgraded, |_, tick| {
            write_json(&upgraded.join("model.json"), &model)?; tick()?;
            fs::create_dir_all(&module.output)?; fs::write(module.output.join("A.class"), b"bytecode")?;
            write_json(&upgraded.join("classpath.json"), &vec![module.output.clone()])?;
            Ok(Some(0))
        }).unwrap().is_some());
        assert_eq!(read_json::<Cache>(&cache_file).unwrap().version, VERSION);
        assert!(build(&plan, &directory("upgraded-hit"), |_, _| panic!("upgraded cache should be reused")).unwrap().is_some());
        f.write("app/src/A.java", "class Changed {}");
        let failed = directory("failed");
        assert!(build(&plan, &failed, |_, tick| { write_json(&failed.join("model.json"), &model)?; tick()?; Ok(Some(1)) }).is_err());
        assert!(!module.output.exists(), "selected stale output is cleared before compiling");
        let retry = directory("retry");
        let mut invoked = false;
        assert!(build(&plan, &retry, |_, tick| {
            invoked = true; write_json(&retry.join("model.json"), &model)?; tick()?; Ok(Some(1))
        }).is_err());
        assert!(invoked);
    }
    #[test]
    #[ignore = "requires MAVEN_HOME/M2_HOME, a full JDK 11+, and Maven plugin downloads"]
    fn real_maven_resolves_runtime_graph_and_reuses_complete_classpath() {
        let f = Fixture::new();
        let home = PathBuf::from(std::env::var_os("MAVEN_HOME").or_else(|| std::env::var_os("M2_HOME"))
            .expect("set MAVEN_HOME or M2_HOME to a Maven 3.6+ installation")).canonicalize().unwrap();
        let group = format!("devflow.fixture.g{}", uuid::Uuid::new_v4().simple());
        // Unique coordinates and a file repository avoid changing any real
        // project's POM or relying on a particular database driver's version.
        let dependency = |name: &str, scope: &str| format!("<dependency><groupId>{group}</groupId><artifactId>{name}</artifactId><version>1</version><scope>{scope}</scope></dependency>");
        let publish = |name: &str, dependencies: &str| {
            let base = format!("repo/{}/{name}/1/{name}-1", group.replace('.', "/"));
            f.write(&format!("{base}.pom"), &format!("<project><modelVersion>4.0.0</modelVersion><groupId>{group}</groupId><artifactId>{name}</artifactId><version>1</version><dependencies>{dependencies}</dependencies></project>"));
            // Valid empty ZIP/JAR: these fixtures exercise dependency resolution,
            // including artifacts never referenced by application source code.
            fs::write(f.0.join(format!("{base}.jar")), [b'P', b'K', 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        };
        publish("transitive", ""); publish("excluded", ""); publish("provided", ""); publish("test-only", "");
        publish("driver", &format!("{}{}", dependency("transitive", "compile"), dependency("excluded", "compile")));
        publish("local-only", "");
        publish("local-driver", &dependency("local-only", "runtime"));
        let repository = format!("file:///{}", display_path(&f.0.join("repo")).replace('\\', "/").trim_start_matches('/').replace(' ', "%20"));
        f.write("pom.xml", &format!(r#"<project><modelVersion>4.0.0</modelVersion><groupId>{group}</groupId><artifactId>parent</artifactId><version>1</version><packaging>pom</packaging>
<modules><module>library</module><module>app</module></modules>
<repositories><repository><id>fixture</id><url>{repository}</url></repository></repositories>
<build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version><configuration><release>11</release></configuration></plugin></plugins></build></project>"#));
        let parent = format!("<parent><groupId>{group}</groupId><artifactId>parent</artifactId><version>1</version></parent>");
        f.write("library/pom.xml", &format!("<project><modelVersion>4.0.0</modelVersion>{parent}<artifactId>library</artifactId></project>"));
        f.write("library/src/main/java/Library.java", "public class Library {} ");
        let driver = format!("<dependency><groupId>{group}</groupId><artifactId>driver</artifactId><version>1</version><scope>runtime</scope><exclusions><exclusion><groupId>{group}</groupId><artifactId>excluded</artifactId></exclusion></exclusions></dependency>");
        f.write("app/pom.xml", &format!("<project><modelVersion>4.0.0</modelVersion>{parent}<artifactId>app</artifactId><dependencies>{}{}{}{driver}</dependencies></project>", dependency("library", "compile"), dependency("provided", "provided"), dependency("test-only", "test")));
        f.write("app/src/main/java/App.java", "public class App { Library library; public static void main(String[] args) {} }");
        let target = f.0.join("app");
        let mut plan = MavenPlan { reactor: f.0.clone(), configuration: f.0.clone(), boundary: f.0.clone(), target,
            home, java: CommandSpec::from(&Command::new("java")), profiles: vec![], mode: BuildMode::Workspace, force: false, local_dependencies: vec![format!("{group}:local-driver:1")] };
        let run = |name: &str| { let path = f.0.join("state/runtime/service").join(name); fs::create_dir_all(&path).unwrap(); path };
        let execute = |mut command: Command, tick: &mut dyn FnMut() -> Result<()>| -> Result<Option<i32>> {
            // Use production process configuration and always reap Maven,
            // including tick errors and timeout paths on Windows.
            super::super::process::configure(&mut command);
            command.stdout(std::process::Stdio::inherit()).stderr(std::process::Stdio::inherit());
            let mut child = command.spawn()?;
            let identity = super::super::process::identity(child.id());
            let deadline = std::time::Instant::now() + Duration::from_secs(180);
            let result = loop {
                if let Err(err) = tick() { break Err(err); }
                match child.try_wait() {
                    Ok(Some(status)) => break Ok(status.code()),
                    Ok(None) => {},
                    Err(err) => break Err(err.into()),
                }
                if std::time::Instant::now() >= deadline { break Err(error("Maven fixture timed out")); }
                thread::sleep(Duration::from_millis(25));
            };
            if result.is_err() {
                if let Some(identity) = identity { let _ = super::super::process::kill_tree(identity); }
                let _ = child.kill(); let _ = child.wait();
            }
            result
        };
        let first = build(&plan, &run("first"), execute).unwrap().unwrap();
        let paths: Vec<_> = std::env::split_paths(&first).collect();
        assert!(paths.iter().any(|p| p.ends_with("driver-1.jar")));
        assert!(paths.iter().any(|p| p.ends_with("transitive-1.jar")));
        assert!(paths.iter().any(|p| p.ends_with("local-driver-1.jar")));
        assert!(paths.iter().any(|p| p.ends_with("local-only-1.jar")));
        assert!(!fs::read_to_string(f.0.join("app/pom.xml")).unwrap().contains("local-driver"));
        for absent in ["excluded-1.jar", "provided-1.jar", "test-only-1.jar"] {
            assert!(!paths.iter().any(|p| p.ends_with(absent)), "unexpected runtime artifact: {absent}");
        }
        assert!(paths.iter().any(|p| p.ends_with("library/target/classes")));
        let cached = build(&plan, &run("cached"), |_, _| panic!("complete runtime classpath should be reused")).unwrap().unwrap();
        assert_eq!(first, cached);
        // Rebuild the application while reusing the library through the
        // workspace reader, then resolve the same complete runtime graph.
        f.write("app/src/main/java/App.java", "public class App { Library library; public static void main(String[] args) { System.out.println(1); } }");
        assert_eq!(first, build(&plan, &run("changed"), execute).unwrap().unwrap());
        plan.local_dependencies.clear();
        let without = build(&plan, &run("without-local"), execute).unwrap().unwrap();
        assert!(!without.contains("local-driver-1.jar"));
        assert!(!without.contains("local-only-1.jar"));
        assert_eq!(without, build(&plan, &run("without-local-cached"), |_, _| panic!("removed local dependencies must also cache correctly")).unwrap().unwrap());
    }
    #[test]
    fn reactor_lock_excludes_other_handles_and_releases_on_drop() {
        let f = Fixture::new(); f.write(".devflow/maven-build.lock", "");
        let lock = fs::OpenOptions::new().read(true).write(true).open(f.0.join(".devflow/maven-build.lock")).unwrap();
        lock.try_lock().unwrap();
        let another = fs::OpenOptions::new().read(true).write(true).open(f.0.join(".devflow/maven-build.lock")).unwrap();
        assert!(matches!(another.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        drop(lock);
        assert!(another.try_lock().is_ok());
    }
}
