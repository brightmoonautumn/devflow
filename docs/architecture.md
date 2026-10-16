# Architecture

```text
Codex --> MCP (/mcp) ---+
                          |
React --> HTTP (/api) ----+--> devflow-core --> local Maven/Node projects
                          |                   JVM/Arthas/HTTP QA (future)
                  Axum adapters
```

## Boundaries
- The same core service methods are called from both HTTP and MCP adapters.
- `DEVFLOW_WORKSPACE` is the only allowed initial scan root; canonicalize and verify every file access before expanding filesystem APIs.
- Managed projects can be Maven (Spring Boot) or Node (React/Vue/Vite/Next.js); use descriptors and scripts to drive runtime discovery rather than Rust-centric assumptions.
- Rust serves the built frontend from `apps/web/dist` during local compiled runs (relative to current working directory).

## Roadmap
1. Accurate Maven modules and Node package manager/script parsing (respect workspace boundaries).
2. Process supervisor: JDK detection, Maven builds, Spring Boot DEBUG launches, Node dev server scripts, structured logs.
3. PID/JVM discovery, bounded Arthas attach/trace/stop.
4. Context Picker and snapshot IDs exposed to MCP.
5. Existing JS HTTP QA scenario migration, assertions, reports.
6. Protocol-complete MCP via rmcp SDK, HTTP sessions/auth and Origin/DNS-rebinding defenses.

## Build strategy
Run backend and Vite separately in development. Build React before starting backend to serve it at port 31080. For eventual one-file release, embed static assets and include a proper installation/upgrade lifecycle.
