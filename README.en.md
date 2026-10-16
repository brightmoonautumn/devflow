# DevFlow Starter

**[简体中文](README.md) | English**

Local-first development control center. **Rust + Tokio + Axum + React + TypeScript + MCP** in one repository.

> This is a runnable **starter scaffold**, not a finished IDE. Currently functional: health API, shallow Maven/Node project discovery, read-only `workspace_list` MCP tool, and React shell. Runtime, Arthas, context basket, and QA are extension points for Codex.

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
```

The script resolves the repository root automatically, waits for the API to become ready,
and stops its backend process tree when the frontend exits. Backend logs are saved in
`.devflow/`. Without `-Workspace` or `DEVFLOW_WORKSPACE`, the shell starts with an empty
project list. `.env.example` is a reference; environment files are not loaded automatically.

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
Invoke-RestMethod http://127.0.0.1:31080/api/projects
cargo test --workspace
pnpm web:check
```

### Codex MCP (experimental starter)

```bash
codex mcp add devflow --url http://127.0.0.1:31080/mcp
```

`/mcp` exposes a minimal stateless JSON-RPC subset (`initialize`, `ping`, `tools/list`, `tools/call`) with one **read-only** `workspace_list` tool. Some MCP clients may require fuller session/notification support; upgrade to the maintained Rust MCP SDK before production. Do not expose the port to the public network. The web dev proxy is intended for browser API requests; Codex talks to Axum directly.

## Supported workspaces
`DEVFLOW_WORKSPACE` is the parent folder, e.g. `D:\code`, containing Maven `pom.xml` and/or Node `package.json` projects. Discovery currently checks the root and direct subfolders only. It does not yet parse multi-module Maven metadata or npm/pnpm workspace graphs. This is intentionally safe and predictable for v0.1.

## Repository organization

- `crates/devflow-core/src/<feature>`: business/domain operations
- `crates/devflow/src/http`: HTTP adapters
- `crates/devflow/src/mcp`: MCP adapter (experimental)
- `apps/web/src/features/<feature>`: React feature implementations
- `apps/web/src/components`: shared UI (add as actual need arises)
- `docs/features`: design specifications

See [AGENTS.md](AGENTS.md) and [docs/architecture.md](docs/architecture.md).
