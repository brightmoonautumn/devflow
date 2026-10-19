# DevFlow — Codex Development Instructions

## Vision
Local-first development control center for Java/Spring Boot Maven and Node.js frontend workspaces. The current scope is workspace management, project launches, and Arthas attachment.

## Core principle
DevFlow has two native entry points: Web UI for humans and MCP for AI. Both MUST use the same core engine and share business rules, state, and permission checks. Entry-point layers handle interaction and protocol adaptation only; never duplicate business logic. Plan both UI and MCP access for every new business capability.

## Architecture
- Monorepo; two Rust crates: `devflow` (Axum HTTP / MCP adapters), `devflow-core` (domain logic).
- React TypeScript in `apps/web`; feature-first folders.
- HTTP and MCP adapters MUST call the same `devflow-core` services; do not duplicate domain logic.
- Workspaces contain arbitrary user projects, typically Maven multi-module Spring Boot and Node.js frontend (npm/pnpm/yarn). **Never assume managed projects are Rust projects.**
- Never recursively scan node_modules/target/.git/build/dist; prefer scoped/lazy discovery.
- Never expose arbitrary shell execution or filesystem traversal via HTTP/MCP. Bind to loopback, use allowlisted workspace roots, validate canonical paths, and implement authentication before enabling destructive operations.
- Default UI is compact and usable at 400–550px width in Codex browser split view.
- No embedded AI chat and no source editor in MVP.

## Features
- Only the workspace feature is currently planned: open/create VS Code-compatible `.code-workspace` files, add folders, open folders directly, launch Java/Node projects, and attach Arthas independently of startup or JVM debugging.
- Runtime and diagnosis code may remain in their existing internal modules; their current requirements belong to `docs/features/workspace.md`. Other scaffold directories do not imply planned or implemented features.

## Coding agreements
- Avoid new crates until necessary. Favor feature-local code; extract shared only after actual reuse.
- Tests accompany new runtime logic; test Windows paths and process cleanup.
- Keep handlers thin, normalize errors, maintain backward-compatible DTOs.
- UI design: low visual noise, keyboard search, minimal card surfaces, no Monaco dependency.
- Read and write text files as UTF-8, including Chinese content.
- Do not compile or run build/test commands unless the user explicitly requests it. When authorized, use the applicable checks: `cargo fmt --all`, `cargo test --workspace`, `pnpm web:check`, `pnpm web:build`.
- For a feature, update matching documentation in `docs/features`.

## MVP status
Implemented code includes VS Code-compatible workspace management, project discovery, process metrics, managed Java/Node launch/stop, bounded stdout/stderr output, and official Arthas attachment shared by HTTP and MCP. Runtime mutations require authentication, active workspace membership and revision checks. Node aggregate script services share the package root but have independent runtime keys, output and process-tree cleanup. MCP starter is still a minimal JSON-RPC foundation; add full protocol compliance before production deployment. See `docs/features/workspace.md` for validation status, configuration, and acceptance criteria.
