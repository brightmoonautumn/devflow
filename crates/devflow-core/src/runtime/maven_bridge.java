// Private Maven 3 bridge. Executed by the selected JDK's source-file launcher
// only on a cache miss. Maven owns model interpolation, profiles and resolution.
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import org.apache.maven.AbstractMavenLifecycleParticipant;
import org.apache.maven.MavenExecutionException;
import org.apache.maven.cli.MavenCli;
import org.apache.maven.execution.MavenSession;
import org.apache.maven.project.MavenProject;
import org.apache.maven.project.DefaultDependencyResolutionRequest;
import org.apache.maven.project.ProjectDependenciesResolver;
import org.apache.maven.RepositoryUtils;
import org.codehaus.plexus.PlexusContainer;
import org.eclipse.aether.DefaultRepositorySystemSession;
import org.eclipse.aether.artifact.Artifact;
import org.eclipse.aether.repository.WorkspaceReader;
import org.eclipse.aether.repository.WorkspaceRepository;
import org.eclipse.aether.util.filter.DependencyFilterUtils;

class DevflowMavenBridge {
    static Path exchange, target, boundary;
    static boolean install;
    static MavenProject application;
    static ProjectDependenciesResolver dependencies;
    static final Map<String, MavenProject> reusable = new HashMap<>();
    static final Map<String, MavenProject> local = new HashMap<>();

    public static void main(String[] args) throws Exception {
        exchange = Paths.get(args[0]).toRealPath();
        target = Paths.get(args[1]).toRealPath();
        boundary = Paths.get(args[2]).toRealPath();
        install = Boolean.parseBoolean(args[3]);
        MavenCli cli = new MavenCli() {
            @Override protected void customizeContainer(PlexusContainer container) {
                try {
                    dependencies = container.lookup(ProjectDependenciesResolver.class);
                } catch (Exception ex) {
                    throw new IllegalStateException("DevFlow 无法获取 Maven 依赖解析器", ex);
                }
                container.addComponent(new Participant(), AbstractMavenLifecycleParticipant.class, "devflow");
            }
        };
        int result = cli.doMain(Arrays.copyOfRange(args, 4, args.length),
                System.getProperty("user.dir"), System.out, System.err);
        if (result == 0 && !Files.exists(exchange.resolve("classpath.json"))) {
            System.err.println("DevFlow: Maven 未返回运行 classpath（需要 Maven 3.6+ / 完整 JDK 11+）");
            result = 1;
        }
        System.exit(result);
    }

    static class Participant extends AbstractMavenLifecycleParticipant {
        @Override public void afterSessionStart(MavenSession session) {
            // Register before Maven makes the repository session read-only.
            // The built-in reactor reader remains first; this reader supplies
            // verified outputs for clean modules omitted from this execution.
            DefaultRepositorySystemSession repository = (DefaultRepositorySystemSession) session.getRepositorySession();
            WorkspaceReader previous = repository.getWorkspaceReader();
            repository.setWorkspaceReader(new WorkspaceReader() {
                final WorkspaceRepository workspace = new WorkspaceRepository("devflow");
                public WorkspaceRepository getRepository() { return workspace; }
                public File findArtifact(Artifact artifact) {
                    MavenProject project = reusable.get(gav(artifact));
                    if (project != null && artifact.getClassifier().isEmpty()) {
                        if ("pom".equals(artifact.getExtension())) return project.getFile();
                        if ("jar".equals(project.getPackaging()) && "jar".equals(artifact.getExtension()))
                            return new File(project.getBuild().getOutputDirectory());
                    }
                    return previous == null ? null : previous.findArtifact(artifact);
                }
                public List<String> findVersions(Artifact artifact) {
                    List<String> versions = new ArrayList<>();
                    for (MavenProject project : reusable.values())
                        if (project.getGroupId().equals(artifact.getGroupId()) && project.getArtifactId().equals(artifact.getArtifactId()))
                            versions.add(project.getVersion());
                    if (previous != null) versions.addAll(previous.findVersions(artifact));
                    return versions;
                }
            });
        }

