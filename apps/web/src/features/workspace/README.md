# Workspace

Keep UI, hooks, tests, and feature-scoped API here. Do not put Rust domain rules in React.

`WorkspaceSearch` provides a separate Search view selected from the left activity
bar, alongside Explorer. Both views share the resizable sidebar, preserve their
state when switched, and keep file results grouped with match previews. A single
query searches names and content. Replacement previews and commits share the
Rust core through HTTP/MCP; editor events block dirty drafts and refresh clean tabs.
`FileWorkbench` autosaves each edited tab after 700ms of inactivity and shows the
last successful save time. Explorer file menus duplicate disk files as uniquely
named siblings or confirm permanent deletion. Delete events pause the open editor
and close its tab only after success; tree events refresh the containing folder.
Both file operations use the authenticated, revision-checked Rust core via HTTP/MCP.
The shared editor loads CodeMirror language parsers by filename, including YAML,
Python, SQL, shell, PowerShell, Dockerfile, TOML, properties, Go and Rust. A local
log tokenizer highlights timestamps, severity levels and exception stacks.
HTTP and MCP use the same bounded ripgrep service in `devflow-core`; see
`docs/features/workspace.md` for configuration, limits and validation status.

### Git 入口

活动栏新增 Git，WorkspaceTree 挂载相邻 `features/git/GitPanel`，FileWorkbench 承载只读 Diff 标签。业务 API 和样式归属 `features/git`，CodeMirror 对比实现封装在 `components/editor/GitDiffEditor`。父子仓库分别选择与提交，行为说明见 `docs/features/workspace-git.md`。
