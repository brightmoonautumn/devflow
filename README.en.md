# DevFlow Starter

**[简体中文](README.md) | English**

Local-first development control center. **Rust + Tokio + Axum + React + TypeScript + MCP** in one repository.

> This is a workspace-focused starter, not a finished IDE. Workspace management, Maven/Node discovery, metrics, managed Java launch/stop, live output and official Arthas attachment share one core engine across Web and MCP. Node launch/stop remains unimplemented. See [workspace documentation](docs/features/workspace.md).

## Prerequisites
- Rust stable toolchain (rustup; supports edition 2024)
- Node.js 22+ and pnpm 11.25.0 (`corepack enable` or `npm i -g pnpm@11.25.0`)
- JDK and Maven for future Spring Boot management; not required to run this starter

## Quick start (two terminals)

On Windows, you can start both services with one command after installing dependencies:

```powershell
pnpm install --frozen-lockfile
.\scripts\dev.ps1
# Optional: .\scripts\dev.ps1 -Workspace 'D:\code'
# Cross-directory workspace:
# .\scripts\dev.ps1 -Workspace 'D:\WorkSpace\main-system.code-workspace' -AllowedRoots @('D:\WorkSpace', 'D:\CompanyProject', 'D:\AiFlow')
```

The script resolves the repository root automatically, waits for the API to become ready,
and stops its backend process tree when the frontend exits. Backend logs are saved in
`.devflow/`. Without `-Workspace` or `DEVFLOW_WORKSPACE`, the shell starts with an empty
project list. Configure `-AllowedRoots` / `DEVFLOW_ALLOWED_ROOTS` to open workspaces in the UI; referenced project folders must also be allowlisted. `.env.example` is a reference; environment files are not loaded automatically.

Alternatively, use two terminals:

```powershell
# terminal 1: from repository root
$env:DEVFLOW_WORKSPACE='D:\code'  # change to an existing directory containing your projects
cargo run -p devflow

# terminal 2
pnpm install
pnpm web:dev
```

Open **http://127.0.0.1:5173** in the Codex embedded browser or any browser.

Production-style local run:

```powershell
pnpm install
pnpm web:build
cargo run -p devflow
```

Open **http://127.0.0.1:31080** from the repository root (frontend bundled from `apps/web/dist` if present).

### Verify

```powershell
Invoke-RestMethod http://127.0.0.1:31080/api/health
$session = Invoke-RestMethod http://127.0.0.1:31080/api/session -Headers @{'X-DevFlow-Client'='web'}
Invoke-RestMethod http://127.0.0.1:31080/api/projects -Headers @{Authorization="Bearer $($session.token)"}
cargo test --workspace
pnpm web:check
```

### Codex MCP (experimental starter)

```bash
codex mcp add devflow --url http://127.0.0.1:31080/mcp --bearer-token-env-var DEVFLOW_API_TOKEN
```

Set the same `DEVFLOW_API_TOKEN` in the backend and MCP client processes. `/mcp` exposes a minimal JSON-RPC subset (`initialize`, `ping`, `tools/list`, `tools/call`) with workspace read/open/create/add/save tools and read-only process metrics. Some MCP clients may require fuller session/notification support; upgrade to a maintained Rust MCP SDK before production. Do not expose the port to the public network. See the workspace documentation for tool arguments.

## Supported workspaces
`DEVFLOW_WORKSPACE` accepts a directory or a VS Code `.code-workspace` file. Web and MCP support open/create/add-folder/save/save-as operations, JSONC comments and trailing commas, relative paths, display names, and local file URIs while retaining configuration fields. Remote URIs are unsupported. Runnable application discovery follows declared Maven modules and parses real Java entry points, excluding aggregators, libraries, and test sources; Node packages require dev/start/serve scripts. It does not run Maven, scan dependency archives, or provide complete effective-POM/npm-workspace resolution.

## Repository organization

- `crates/devflow-core/src/<feature>`: business/domain operations
- `crates/devflow/src/http`: HTTP adapters
- `crates/devflow/src/mcp`: MCP adapter (experimental)
- `apps/web/src/features/<feature>`: React feature implementations
- `apps/web/src/components`: shared UI (add as actual need arises)
- `docs/features`: design specifications

See [AGENTS.md](AGENTS.md) and [docs/architecture.md](docs/architecture.md).
