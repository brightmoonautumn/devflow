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
`FileWorkbench.onActiveFileChange` reports the actual active tab, including restored
tabs and tab switches. Explorer's locate button reveals that file under the most
specific available workspace root, expands only its ancestor chain through the
existing children API, then focuses and scrolls to the selected row. It supports
compact folders and Windows path casing without changing tabs, drafts or files.
No active file or a file outside the active roots disables the button.
The shared editor loads CodeMirror language parsers by filename, including YAML,
Python, SQL, shell, PowerShell, Dockerfile, TOML, properties, Go and Rust. A local
log tokenizer highlights timestamps, severity levels and exception stacks.
`FileEditor` routes Markdown to a Vditor document editor behind the shared editor
boundary. Document, source and comparison modes use the same draft and file APIs;
Mermaid/KaTeX/highlight assets are served locally. See `components/editor/README.md`.
HTTP and MCP use the same bounded ripgrep service in `devflow-core`; see
`docs/features/workspace.md` for configuration, limits and validation status.

`workspaceViewState.ts` persists view metadata per workspace: ordinary file tabs,
active file, source cursor, expanded tree nodes and sidebar preferences. Files are
read lazily through the existing authorized API after restoration. Local sessions
use localStorage; readonly shares use sessionStorage. Draft content and credentials
are not part of this store. Markdown stores only its mode under an editor-specific
key. Readonly file polling checks the active file every 15s, pauses in hidden pages
and backs off to 60s on failure without remounting editors.
