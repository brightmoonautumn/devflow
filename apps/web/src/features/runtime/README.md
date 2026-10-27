# Runtime

Keep UI, hooks, tests, and feature-scoped API here. Do not put Rust domain rules in React.

JavaLaunchDialog configures one literal JVM/application argument per line, Spring profiles, separate Maven profiles, on-demand workspace builds or install compatibility, and an optional force-build preference. Existing saved preferences default to workspace builds with caching enabled.

Local runtime dependencies are backend-persisted in `.devflow/local-java-dependencies.json`, outside managed repositories. `java_options.localDependencies` returns the effective list; `java_execute` with `configure_dependencies` and `dependencies: [{ value, enabled }]` saves a per-service override (including an empty override). Java start/restart automatically apply that list in the shared engine. Maven coordinates resolve runtime transitives after the ordinary build; absolute JAR paths must remain within the project's workspace build boundary. Maven artifact version conflicts stop launch; arbitrary local JAR contents are not analyzed for duplicate classes. No dependency is injected into project compilation or packaging.
JavaConsole polls bounded stdout/stderr/build/Arthas output and supports stop/attach.
HTTP and MCP delegate all process operations to the same devflow-core JavaService.
Maven input/output checks and dependency build decisions live in the independent core runtime host. The UI displays its cache/build messages in the existing output panel; unchanged restarts skip Maven. No background auto-build is triggered by editing configuration.
Only Java execution is implemented; external processes cannot supply historical stdout.

RuntimeConsole copies a short service handoff for any Agent; the core diagnostics response keeps its redacted log excerpt separately. Java services expose DiagnosisDialog for problem intake, persisted session history, bounded evidence, cancellation and conclusions. The Agent continues through shared diagnosis_get/diagnosis_execute MCP operations; creating a record does not launch AI. See docs/features/workspace.md for workflow, limits and pending validation.
