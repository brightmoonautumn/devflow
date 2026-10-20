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
`FileEditor` routes Markdown to a Vditor document editor behind the shared editor
boundary. Document, source and comparison modes use the same draft and file APIs;
Mermaid/KaTeX/highlight assets are served locally. See `components/editor/README.md`.
HTTP and MCP use the same bounded ripgrep service in `devflow-core`; see
`docs/features/workspace.md` for configuration, limits and validation status.

### Git 入口

活动栏新增 Git，WorkspaceTree 挂载相邻 `features/git/GitPanel`，FileWorkbench 承载只读 Diff 标签。业务 API 和样式归属 `features/git`，CodeMirror 对比实现封装在 `components/editor/GitDiffEditor`。父子仓库分别选择与提交，行为说明见 `docs/features/workspace-git.md`。

`workspaceViewState.ts` persists view metadata per workspace: ordinary file tabs,
active file, source cursor, expanded tree nodes and sidebar preferences. Files are
read lazily through the existing authorized API after restoration. Local sessions
use localStorage; readonly shares use sessionStorage. Draft content, diff bodies
and credentials are not part of this store. Markdown stores only its mode under
an editor-specific key. Readonly file polling checks the active file every 15s,
pauses in hidden pages and backs off to 60s on failure without remounting editors.

Git History 为 FileWorkbench 中独立的工具标签，不占文件标签配额；GitPanel 经 onOpenHistory 打开，实际页面/图/样式位于 features/git。历史 Diff 使用含 commit/parent 的独立标签键与 historical 标志，公共编辑器接收中性的左右标题；历史只读禁止还原。

Git 展示读取改为核心内存快照与按仓库后台刷新；History 使用独立仓库目录接口。HTTP/MCP/只读分享复用相同规则，本机与分享共用 32 MiB 历史内容 LRU 和 16 MiB 状态内容估算预算，授权缓存键隔离。详细行为与未运行的验证场景见 `docs/features/workspace-git.md` 的“Git 扫描与内存缓存”。
