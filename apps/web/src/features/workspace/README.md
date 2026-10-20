# Workspace

Keep UI, hooks, tests, and feature-scoped API here. Do not put Rust domain rules in React.

`WorkspaceSearch` provides a separate Search view selected from the left activity
bar, alongside Explorer. Both views share the resizable sidebar, preserve their
state when switched, and keep file results grouped with match previews. A single
query searches names and content. Replacement previews and commits share the
Rust core through HTTP/MCP; editor events block dirty drafts and refresh clean tabs.
HTTP and MCP use the same bounded ripgrep service in `devflow-core`; see
`docs/features/workspace.md` for configuration, limits and validation status.
