# Runtime

Keep UI, hooks, tests, and feature-scoped API here. Do not put Rust domain rules in React.

JavaLaunchDialog configures one literal JVM/application argument per line, Spring profiles, separate Maven profiles, on-demand workspace builds or install compatibility, and an optional force-build preference. Existing saved preferences default to workspace builds with caching enabled.
JavaConsole polls bounded stdout/stderr/build/Arthas output and supports stop/attach.
HTTP and MCP delegate all process operations to the same devflow-core JavaService.
Maven input/output checks and dependency build decisions live in the independent core runtime host. The UI displays its cache/build messages in the existing output panel; unchanged restarts skip Maven. No background auto-build is triggered by editing configuration.
Only Java execution is implemented; external processes cannot supply historical stdout.