        @Override public void afterProjectsRead(MavenSession session) throws MavenExecutionException {
            try {
                List<MavenProject> closure = new ArrayList<>(session.getProjects());
                List<String> modules = new ArrayList<>(), descriptors = new ArrayList<>();
                Set<String> watched = new TreeSet<>();
                Set<String> activation = new TreeSet<>();
                watch(watched, session.getRequest().getUserSettingsFile());
                watch(watched, session.getRequest().getGlobalSettingsFile());
                watch(watched, session.getRequest().getUserToolchainsFile());
                watch(watched, session.getRequest().getGlobalToolchainsFile());
                for (MavenProject project : session.getAllProjects()) {
                    Path base = project.getBasedir().toPath().toRealPath();
                    inside(base);
                    descriptors.add(q(project.getFile().toPath().toRealPath().toString()));
                    for (MavenProject parent = project.getParent(); parent != null; parent = parent.getParent()) watch(watched, parent.getFile());
                    for (MavenProject item = project; item != null; item = item.getParent())
                        for (org.apache.maven.model.Profile profile : item.getOriginalModel().getProfiles())
                            if (profile.getActivation() != null && profile.getActivation().getFile() != null) {
                                watchActivation(activation, item, profile.getActivation().getFile().getExists());
                                watchActivation(activation, item, profile.getActivation().getFile().getMissing());
                            }
                    for (org.apache.maven.model.Dependency dependency : project.getDependencies()) watchSource(watched, dependency.getLocation(""));
                    if (project.getDependencyManagement() != null)
                        for (org.apache.maven.model.Dependency dependency : project.getDependencyManagement().getDependencies()) watchSource(watched, dependency.getLocation(""));
                }
                for (MavenProject project : closure) {
                    Path base = project.getBasedir().toPath().toRealPath();
                    inside(base);
                    if (base.equals(target)) application = project;
                    local.put(gav(project), project);
                    List<String> inputs = new ArrayList<>(), upstream = new ArrayList<>();
                    for (String source : project.getCompileSourceRoots()) inputs.add(q(checked(source)));
                    for (org.apache.maven.model.Resource resource : project.getResources())
                        inputs.add(q(checked(resource.getDirectory())));
                    for (MavenProject dependency : session.getProjectDependencyGraph().getUpstreamProjects(project, false))
                        if (closure.contains(dependency)) upstream.add(q(dependency.getBasedir().toPath().toRealPath().toString()));
                    modules.add("{\"path\":" + q(base.toString()) + ",\"name\":" + q(project.getArtifactId())
                        + ",\"packaging\":" + q(project.getPackaging()) + ",\"directory\":" + q(checked(project.getBuild().getDirectory())) + ",\"output\":" + q(checked(project.getBuild().getOutputDirectory()))
                        + ",\"inputs\":[" + String.join(",", inputs) + "],\"upstream\":[" + String.join(",", upstream) + "]}");
                }
                if (application == null) throw new IOException("启动模块不在选定 Maven Reactor 中");
                if (!install) {
                    for (MavenProject project : closure)
                        for (org.apache.maven.model.Plugin plugin : project.getBuildPlugins())
                            if (!plugin.getExecutions().isEmpty() && local.containsKey(plugin.getGroupId() + ":" + plugin.getArtifactId() + ":" + plugin.getVersion()))
                                throw new IOException("本地 Maven 插件有绑定执行，需要选择构建并安装模式: " + plugin.getArtifactId());
                }
                List<String> config = new ArrayList<>();
                for (String path : watched) config.add(q(path));
                List<String> probes = new ArrayList<>();
                for (String path : activation) probes.add(q(path));
                write("model.json", "{\"modules\":[" + String.join(",", modules) + "],\"descriptors\":[" + String.join(",", descriptors) + "],\"watched\":[" + String.join(",", config) + "],\"activation\":[" + String.join(",", probes) + "]}");
                // The native host checks fingerprints and validates every path.
                Path selection = exchange.resolve("selection.txt");
                long deadline = System.nanoTime() + 180_000_000_000L;
                while (!Files.exists(selection)) {
                    if (Files.exists(exchange.resolve("stop"))) throw new IOException("构建已取消");
                    if (System.nanoTime() > deadline) throw new IOException("等待构建指纹检查超时");
                    Thread.sleep(25);
                }
                Set<String> selected = new HashSet<>(Files.readAllLines(selection, StandardCharsets.UTF_8));
                List<MavenProject> execute = new ArrayList<>();
                for (MavenProject project : closure) {
                    if (selected.contains(project.getBasedir().toPath().toRealPath().toString())) execute.add(project);
                    else reusable.put(gav(project), project);
                }
                if (!execute.contains(application)) throw new IOException("构建计划缺少启动模块");
                System.out.println("[DevFlow] 构建 " + execute.size() + "/" + closure.size() + " 个模块；复用 " + reusable.size() + " 个模块。");
                for (MavenProject project : execute) System.out.println("[DevFlow] 构建模块: " + project.getArtifactId());
                // DefaultGraphBuilder rebuilds the graph from this selected list.
                session.setProjects(execute);
            } catch (Exception ex) {
                throw new MavenExecutionException("DevFlow 构建规划失败: " + ex.getMessage(), ex);
            }
        }

