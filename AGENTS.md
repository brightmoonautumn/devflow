# DevFlow — Codex Development Instructions

## Vision
Local-first AI development control center for Java/Spring Boot Maven and Node.js frontend workspaces. Codex remains the coding interface. DevFlow provides process management, context picking, JVM diagnosis, and QA through Web and MCP.

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
- `workspace`: import/detect local projects, Maven pom.xml, Node package.json; later parse modules and package managers.
- `runtime`: Maven/Spring Boot debug launches and Node dev scripts, tracked process trees and structured logs. JVM debug and Arthas attachment are independent operations.
- `diagnose`: Arthas and bounded log collection; integrate safely with managed JVMs.
- `explorer`: fast file/symbol search and stable context selection IDs shared by Web & MCP.
- `qa`: HTTP scenarios, variables, assertions, reports; migrate legacy JS gradually.

## Coding agreements
- Avoid new crates until necessary. Favor feature-local code; extract shared only after actual reuse.
- Tests accompany new runtime logic; test Windows paths and process cleanup.
- Keep handlers thin, normalize errors, maintain backward-compatible DTOs.
- UI design: low visual noise, keyboard search, minimal card surfaces, no Monaco dependency.
- Use `cargo fmt --all`, `cargo test --workspace`, `pnpm web:check`, `pnpm web:build`.
- For a feature, update matching documentation in `docs/features`.

## MVP status
Working starter includes HTTP health/project discovery, basic read-only MCP `workspace_list`, React shell and proxy. Other feature directories are intentionally scaffolds, **not finished implementations**. MCP starter is only a minimal JSON-RPC foundation; add protocol tests and secure sessions before production deployment.