        @Override public void afterSessionEnd(MavenSession session) throws MavenExecutionException {
            if (session.getResult().hasExceptions() || application == null) return;
            try {
                // compile only resolves compile-scope artifacts. In particular,
                // getArtifacts() can omit runtime drivers entirely. Resolve the
                // runtime graph explicitly using Maven's own mediation, settings
                // and reactor reader, after selected module outputs are ready.
                var request = new DefaultDependencyResolutionRequest(application, session.getRepositorySession());
                request.setResolutionFilter(DependencyFilterUtils.classpathFilter("runtime"));
                var resolved = dependencies.resolve(request);
                Set<String> paths = new LinkedHashSet<>();
                paths.add(q(checked(application.getBuild().getOutputDirectory())));
                for (var dependency : resolved.getResolvedDependencies()) {
                    String scope = dependency.getScope();
                    if (!("compile".equals(scope) || "runtime".equals(scope))) continue;
                    org.apache.maven.artifact.Artifact artifact = RepositoryUtils.toArtifact(dependency.getArtifact());
                    MavenProject project = local.get(artifact.getGroupId() + ":" + artifact.getArtifactId() + ":" + artifact.getVersion());
                    if (!install && project != null) {
                        if (!"jar".equals(project.getPackaging()) || artifact.hasClassifier() || !"jar".equals(artifact.getType()))
                            throw new IOException("特殊工作区产物需要选择构建并安装模式: " + artifact);
                        paths.add(q(checked(project.getBuild().getOutputDirectory())));
                    } else {
                        if (artifact.getFile() == null || !artifact.getFile().exists()) throw new IOException("依赖产物缺失: " + artifact);
                        paths.add(q(artifact.getFile().getCanonicalPath()));
                    }
                }
                System.out.println("[DevFlow] 已解析运行时依赖，启动 classpath 共 " + paths.size() + " 项。");
                write("classpath.json", "[" + String.join(",", paths) + "]");
            } catch (Exception ex) {
                var failure = new MavenExecutionException("DevFlow 运行 classpath 生成失败: " + ex.getMessage(), ex);
                session.getResult().addException(failure);
                throw failure;
            }
        }
    }

    static String gav(MavenProject p) { return p.getGroupId() + ":" + p.getArtifactId() + ":" + p.getVersion(); }
    static String gav(Artifact a) { return a.getGroupId() + ":" + a.getArtifactId() + ":" + a.getVersion(); }
    static void watch(Set<String> paths, File file) throws IOException {
        if (file != null && !file.isDirectory()) paths.add(file.getCanonicalPath());
    }
    static void watchSource(Set<String> paths, org.apache.maven.model.InputLocation location) throws IOException {
        if (location != null && location.getSource() != null && location.getSource().getLocation() != null) {
            File file = new File(location.getSource().getLocation());
            if (file.isAbsolute() && file.isFile()) watch(paths, file);
        }
    }
    static void watchActivation(Set<String> paths, MavenProject project, String expression) throws IOException {
        if (expression == null) return;
        String value = expression.replace("${basedir}", project.getBasedir().getPath()).replace("${project.basedir}", project.getBasedir().getPath());
        for (String name : System.getProperties().stringPropertyNames()) value = value.replace("${" + name + "}", System.getProperty(name));
        for (Map.Entry<String, String> env : System.getenv().entrySet()) value = value.replace("${env." + env.getKey() + "}", env.getValue());
        if (value.contains("${")) throw new IOException("无法跟踪 Profile 文件激活条件: " + expression);
        File file = new File(value);
        paths.add((file.isAbsolute() ? file : new File(project.getBasedir(), value)).getCanonicalPath());
    }
    static void inside(Path path) throws IOException {
        if (!path.startsWith(boundary)) throw new IOException("模块或构建路径超出工作区目录: " + path);
    }
    static String checked(String value) throws IOException {
        Path path = new File(value).getCanonicalFile().toPath();
        inside(path);
        return path.toString();
    }
    static void write(String file, String value) throws IOException {
        Path temporary = exchange.resolve(file + ".tmp");
        Files.writeString(temporary, value, StandardCharsets.UTF_8);
        Files.move(temporary, exchange.resolve(file), StandardCopyOption.REPLACE_EXISTING);
    }
    static String q(String value) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            if (c == '"' || c == '\\') out.append('\\').append(c);
            else if (c < 32) out.append(String.format("\\u%04x", (int)c));
            else out.append(c);
        }
        return out.append('"').toString();
    }
}
