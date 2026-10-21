# 工作区

## 功能概述

当前仅建设工作区：管理 VS Code 兼容的工作区文件和文件夹，搜索文件名与文件内容，启动 Java/Spring Boot 与 Node 项目，并支持 Arthas 附加。所有业务能力由同一核心引擎提供，Web UI 和 MCP 共用业务规则、状态及权限校验。

## Git 管理

新增活动栏 Git 面板：IDEA 风格的 Changes / Unversioned Files、Add to VCS、Rollback、选择文件提交与只读 Diff。父仓库子模块指针与子仓库文件独立分组、独立提交。Web 与 MCP 共用核心服务；具体行为、限制及待验证项见 [工作区 Git 管理](workspace-git.md)。本轮未编译或运行测试。

## 文件搜索

- 左侧活动栏分别提供“资源管理器”和“搜索”，共用一个可拖拽、键盘调宽且记忆宽度的侧栏，搜索框不嵌入目录树。再次点击当前活动入口可收起；切换/收起保留目录展开状态、搜索词和结果。一个输入框同时搜索文件名/相对路径与文件内容，不再显示模式页签；`Ctrl/Cmd+P` 和 `Ctrl/Cmd+Shift+F` 均打开统一搜索。名称匹配采用路径片段，整词/正则选项用于内容匹配。输入防抖 350ms，Enter 或标题栏刷新按钮可重新搜索；文件名按名称或相对路径片段匹配，默认忽略大小写；内容支持大小写、整词、正则切换。
- 内容结果按文件分组，文件行显示匹配行数，可展开/折叠或一键全部折叠；组内每个匹配行一条，显示行号、预览与准确的匹配高亮，当前选中结果有边框。预览最多 300 字符，行号和 UTF-16 列号从 1 开始，多根工作区另显示来源。单击结果预览文件并定位首个匹配，双击固定标签。键盘上下/Home/End 移动结果焦点，左右展开/折叠文件；支持清空结果、Esc/停止按钮取消等待。搜索磁盘内容，未保存草稿不参与搜索；草稿与磁盘不同会影响跳转位置。
- 参考 VS Code 的 [文件搜索 provider](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/services/search/node/ripgrepFileSearch.ts) 与 [内容搜索 engine](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/services/search/node/ripgrepTextSearchEngine.ts)，复用 ripgrep 命令行工具；没有复制 VS Code 源码，搜索不引入新 Rust crate；替换捕获组则复用锁文件中已有的 regex。文件名使用 `rg --files --null`，内容使用 `rg --json`，通过参数数组启动，无 shell 拼接，不接受用户传入可执行文件或目录。
- 搜索组件随发布包提供，普通用户无需安装 rg、修改 PATH 或配置环境变量。根项目锁定官方 `@vscode/ripgrep@1.18.0`，pnpm 锁文件固定其平台可选包及 integrity；`postinstall` 自动执行 `scripts/prepare-ripgrep.cjs`，准备当前平台二进制到 `tools/ripgrep/`，复制包许可证与 upstream MIT 许可证，记录版本/架构/SHA-256 清单。同一二进制不重复覆盖，避免 Windows 下正在搜索时的文件锁冲突。该目录为生成产物，不进 Git。
- `scripts/dev.ps1`、`DevFlow-Restart.ps1`、`scripts/package.ps1` 会显式准备组件，其中源码重启在结束后台前完成准备，失败不先停止后台。发布脚本复制整个 `tools/ripgrep`；发布环境运行搜索不执行 Node，也不联网。源码依赖安装若跳过脚本，可执行 `pnpm tools:prepare`，平台可选依赖必须安装。
- 核心定位顺序为高级 `DEVFLOW_RG_PATH` 绝对路径覆盖、可执行文件旁的 `tools/ripgrep/rg[.exe]`、默认源码 `target/debug` 或 `target/release` 对应仓库的 `tools/ripgrep/`、最后才兼容 PATH。不按用户项目或当前工作目录查找组件，不自动借用 VS Code 安装目录。缺失时返回 `search_unavailable` 并提示完整发布包/源码依赖准备；旧后端缺少接口时提示更新重启。自定义 Cargo target 目录可使用高级覆盖选项。
- 默认遵循 `.gitignore` / `.ignore`（非 Git 项目也生效），包含未被忽略的隐藏文件；可展开“包含文件 / 排除文件”，各输入一个 glob，例如 `**/*.java`、`**/test/**`，留空表示不额外限制，使用 ripgrep 原生 glob 语义（显式包含可覆盖 ignore）。强制排除规则始终最后应用，不跟随符号链接，不递归扫描任意层级的 `node_modules / target / .git / build / dist`，目录名不区分大小写。内容文件最多 1 MiB，按 UTF-8 搜索，沿用 ripgrep 的二进制检测；不提供多行搜索或编码选择。
- 总预算为 5 秒、最多 200 条结果；名称候选扫描额外限制 20000 个候选文件。stdout 单条记录最多 128 KiB，使用有界通道，stderr 仅保留 8 KiB；达到预算或输出记录过长时返回 `truncated` 与说明。超时、结果截断及解析错误均终止并回收 rg 子进程。前端停止/切换查询会中止旧 HTTP 请求并丢弃旧响应，服务端已启动的搜索最迟在自身预算结束后回收。
- `POST /api/workspace/search` 与 MCP `workspace_search` 共用 `WorkspaceService::search`。请求为 `{ query, mode?: "both" | "files" | "content", expectedRevision, caseSensitive?: false, wholeWord?: false, regex?: false, includeGlob?: "", excludeGlob?: "" }`，默认 both；保留 files/content 以兼容旧客户端。files 模式不接受整词/正则，glob 各最多 1024 字节，禁止换行、NUL 或开头的 `!`。需要有效令牌，只搜索当前工作区内已授权且可用的根目录，校验结果的 canonical 路径归属并对重叠根去重。开始及结束检查工作区版本，版本改变返回 `conflict`；不允许调用方指定任意搜索根。
- 返回 `{ revision, matches: [{ path, root, relativePath, line, column, preview, previewMatches: [{ start, end }] }], truncated, warnings }`。高亮范围为预览内的 UTF-16 起止偏移（end 不包含），由同一 rg 匹配结果生成，避免前端另用 JS 正则产生差异；单条预览最多保留 200 个高亮范围。文件名结果的行、列、预览为 null，范围为空数组。空白词、换行、NUL、超过 1024 UTF-8 字节的搜索词被拒绝，正则错误返回 `search_failed`。
- 搜索基础实现阶段只修改源码、生成依赖锁文件（`--lockfile-only --ignore-scripts`）、补充测试源码和静态差异检查；随后用户已授权安装、编译和重启，见下方最新替换验证状态。普通测试源码覆盖内置路径定位/覆盖/无关目录不向上查找，以及鉴权、非法参数、版本、无工作区和禁止任意路径；rg 集成测试默认忽略，另覆盖 glob、强制目录排除、UTF-16 高亮范围、中文路径、BOM/CRLF、二进制/大文件、正则/整词、重叠根及截断后续查询。依赖准备和实际文件搜索已在前次授权运行中通过；完整发布包离线搜索、当前统一搜索/替换及 400–550px 布局仍待运行验收。

## 搜索替换

- 搜索与替换输入均采用单层外框，聚焦只改变外框边色，内层 input 不再绘制 outline/box-shadow。点击搜索框左侧箭头展开“替换”行：右侧 `AB` 开关保留基本大小写形式，旁边按钮预览全部替换；结果文件行支持文件内替换，内容行支持替换该行首个匹配（用行号和 UTF-16 列号精确定位）。文件名匹配只用于打开文件，替换不重命名文件。
- 单处/文件内替换点击后，核心先生成快照计划再立即提交。全部替换先显示文件数、真实替换处数及各文件的首处修改前后片段，确认后提交。空替换文本表示删除。按文件分组的“行数”不等于真实处数，同一行多处会在文件内/全部替换时全部处理。
- 参考 [VS Code ReplaceService](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/search/browser/replaceService.ts) 的按匹配范围编辑和替换预览思路，业务逻辑留在 `devflow-core`，没有复制 VS Code 源码。字面量模式将替换文本原样使用；正则模式支持 `$&`、`$0`–`$99`、`$$`、`\n`、`\t`、`\\`，不存在的组号保留为字面量，不实现 VS Code 全部替换语法。`AB` 支持全大写、全小写、首字母大写的基本转换，不模拟完整分词/复合标识符规则。
- 替换仅处理已有 UTF-8 普通文件，保留 BOM 与 LF/CRLF；拒绝独立 CR 换行、二进制控制字符、只读文件、当前工作区配置和越界路径。单文件替换后最多 1 MiB；输入替换文本最多 16 KiB；单计划保存原文与新文总计最多 8 MiB、最多 20000 处。全部替换必须得到完整、无警告的内容搜索结果，截断时要求缩小范围。单处/文件内替换直接校验并读取选中的整个授权文件，不受工作区结果列表截断影响；此时显式文件路径决定范围。
- `POST /api/workspace/replace/preview` 与 MCP `workspace_replace_preview` 接收 `{ search: { query, expectedRevision, caseSensitive?, wholeWord?, regex?, includeGlob?, excludeGlob? }, replacement, preserveCase?, path?, line?, column? }`，只读磁盘，返回 `{ planId, revision, replacements, files: [{ path, replacements, before, after }] }`，前后文为最多 240 字符的示例，不是整文件差异。省略 path 表示完整搜索范围，传 path 表示单文件，另传 line/column 表示单处；line/column 从 1 开始。不要用前后片段充当写入原文。
- `POST /api/workspace/replace/apply` 与 MCP `workspace_replace_apply` 接收 `{ planId, expectedRevision }`。服务端仅缓存最近一个计划，5 分钟过期，绑定工作区版本，提交后消费计划。提交先对全部文件检查权限、只读和精确磁盘原文，再复用编辑器的冲突检测写入。若后续 I/O 失败，立即停止并返回 `{ files, replacements, failure }`，files 为已经写入的真实清单；多文件替换不是文件系统事务，不声明整体回滚或提供跨文件撤销。
- Web 提交前检查目标编辑器标签，遇到未保存修改/读取中/保存中会阻止替换。成功后自动刷新未修改的已打开标签；提交期间新产生的草稿保留并显示磁盘已修改提示，后续保存仍由精确原文校验阻止覆盖。MCP 同样有磁盘冲突保护，但不感知浏览器内存中的草稿。
- 前一轮经用户授权已安装并准备 rg、编译前后端、重启后台，并通过实际文件搜索请求。本次统一搜索/样式/替换修订只做源码修改、TS/TSX 语法解析与差异检查，未再编译、运行测试或重启。新增测试源码覆盖字面量删除、正则捕获/转义/不存在组、大小写、UTF-16 单处定位、BOM/CRLF、零宽匹配和末尾换行、整批冲突预检、计划过期/消费/鉴权、越界及工作区关闭；依赖 rg 的预览/提交与统一搜索测试默认忽略，待显式运行。实际 UI 及替换链路仍待运行验收。

## 工作区管理

### 本地浏览器启动器（Windows）

- 源码重启脚本兼容 `pnpm.exe`、`pnpm.cmd` 和 `pnpm.ps1`；没有 pnpm 启动文件时自动使用 `corepack pnpm`，遵循仓库 `packageManager` 版本，无须先全局启用 pnpm shim。通过所选工具直接执行 `--filter @devflow/web run build`，避免根目录 `web:build` 再次调用缺失的 `pnpm` 命令。工具缺失时在停止后台之前给出明确提示；首次由 Corepack 获取 pnpm 可能需要网络。

- 源码目录提供 `DevFlow-Restart.cmd`，双击后调用根目录 `DevFlow-Restart.ps1`：检查 Cargo / pnpm，按可执行文件绝对路径识别并结束本项目目录下已有的 `devflow.exe`（包括 31081 开发实例），编译开发版后端与前端，再使用 `scripts/launch.ps1 -ExecutablePath` 明确启动本次生成的 `target/debug/devflow.exe`，固定打开 31080。只结束后台自身，不使用进程树终止命令；不结束其他目录中的同名程序，端口仍占用则失败。重复运行由互斥锁拦截，构建失败即停止且窗口保留错误，不回退到旧程序。脚本复用普通启动器的授权配置及日志，不自动安装依赖。新增脚本本轮仅做静态语法与差异检查，未实际执行停止进程、编译或重启。

- 根目录 `DevFlow.vbs` 隐藏启动 Windows PowerShell 中的 `scripts/launch.ps1`，启动后台并等待健康检查及网页就绪后打开默认浏览器。禁用 VBScript 时可用 `DevFlow.cmd` 调用相同流程。普通启动不编译、不安装依赖、不依赖 Vite，使用后端提供的 `apps/web/dist`。API/MCP 路由继续鉴权，静态页面在取得会话前即可加载。
- 使用本机命名互斥锁串行处理重复启动，健康后台直接复用；非 DevFlow 程序占用固定回环端口 31080 时明确失败，不杀进程。并发启动由绑定端口的结果及健康检查兜底。启动等待最多约 30 秒，单次请求超时可能令总等待略长；失败时显示日志位置，不强制清理业务进程。
- 首次未配置授权位置时弹出本机文件夹选择器，取消则不启动。仅将用户所选目录及显式授权目录交给现有核心引擎校验，不授予整机文件访问权限。`.devflow/launcher.json` 保存初始 `workspace`、`allowedRoots`，不包含令牌；显式启动参数优先于环境变量，环境变量优先于保存配置。复用已有后台时不会切换该后台的工作区或授权目录。
- 后台作为独立进程隐藏运行，浏览器关闭或启动器退出均不结束后台。当前只实现打开与复用，不包含后台重启、托管状态迁移、崩溃守护。工作区位置通过核心层最近记录自动恢复。原有开发脚本退出时的清理行为不变。
- “最近打开”是可见且持久的历史列表：左上角工作区菜单直接展示最近 5 个并可点击重开，“查看全部最近打开…”及“打开工作区”窗口展示全部最多 20 个位置，含工作区文件和文件夹的名称、完整路径与类型。最近使用排在前面，重复打开只前移、不重复记录，失效或链接目标改变的位置标记不可用；关闭当前工作区仅记下当前已关闭，历史列表仍保留。旧版本只有 `path/roots` 的单位置记录会迁入历史；旧版已经清掉的记录无法补回。
- `workspace_catalog` / `GET /api/workspace/catalog` 增加 `recentWorkspaces`，读取历史不扩展授权。`POST /api/workspace` 的 `open_recent` 操作和 MCP `workspace_open_recent` 只允许重开核心持久历史中的路径，恢复该项当时的精确工作区文件及项目根授权；仍校验身份、当前工作区版本、未保存更改与规范化路径。任意客户端路径不能借此授权。
- 本轮增加历史关闭后保留、最近排序、去重、20 项上限、旧记录迁移、失效项标记、历史重开与授权隔离的测试代码；未执行构建或测试。前端遇到未返回最近列表的旧后台会提示更新重启。
- 核心引擎将最近成功打开的 `.code-workspace` 或直接打开的文件夹、该工作区可访问的根目录写入 `.devflow/recent-workspace.json`（UTF-8、临时文件同步后替换）。Web 与 MCP 的工作区变更共用这一逻辑，打开、新建、另存后更新，关闭只将当前位置写为空并保留历史。仅保存位置与范围，不自动保存工作区脏配置或编辑器草稿；其他已浏览/选择的目录不会一并恢复授权。编辑器中该记录只读，由核心维护。
- 普通 `DevFlow.vbs` / 一键更新启动优先恢复最近工作区，覆盖旧 `launcher.json` 中第一次选择的位置；启动器不把旧位置再次传作初始工作区。显式 `-Workspace` 或外部 `DEVFLOW_WORKSPACE` 指定的位置优先，后台通过 `DEVFLOW_RESTORE_RECENT=1` 区分启动器的恢复意图。直接启动后端、没有指定初始工作区时也会恢复最近记录，按后端工作目录分别保存状态。
- 恢复时重新校验规范化路径、已记住的根目录与链接目标；工作区文件只授予精确文件权限，不顺带开放父目录，新加入的越界配置根不会自动扩权。位置失效或记录损坏时进入空工作区并显示原因，不回退旧盘符目录、不导致后台退出；记忆写入失败不撤销已成功的工作区操作，返回警告。目录根名称统一去除 Windows `\\?\` 前缀。
- 本轮补充恢复最近中文文件夹、工作区文件及精确授权、显式初始位置优先、关闭后保持关闭、记录损坏及目录删除的测试代码；按约定未运行编译、构建或测试。首次升级没有历史记录时，需要成功打开一次所需工作区，后续重启即可自动恢复。
- `scripts/package.ps1` 由开发者显式执行构建，输出新的便携发布目录，包含 `DevFlow.vbs`、`DevFlow.cmd`、`scripts/launch.ps1`、`devflow.exe` 和完整前端静态资源；不覆盖既有输出目录，不携带本机配置、授权目录、令牌或日志。使用者解压到可写目录后直接启动。
- 新增 `scripts/verify-launcher.ps1`，覆盖中文与空格路径、授权根去重、配置优先级、配置恢复和程序查找规则。按用户约定本轮未运行编译、打包或测试命令。待验收：冷启动、重复双击、端口冲突、缺少静态资源、后台异常退出、文件夹选择取消及关闭浏览器后后台继续运行。

- 字体统一使用 Segoe UI 与微软雅黑中文后备字体，等宽区使用 Consolas 与同一中文后备字体。服务名称 14px、常用界面和树节点 13px、状态/类型/操作与日志 12px、辅助说明最低 11px，使用共享字号令牌，窄屏也不退回 9–10px 小字。浅色主题正文、次级和辅助文字分别为 `#24292f`、`#4b5563`、`#636b75`，卡片组件复用相同灰阶，消除双套文字令牌导致的浅灰字；保留深色配色和现有布局。

- 打开已有 `.code-workspace` 文件，正确显示其中的文件夹。
- 创建并保存与 VS Code 相同格式的工作区文件，支持添加文件夹。
- 支持直接打开文件夹，无须先创建工作区文件。
- 相对路径以工作区文件所在目录为基准解析；兼容绝对路径、中文、Windows 路径及文件夹显示名称。
- 兼容 VS Code 工作区 JSONC 格式，包括注释和尾随逗号；保存时保留已有 `settings` 等未知配置字段。
- 文件夹不存在或无法访问时明确显示异常，不静默忽略。
- 工作区文件夹在左侧资源管理器中以树展示；侧栏顶部直接显示工作区名称和未保存状态，名称采用 14px、600 字重与随明暗主题切换的强调色，字号与顶部 DevFlow 名称一致，高于下方 13px 文件夹名称，名称和未保存提示使用独立组件类名；全局标题仅做基础重置，不通过 `h2 span` 等标签组合设置辅助文字样式，不再显示“资源管理器”标题或重复的名称行，长名称省略并保留路径提示。展开时只读取当前目录的一层子文件夹和文件，支持折叠、刷新、键盘方向键及路径提示，展开目录不读取文件正文；点击文件后才按需读取。窄屏默认收起，点击资源管理器图标后以侧栏覆盖显示。
- 鼠标悬停在任意文件或文件夹行时，直接按 C 复制该条目的绝对路径，无须先点击；保留原始路径格式，不添加引号。文件或文件夹行在悬停或键盘聚焦时显示复制按钮，预留固定位置避免名称抖动；点击复制绝对路径，不触发行选择或展开。复制按钮、C 快捷键和菜单共用剪贴板逻辑，成功后对应行显示绿色勾和“路径已复制”提示，3 秒后恢复；失败在侧栏底部说明。输入框、可编辑区域、输入法组合输入、修饰键组合、按键长按重复及弹窗或树节点菜单打开时不触发；关闭侧栏、切换工作区或刷新目录树时清理监听及反馈。此操作仅将已有路径写入浏览器剪贴板，不新增核心引擎或 MCP 业务接口。
- 所有文件和文件夹支持右键及 Shift+F10 打开菜单，提供“复制绝对路径”和“复制相对路径”；菜单标明所属根文件夹名称，相对路径使用 `/` 分隔，以所属根文件夹为基准，根自身为 `.`；不属于该根的路径禁用相对复制。菜单支持方向键、Home/End 和 Escape，复制后关闭并恢复焦点。根文件夹支持右键、Shift+F10 和行末“文件夹操作”按钮，从工作区移除对应根引用；子目录不提供根移除动作。移除只改内存配置并标记未保存，保存后持久化，不删除磁盘目录、不停止进程。重复引用一并移除，失效或越界引用也可清理，仍校验工作区版本与身份。
- 左上角 Logo 使用复制到 `apps/web/src/assets/extreme-plus-logo.svg` 的本地 SVG；暗色主题自动切换透明底、浅灰与蓝色的 `extreme-plus-logo-dark.svg`，不加白色底块。目录树复用现有 Lucide 图标，按目录用途和文件类型使用鲜明配色：暗色主题提高图标亮度和饱和度，浅色主题使用更深的同色系；背景、文字及 Logo 保持原配色。
- 对工作区中授权的工程按需识别 Maven 模块和 Node 工程，避免无界递归扫描。
- 顶部工作区菜单负责新建、浏览打开、保存/另存为和关闭；新建时可选择起始文件夹，再选择 `.code-workspace` 的保存目录与名称，也可新建空工作区。资源树的添加入口只把已有文件夹加入当前工作区，不创建磁盘文件夹。
- 打开工作区提供“选择工作区文件…”和“选择本机文件夹…”，添加文件夹、新建工作区及另存为提供系统文件夹选择入口，使用 Windows 原生窗口自由选择本机位置；取消不改变当前选择或工作区。下方仍支持从已授权位置逐层浏览、筛选和刷新。系统窗口选择完成后，再通过原有确认按钮切换或保存；未保存更改仍要求先保存或明确放弃。
- `POST /api/workspace/pick` 与 MCP `workspace_pick` 共用核心流程：只接收 `kind`（`workspace_file` / `folder`）和 `expectedRevision`，验证身份及选择前后版本，单个选择窗口最长 180 秒。固定 PowerShell / Windows Forms 脚本通过 Windows UTF-16 进程参数传入，窗口结果使用 UTF-8；不接收客户端命令或路径作为授权依据。所选文件夹及其子目录加入本次后台会话授权；所选工作区文件仅授权该文件及其中明确声明、实际存在的本地文件夹，不授权工作区文件所在父目录。重启时仅为最近工作区恢复其文件与根目录授权；其他会话选择的位置需重新选择；其他平台继续使用下方位置浏览器。
- 本轮补充了选择取消、过期版本、鉴权、中文路径、文件及文件夹授权边界与工作区保存的测试代码；按默认约定未执行编译、构建或测试，原生窗口交互待重启新版后端后验证。
- 资源树支持拖拽调宽、宽度记忆、左右方向键微调、Home/End 到边界、双击复位。桌面宽度 200–520px，并为服务区保留空间；窄屏最多 360px，并为手柄预留至少 28px 空间。去掉侧栏和项目菜单的全屏点击遮罩，侧栏位于服务内容之上；活动栏宽度与侧栏左边界一致。拖拽热区 12px，带可见握柄，使用 Pointer Capture 和 window 事件兼容嵌入式浏览器，结束、取消、失焦和卸载时均清理。目录节点使用 memo，拖拽调宽不重新渲染整棵树。
- 服务搜索统一放在顶部导航栏，“全部 / Maven / Node.js”类型筛选与“服务”标题在首行左右对齐，不再显示 LOCAL 标记。对话框沿用 [Beautiful UI](https://www.beautifului.dev/) 的简洁边框、圆角与操作层级，并保留原生 dialog 的焦点约束和 Escape 关闭。
- 服务页仅展示当前工作区的真实服务与采样指标，移除演示入口、示例项目和模拟趋势。卡片铺满服务区，按服务区实际可用宽度布局：达到 760px 时每行两张，否则每行一张，拖动侧栏也会自适应。默认展示名称、类型、运行状态与指标；项目目录、识别文件和启动入口收进下拉详情。点击卡片查看底部运行输出，仅点击箭头展开详情。Java 启动按钮为绿色实心三角，停止按钮为红色实心圆点，并保留文字；运行状态未知时禁用操作，Node.js 服务同样支持一键启停、独立启动配置与运行输出。
- 顶栏展示 DevFlow 后端自身的 CPU 和物理内存；仅采集当前服务 PID，不包括托管工程、Codex 或浏览器，CPU 按可用逻辑核心容量归一。首次 CPU 采样为 null，页面显示采样中；每 3 秒刷新，后台页面暂停轮询，请求超时 10 秒。核心独立采样器仅刷新自身 PID，缓存 2 秒，避免影响工程指标采样；关闭工作区后仍可查看。窄屏顶栏第二行同时容纳搜索与自身指标。
- 指标区不重复显示 DevFlow 名称，图标与数值按占用分色：CPU 低于 30% 为绿色、30% 至低于 70% 为黄色、达到 70% 为红色；内存低于 256 MiB 为绿色、256 MiB 至低于 512 MiB 为黄色、达到 512 MiB 为红色。内存阈值为当前后端进程物理内存的绝对值，不表示整机内存压力；无采样时为灰色，悬停说明阈值，并提供文字状态供辅助技术读取。
- JVM 诊断、运行输出等底部面板支持拖拽上边缘调整高度，保留高度偏好；支持上下方向键微调、Home/End 到边界、双击恢复默认高度。高度随容器空间约束，为上方服务列表保留空间；使用 Pointer Capture 和 window 事件，结束、取消、失焦或卸载时清理拖拽。JVM 诊断当前提供官方 Arthas 快速附加与独立 HTTP 控制台入口。

### 验收标准

服务列表首先按运行状态排序：检测到运行进程或处于构建 / 启动中的服务优先，运行组内保持工作区原有顺序；其余服务按最近使用时间倒序，无使用记录或时间相同则保持工作区原有顺序。检测到运行状态、查看运行输出、启动 / 停止、打开启动配置或诊断时更新使用记录，搜索和类型筛选后仍保持此规则。最近使用属于 Web 展示偏好，以服务标识（包括 Node.js 启动脚本）分别保存，最多保留 1000 项；本地页面写入浏览器 localStorage，刷新和重新打开后保留，只读分享使用独立的 sessionStorage。运行状态继续采用 HTTP / MCP 共用核心引擎的采样结果，因此页面也会记住采样时正在运行的 MCP 或外部启动服务；未被页面观察到的运行不补记。此次仅修改前端展示及文档，未运行编译或测试。

服务卡片不展示 Maven / Node.js 类型标签，通过顶部类型筛选区分服务；名称下方仅按需显示端口与运行状态，无端口时不保留空摘要行。

服务列表底部不再重复展示服务数量、Maven / Node.js 类型说明和进程指标刷新提示；数量与筛选保留在顶部，指标不可用时由各服务卡片提示。

暗色 / 浅色主题切换使用 View Transition，从页面中心以圆形向外扩散，约 560ms 完成；过渡期间忽略重复点击，继续记忆主题偏好。浏览器不支持该能力或系统启用减少动态效果时直接切换。

- 能打开 `D:\WorkSpace` 中现有的全部 `.code-workspace` 文件，正确显示文件夹名称及解析后的路径；路径不可用时正确标记。
- 当前验收样本共 9 个文件：`2.0.0`、`ai-chat`、`extreme-plus-portal`、`extreme-skills`、`framework-master`、`health-management`、`main-system`、`plan`、`vue3-upgrade-design`（均带 `.code-workspace` 扩展名）。
- 能创建工作区、添加文件夹并保存，保存后可由 DevFlow 和 VS Code 重新打开，文件夹及原有配置正确。
- 能直接打开文件夹并显示其中识别到的工程。

### 服务进程与端口展示

- 服务卡片直接显示主 PID；Java 应用进程与 Maven 启动器、Windows `conhost.exe` 辅助进程分开，Node.js 尽量展示包管理器下的应用进程，工作子进程不重复作为主进程。同一服务确有多个独立应用进程时保留多个主 PID，不随意取第一个。构建阶段尚无应用进程时显示“等待应用启动”。展开项目详情后显示辅助进程 / 启动器名称、PID 和父 PID。
- 核心运行快照新增 `primaryPids`、`processes`（`pid` / `parentPid` / `name` / `role`）及 `listeners`（`pid` / `port` / `purpose` / `purposeSource`），HTTP、MCP 和只读分享共用识别逻辑。保留原 `pids`、`javaPids`、`nodePids`、`listeningPorts` 字段及其语义；资源指标和进程树清理继续覆盖全部关联进程。进程元数据不返回完整命令行。
- 卡片端口标签仅显示端口数字（如 `8001`），按端口号去重排序，最多展示三个，其余以 `+N` 提示；所属 PID、用途及其依据放在展开详情中。新旧后端均使用简洁数字标签。同一 PID 的 IPv4 / IPv6 同端口去重，不同 PID 的同端口在详情中保留各自归属；采集失败时 `listeners` 为 null，无监听时为空数组。
- 用途仅依据可读取的启动参数：JDWP 监听地址、显式 `server.port`、`management.server.port`、`grpc.server.port` 以及 Arthas 启动器的端口参数；用途来源标记为 `command_line`。配置文件、远程配置或应用代码决定的端口暂不推断，显示“用途未知”（`unknown`），不根据 8080、50051 等常见号码断言协议。旧后端仍显示端口，并提示更新后才能识别主 PID 及端口归属。
- 已补充 Windows 中文路径、控制台 / Maven 辅助进程、Node.js 包管理器与子进程、监听归属及去重、未知用途、采集失败和退出后清空的测试代码；本轮未运行编译或测试。

## 文件查看与编辑

- 新增轻量文件编辑能力，扩展原 MVP 不提供源码编辑器的范围。参考 [VS Code 的 Explorer / Editor 与预览标签交互](https://code.visualstudio.com/docs/editing/getting-started/userinterface)，使用 [CodeMirror 6](https://codemirror.net/docs/guide/) 的开源编辑内核，不引入 Monaco 或完整 IDE。
- 点击左侧文件节点，在右侧预览文件；双击文件或标签固定标签，首次编辑自动固定。未修改的预览标签可被下一次预览复用；同时最多打开 12 个文件，切换标签保留未保存内容、光标、滚动与撤销历史。顶部“服务”标签返回原服务页面，底部运行输出保持独立；窄屏打开文件后自动收起目录树。
- 提供行号、括号匹配、折叠、撤销/重做、查找替换、Tab 缩进、Ctrl/Cmd+S 立即保存，停止输入 700ms 后自动保存；JS/TS/JSX/TSX、JSON、HTML/Vue、CSS、Java、Markdown、XML/SVG 提供语法高亮，其他 UTF-8 文本按纯文本编辑。`components/editor/SourceEditor.tsx` 是公共编辑组件边界，工作区业务不直接调用 CodeMirror API；编辑器按需加载，跟随现有明暗主题令牌。
- Ctrl/Cmd+F 查找替换栏位于编辑区域顶部，采用紧凑双行布局；查找行包含输入框、大小写/全字/正则切换、上下项导航和全选匹配，替换行可收起，包含替换输入与中文操作按钮。展开箭头使用 CSS 绘制并始终对齐第一行，避免字符基线造成视觉偏下。只读时隐藏替换入口，窄区域自动换行，明暗主题与匹配高亮沿用工作台令牌。
- 普通源码与三方合并编辑器共用 `editorSearch.ts` 和中文短语配置；查询、导航、替换复用 CodeMirror 搜索命令，保留 Enter/Shift+Enter、Escape 等键盘交互并避开中文输入法组合输入。显式声明现有传递依赖 `@codemirror/search`，同步锁文件；未安装依赖、编译或运行测试，顶部布局、明暗主题、窄屏与键盘操作待界面验收。
- 语法高亮按文件名和扩展名自动识别，复用 CodeMirror 官方语言目录并按需加载解析器；在既有格式外支持 YAML/YML、Python/PYI/PYW、SQL、Shell/Zsh、PowerShell、Dockerfile、TOML、Properties/INI、Go、Rust、C/C++ 等常见格式。额外识别 `.env` / `.env.*`、`Dockerfile.*`、Shell 启动文件、JSONC 与 `.code-workspace`、MTS/CTS、SVG/XSD 等配置别名；JSONC/JSON5 使用 JSON 高亮规则，环境变量文件使用 Properties 高亮规则，未知格式保持纯文本。
- `.log` / `.out` / `.err` 及其轮转后缀使用本地轻量日志高亮：时间、ERROR/FATAL、WARN、INFO、DEBUG/TRACE、异常名称与 Java 堆栈、URL、字符串、数字和线程标记使用不同主题颜色；兼容方括号包裹的级别和时间。语言模块加载失败时保持可编辑的纯文本，异步加载不重建编辑器或清空撤销记录，关闭标签后不会应用迟到结果。高亮仅影响 Web 展示，文件 HTTP/MCP 读写规则保持共用核心服务。
- 本次高亮扩展已更新前端依赖与锁文件（安装时禁用生命周期脚本），按约定未运行编译、构建或测试命令，实际显示待用户构建后验证。
- 应用内确认/提示经公共表单防腐层 `BaseDialogProvider / useDialogs` 统一呈现，业务不再直接调用浏览器 `confirm / alert`；覆盖未保存文件关闭/重新读取、标签数量上限、永久删除、最近工作区切换与 Git 回滚。弹窗继承明暗主题、字号、表面和边框令牌，复用公共按钮，危险操作明确标注；默认焦点落在取消，支持 Escape、焦点约束、请求排队与卸载取消。确认放弃草稿期间暂停该文件自动保存，取消后恢复，确认后重查状态。浏览器刷新/关闭页面的 `beforeunload` 提示仍由浏览器控制，无法定制外观。
- 本次公共对话框改造已核对源码与差异空白，按约定未执行编译、构建或测试命令，实际交互待用户构建验证。
- 文件标签显示未保存圆点，关闭脏标签、重新读取脏文件前确认放弃修改；刷新或关闭浏览器时触发原生未保存提醒。本轮不实现跨重启草稿恢复、空文件创建、重命名或运行编辑内容；标签按工作区保存和恢复，保存时仍以当前工作区重新校验文件成员资格。
- `GET /api/workspace/file?path=...&expectedRevision=...` / `PUT /api/workspace/file` 与 MCP `workspace_file_read` / `workspace_file_write` 共用 `WorkspaceService` 读写逻辑。读写均鉴权、校验活动工作区版本、授权根及规范化后的文件路径；只允许当前工作区根内已有的普通文件，排除 node_modules、target、.git、build、dist。拒绝跨根链接、二进制、非 UTF-8 及超过 1 MiB 的文本；只读文件仅查看，当前活动工作区配置通过原工作区管理修改。
- 保存请求包含读取时的精确 `expectedContent`（含 BOM / 原始换行），外部编辑或其他入口先保存时返回 `file_conflict`，保留浏览器草稿且不覆盖磁盘；“重新读取”会要求确认放弃草稿。保存保留原 UTF-8 BOM 和 LF / CRLF / CR 风格；混合换行统一为检测到的风格。先写同目录临时文件并同步，再重新核对路径和原文、保留权限后替换，失败清理临时文件；文件系统外部写入与最终替换之间仍存在很短的竞争窗口，无跨进程文件锁。
- 编辑器右上角展示最近一次成功写入的本地时间（`保存于 HH:mm:ss`），移除手动保存按钮；等待保存、保存中和失败时展示相应状态。各标签独立防抖，保存过程中继续输入的内容在本次成功后再次保存；失败后保留草稿并暂停自动保存，可用 Ctrl/Cmd+S 重试或重新读取合并。
- 左侧文件节点右键或更多菜单提供“复制文件”“删除文件”。复制磁盘内容为同目录 `名称 副本.扩展名`，重名时追加编号，不覆盖已有文件；不包含尚未保存的编辑草稿。删除前确认永久删除及放弃该文件草稿；读取或保存期间阻止删除，删除过程中暂停编辑和自动保存，成功关闭标签，失败保留草稿；操作后仅刷新所在已展开目录，保留其他展开状态。
- `POST /api/workspace/file/copy` / `DELETE /api/workspace/file` 与 MCP `workspace_file_copy` / `workspace_file_delete` 共用核心服务，请求均为 `{ path, expectedRevision }`，返回 `{ path, revision }`。鉴权、版本、授权根、活动工作区成员、普通文件及忽略目录校验复用读写路径规则；不复制或递归删除文件夹。复制按原始字节流处理，删除无需解码，支持二进制文件；删除保护只读文件、活动工作区配置与最近工作区记录。文件系统外部替换路径仍存在短暂竞争窗口，不提供跨进程文件锁。
- 自动保存与文件复制/删除已补充源码和核心边界测试，按约定未执行编译、构建、格式化或测试命令；待用户构建新版前后端后验收。
- 文件保存不改变工作区版本或启动进程，清除服务发现缓存供下次刷新读取新的描述文件。文件 API 与 MCP 请求体上限 16 MiB，用于容纳双份原文及 JSON 转义；核心文件内容仍限制 1 MiB。接口返回只读原因、BOM、换行与工作区版本，适配层仅转发，不复制读写规则。
- 已补充中文路径、BOM / CRLF、外部修改与过期读取冲突、未鉴权、过期工作区、授权目录中非当前根文件、链接越界、二进制、非法编码、大文件及活动配置只读保护的测试代码。已更新并安装编辑器依赖（禁用安装脚本），按约定未执行编译、构建或测试命令；实际 UI 与保存行为待构建重启后验收。

### 刷新后恢复工作台

- 本机浏览器按工作区身份保存已打开的普通文件标签、顺序、预览/固定状态、活动文件、源码光标行列、目录展开和选中项；同时保留侧栏开关与视图、每个 Markdown 标签的文档/源码/对照模式。刷新及硬刷新后恢复，无需重新逐层展开。工作区切换保存各自标签，不混用不同工作区记录。
- 保存的是界面元数据，不保存文件正文、未保存草稿、Diff 内容或访问凭证；草稿仍沿用原生离开保护。最多恢复 12 个普通文件标签和 200 个目录展开项；只读取当前活动文件，其他文件点击时再读取。目录只逐层恢复已展开节点，不递归扫描项目。恢复内容均经过既有鉴权与当前工作区文件 API；文件已移除或越界时显示读取错误。
- 标签元数据写入防抖 250ms，并在页面隐藏/离开时补写；浏览器禁用存储时退回内存行为。本机使用 localStorage，分享页使用当前标签的 sessionStorage，两种入口隔离，不把分享文件正文或凭证写入持久记录。旧保存/读取在工作区切换后不更新新标签。
- 只读分享当前普通文件每 15 秒通过同一个读取接口同步磁盘内容，页面隐藏暂停，回到前台/联网后补查（至少间隔 5 秒），错误退避 30–60 秒。仅内容/只读状态变化时更新现有编辑器，不重新挂载、不加载后台标签或轮询整棵目录。正文和源码更新尽量保留滚动与选择；失效邀请沿用原有清理流程。
- 待验收：固定/预览多个标签并展开目录后硬刷新，检查活动文件、光标和 Markdown 模式；切换工作区后分别恢复；删除/撤销权限后无法通过旧状态读取；分享页前台 15 秒内显示已保存修改、后台停止请求、网络错误退避恢复、撤销邀请清空页面。仅完成源码与差异检查，未执行编译或测试。

### Markdown 文档编辑

- 图表渲染的临时测量节点放入固定定位、零高度且裁剪溢出的独立容器，保留 SVG 测量能力，并在成功/失败后清理，避免 Mermaid 初始化时短暂撑出页面外层滚动条。表格单元格覆盖上游 `nowrap`，中文及长标识符在列内换行，宽表仍在表格内横向滚动。待构建后验收首次打开、重新读取和主题切换时的滚动条，以及长中文/API 标识符表格；未执行构建或测试。

- 文档布局修正：将加载可见状态放在 Vditor 挂载节点之外，避免 React 更新 className 清除其纵向布局与主题类；外框、内容区和正文逐层约束 flex 高度，正文独立滚动并显式保留滚动条。只读预览不再使用依赖默认的降低透明度样式。
- 文件路径 `file-toolbar` 已移除，重新读取按钮移入标签栏；活动文件的保存时间/等待/失败状态经回调显示在应用顶部 CPU/内存区域，服务和 Diff 标签不显示文件保存状态。Markdown 文档/源码/对照改为标签栏右侧紧凑图标分段切换，保留按钮提示与无障碍标签，不再独占一行。
- `.md` / `.markdown` 经公共 `FileEditor` 路由至 Vditor，参考 Office Viewer 的 Vditor 接入方式，直接使用开源上游 npm 分发，并通过公共编辑器防腐层隔离。默认“文档”视图可直接编辑正文，工具栏提供标题、强调、列表、任务、引用、代码、链接、表格、撤销与目录；“源码”保留 CodeMirror 编辑，“对照”在源码旁显示只读实时文档。小于 650px 的编辑区域改为上下对照。
- 中文正文限制阅读宽度，重新配置标题层次、段落节奏、主题颜色、表格及代码排版；宽表格自身横向滚动，不撑开页面。Mermaid 图表就地渲染，双击放大后可缩放 50%–300%；支持 KaTeX 公式、代码高亮、任务列表和脚注。图表语法未闭合时显示诊断，原始代码保留可修正。
- 所需 Lute、中文语言包、图标、代码高亮、KaTeX 字体和主题由 Vite 插件按固定白名单本地提供，构建时输出到静态目录；Mermaid 作为本地依赖按需加载，不依赖在线 CDN。新增 Vite 插件需重启开发服务器，已构建服务需更新前端构建产物；此处不要求重编译 Rust。
- 文档和源码使用同一草稿，沿用 700ms 自动保存、精确 `expectedContent` 冲突检测、BOM/换行保持、只读与删除保护；HTTP/MCP 文件服务无新增业务分支。初次打开及初始化规范化不会触发保存，真实可视化编辑可能规范化表格、空行等 Markdown 排版，源码模式可保留原有排版。切换模式前刷新最新草稿，文档输入回传后不重设实例，避免光标跳动。
- 编辑正文中的链接通过 Ctrl/Cmd+单击或双击打开；对照文档可单击。同目录文档相对链接通过既有文件打开流程处理，授权与规范路径仍由共享核心校验；文内锚点就地跳转。本轮不增加本地图片二进制读取或附件上传接口。
- 参考样本为 `D:\CompanyProject\extreme-plus\extreme-module\extreme-plus-ai\docs\agent\permission-design.md`：已按 UTF-8 读取其结构，包含 2 个 Mermaid 流程图、4 个表格、2 个纯文本代码块，以及相邻 `loop-prevention-design.md` 链接；未改写该文档。
- 待用户构建后验收：打开样本时不产生未保存状态；中文段落与 4 个表格正常呈现；2 张流程图正确布局且 `<br/>` 换行有效；图表可放大缩放；编辑正文/表格/图表后自动保存，源码与对照一致；主题切换和 400–550px 布局可用；相邻文档链接仍遵守工作区权限；外部修改冲突和只读保护保持有效。按约定未执行编译、构建、测试或实机 UI 验收，仅完成源码与差异空白检查。

## 项目启动与 Arthas

- 支持启动 Java/Spring Boot 项目，识别 Maven 多模块工程及实际启动模块。
- Spring Boot 支持附加启动配置，区分 JVM 参数、应用参数、环境变量和 Spring Profile。
- 支持启动 Node 项目，根据所属工程的包管理器、脚本及配置确定工作目录与启动方式。
- 展示启动日志和进程状态，支持停止由 DevFlow 启动的进程并清理相应进程树。
- Java / Node 运行输出中的 HTTP、HTTPS 地址显示为主题强调色下划线链接，支持鼠标点击与键盘聚焦后在新标签页打开，原输出栏继续保留。识别前移除 ANSI 控制序列，保留原文及分段日志颜色；不解析日志 HTML，不激活其他协议。Local / Network 地址、端口、路径与查询参数均保留，常见句尾标点及外围括号不计入链接。此行为仅为 Web 输出展示，无新增核心业务或 MCP 接口；按默认约定未执行构建或测试。
- 支持向目标 Java 进程附加 Arthas，展示附加结果；附加是独立操作，不与启动或 JVM DEBUG 绑定。
- UI 和 MCP 可以操作、查询同一运行实例，使用一致的错误语义；重复启动和跨入口并发操作由核心统一处理。

### 验收标准

| 类型 | 目标路径 | 验收结果 |
| --- | --- | --- |
| Java/Spring Boot | `D:\CompanyProject\extreme-plus\extreme-module\extreme-plus-system` | 能带指定附加参数启动，查看真实日志和状态，停止进程，并能对运行中的目标 JVM 附加 Arthas。 |
| Node | `D:\CompanyProject\main-system\projects\extreme-plus-system-ai` | 能通过所属工程的实际启动方式运行目标项目，查看真实日志和状态，并停止所启动的进程。 |

Node 验收目录当前没有 `package.json`，需定位其所属主工程的启动配置，不能假设直接在该目录执行 `npm run dev`。Java 验收目标是 Maven 子模块，需结合父工程解析启动配置。依赖、工具链、实际启动命令和附加参数在实现及验收前确认，不在需求中臆定。

## 当前实现与边界

- 已有基础项目发现、工作区页面，以及 HTTP/MCP 共用的只读进程指标服务。
- 当前进程指标基于应用启动主类、执行 JAR 或 Node 入口脚本进行尽力观测，不代表 DevFlow 已托管该进程；未匹配到进程也不能证明服务已停止。Java classpath 中出现依赖模块不作为其运行依据，JVM 与 Node 工程分类型匹配；只聚合匹配入口的子进程。内存指标为进程物理内存，不是 JVM 堆。
- 演示数据已移除。Java 和 Node.js 操作均使用真实共享运行服务。
- Java 检测到 PID 或处于托管构建阶段时显示“停止”，未检测到时显示“启动”；外部启动且关联到项目的 Java 应用也可以选择 PID 停止或附加，但无法追溯获取外部 stdout。
- 服务发现本身不执行 Maven；实际启动会在目标模块执行 Maven `compile dependency:build-classpath`，使用本机 Maven 配置和仓库，随后直接启动识别出的主类。依赖尚未安装的多模块项目仍需按项目要求先构建/安装父工程及依赖；不自动安装整个仓库，不支持任意自定义构建命令、Gradle 或自定义 classes 输出目录。
- UI 适配 400–550px 分屏，保持紧凑，支持项目列表搜索和类型筛选；未知状态不得显示为正在运行或已完成。
- 本次整理不包含独立的上下文检索、QA 或完整诊断平台，也不删除对应代码目录。

## 工作区管理使用说明

点击顶部工作区名称或“管理工作区”，可以打开工作区文件/文件夹、新建工作区、添加文件夹、保存及另存为。浏览器页面通过输入本机完整路径操作，也提供授权根目录下已有工作区文件的快捷列表。添加文件夹先修改内存中的共享工作区，需主动保存；切换工作区前提示未保存的更改。当前活动工作区及未保存状态不跨后端重启保留，已保存的工作区文件可重新打开。

启动示例（命令由用户自行执行，会编译并启动服务）：

```powershell
.\scripts\dev.ps1 -Workspace 'D:\WorkSpace\framework-master.code-workspace' -AllowedRoots @('D:\WorkSpace', 'D:\CompanyProject', 'D:\PersonalProject', 'D:\AiFlow', 'D:\DevelopmentTools\Maven\repository', 'D:\Temp')
```

- `DEVFLOW_WORKSPACE` 或 `-Workspace`：初始工作区文件或目录；其目录自动加入授权范围。
- `DEVFLOW_ALLOWED_ROOTS` 或 `-AllowedRoots`：可访问的目录白名单，必须存在；环境变量使用系统路径列表分隔符（Windows 为分号）。工作区文件引用其他目录不会自动扩大授权范围。
- `DEVFLOW_API_TOKEN`：可选的固定 Bearer 令牌，供 MCP 客户端配置；未设置则每次启动生成随机令牌。Web 从受本地 Host/Origin 和自定义请求头保护的 `/api/session` 获取会话令牌，仅保存在内存。`.env.example` 不会自动加载。
- 工作区文件最大 2 MiB、最多 512 个文件夹。仅访问本地路径及 `file:` URI；远程 URI 保留并显示不支持，不尝试连接。
- 保存已有文件时保留配置和注释、检测外部修改并使用同目录临时文件替换；新建/另存为不覆盖其他已有文件。另存为会把原来的相对文件夹路径改为绝对路径，避免位置变化导致引用失效。

## HTTP 与 MCP 契约

所有下述业务接口使用 `Authorization: Bearer <token>`；HTTP 和 MCP 调用同一 `DevFlowEngine` 的工作区与运行服务。

| HTTP | MCP | 用途 |
| --- | --- | --- |
| `GET /api/workspace`（兼容 `/api/projects`） | `workspace_get` | 当前工作区、文件夹、项目、版本和未保存状态 |
| `GET /api/workspace?refresh=true`（兼容 `/api/projects?refresh=true`） | `workspace_refresh` | 重新从源码识别可启动服务，不改变工作区版本或启动项目 |
| `GET /api/workspace/catalog` | `workspace_catalog` | 授权目录及其直属工作区文件，不递归扫描 |
| `GET /api/workspace/browse?path=...` | `workspace_browse` | 浏览授权位置的一层文件夹与工作区文件；省略 path 返回授权根，不要求已有工作区 |
| `GET /api/workspace/children?path=...&expectedRevision=...` | `workspace_children` | 当前工作区内目录的一层子项；目录优先排序，最多处理 2000 项，排除链接及依赖/构建/Git 目录 |
| `POST /api/workspace`，`operation.type=open` | `workspace_open` | 打开工作区文件或文件夹 |
| `POST /api/workspace`，`operation.type=create` | `workspace_create` | 新建空工作区文件并打开 |
| `POST /api/workspace`，`operation.type=create_from_folder` | `workspace_create_from_folder` | 用 path 指定新文件、folder 指定起始文件夹；校验完成后原子创建并打开 |
| `POST /api/workspace`，`operation.type=close` | `workspace_close` | 关闭当前工作区，遵守版本和未保存保护，不停止进程 |
| `POST /api/workspace`，`operation.type=add_folder` | `workspace_add_folder` | 添加文件夹及可选显示名称 |
| `POST /api/workspace`，`operation.type=remove_folder` | `workspace_remove_folder` | path 指定工作区根引用，移除并标记未保存，不删除文件或停止进程 |
| `POST /api/workspace`，`operation.type=save` | `workspace_save` | 保存；可选 path 指定另存为位置 |
| `GET /api/runtime/self` | `devflow_metrics` | 当前 DevFlow 后端 PID、CPU、物理内存、运行时间及采样时间；不要求已打开工作区 |
| `GET /api/runtime/projects` | `runtime_list` | 工作区项目的关联 PID、资源指标、实际 TCP 监听端口与采集错误 |
| `POST /api/runtime/java` | `java_execute` | `projectPath`、`expectedRevision`、`operation`：Java 启动、停止或 Arthas 附加 |
| `GET /api/runtime/java/options?projectPath=...` | `java_options` | Java 主类及资源目录中 `application-{profile}.yml/yaml/properties` 的 Profile 候选 |
| `GET /api/runtime/java/output?projectPath=...&after=...&runId=...` | `java_output` | 托管状态、stdout/stderr/构建/Arthas 增量日志、退出码及 Arthas 地址 |
| `POST /api/runtime/node` | `node_execute` | `projectPath`、聚合服务的 `script`、`expectedRevision` 和 `operation`：启动或停止 Node 服务 |
| `GET /api/runtime/node/options?projectPath=...&script=...` | `node_options` | 服务的可用脚本与自动识别包管理器 |
| `GET /api/runtime/node/output?projectPath=...&script=...&after=...&runId=...` | `node_output` | Node 状态、stdout/stderr 增量日志、PID 和退出码 |

### Java 使用约定

卡片以图标角落的绿色圆点表示运行，构建中为黄色；运行时不显示“检测到运行进程”文字。类型旁显示实际 TCP 监听端口标签，最多显示 3 个及剩余数量，悬停或展开详情可看完整列表。端口由共享核心按关联 PID 聚合，不从 Spring 配置猜测，也不把出站连接或 UDP 当作监听端口。当前 Windows 复用 [系统 netstat](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/netstat) 的 `-ano -p tcp` 模式，一次采样读取本机全表后按服务分配，随已有 2 秒缓存刷新；进程退出后清空，IPv4/IPv6 相同端口去重。端口列表包含业务、调试和 Arthas 等诊断端口，不代表每个端口都是 HTTP 页面。

- 通用发现无需服务名或固定端口预设，不限于 DevFlow 或当前编辑器启动的进程。外部 Java 根据启动主类或可执行 JAR 路径识别，Node 根据实际执行脚本识别，并聚合关联子进程；详情显示关联 PID。同主类存在多个项目时优先参考工作目录及 classpath，无法消除歧义时不任意归属。classpath 中的依赖本身不视为运行服务。托管进程以内部记录的 PID 和启动时间校验身份，在端口和子进程聚合之前补入；HTTP/MCP 运行操作使用与展示相同的工作区范围和归属规则。
- `listeningPorts: []` 表示采集成功但没有关联监听端口；`listeningPorts: null` 配合新增 `portError` 给出失败原因，包括 netstat 无法启动、超时、异常退出、读取失败或超出 4 MiB 限制。其他系统明确返回尚未支持的原因，失败不阻断其余运行指标。Web 区分“等待监听”“暂无监听端口”“端口采集失败”，具体错误可在悬停与展开详情中查看。旧后端缺少 `listeningPorts` 字段时显示“后端需更新”，提示由用户重新编译并重启后端。
- 范围为本机 Windows 可读取身份的进程与监听端口；外部进程的启动信息不可读、仅有未展开的 Java 参数文件或 JAR 位于工作区外且缺少可验证关联信息时，不能保证归属。Docker/WSL 仅能在宿主机层看到公开的端口，容器内服务归属与远程服务器实例不在本次范围内。未接入 Nacos、未增加预期端口配置或启动前端口冲突拦截；不能仅凭端口号推断服务身份。

本次通用端口修正新增了外部进程及子进程端口归属、中文 Windows 路径、IPv4/IPv6 去重、退出后清空、采集失败恢复、托管 PID 聚合顺序与 PID 复用、同主类多项目歧义的单元用例。按用户约定未执行编译、格式化或测试命令，待用户编译并重启后验证。

- 点击服务卡片“启动”直接使用本机已保存配置；无配置时使用空参数、默认 Spring Profile 和唯一启动主类，不再弹出配置。更多操作菜单提供“启动配置”，保存仅更新本机偏好，下次启动生效，不影响运行中的服务；有多个主类且未选择时，在运行输出面板提示先配置。HTTP/MCP 继续使用同一核心启动操作，接口不变。
- 启动配置区的 JVM 参数、程序参数均每行一个完整参数，无须为包含空格的值添加引号；HTTP/MCP 对应 `jvmArgs`、`args` 字符串数组。`profiles` 为 Spring Profile 数组，支持多选检测候选或手填，空数组沿用应用默认配置。`mainClass` 必须来自当前项目发现结果。界面兼容读取原先成功提交时保存的配置。
- 表单通过 `apps/web/src/components/form` 公共组件层收口：`BaseSelect`、`BaseCheckbox` 内部适配 Radix 交互原语，业务只使用项目定义的值、选项和回调；`BaseField`、`BaseInput`、`BaseTextarea`、`BaseButton` 统一标签、字号、尺寸、焦点及禁用状态，复用工作区深浅色令牌。启动配置已整体接入，日志工具栏的 PID 选择复用同一 `BaseSelect`；其他既有表单按后续实际修改逐步迁移。下拉层挂在所属原生 dialog 内，保留模态焦点约束、方向键选项导航与 Escape 先关闭下拉的行为。
- 启动、停止请求提交时，卡片按钮显示旋转图标与“启动中… / 停止中…”，阻止重复提交，并等待操作后的新指标采样再恢复按钮状态；构建期间继续显示启动动画，且允许点击取消启动。系统启用减少动态效果时保留状态文字，停止旋转动画。
- 请求示例：`{ "projectPath": "D:/code/app", "expectedRevision": 1, "operation": { "type": "start", "jvmArgs": ["-Xmx512m"], "args": ["--server.port=8080"], "profiles": ["dev"], "mainClass": "example.Application" } }`。停止传 `{ "type": "stop" }`；外部进程需另传 `pid`，附加传 `{ "type": "attach", "pid": 12345 }`。
- Java 启动器优先使用 `DEVFLOW_JAVA_HOME`，其次 `JAVA_HOME`、PATH；Maven 使用 `MAVEN_HOME`/`M2_HOME` 或 PATH 中 Maven 的 ClassWorlds 启动器，直接调用 Java，不调用 cmd/PowerShell、不执行请求指定的 shell。Windows 默认给子 JVM 配置 `.devflow/sockets` 原生 NIO 临时路径，避免当前系统继承 TEMP 路径导致的 Unix-domain loopback 错误；可用 `DEVFLOW_JAVA_SOCKET_DIR` 指定更短的本机目录，或通过 JVM 参数覆盖系统属性。
- 启动时异步返回 `starting`，Maven 完成后变为 `running`；表示进程已启动，不等于业务服务健康就绪。失败、退出、取消均写入输出，构建过程可取消。停止校验 PID 与启动时间并清理当前进程树，避免误伤 PID 复用。正常 Ctrl+C 关闭 DevFlow 会清理托管进程；操作系统强制终止后端不保证执行清理。
- 资源指标的 `pids` 包含关联的进程树，另提供 `javaPids` 作为 Java 操作候选，排除 conhost 和 Maven 启动器。托管服务优先选择其应用主 PID，外部服务存在多个 Java 进程时要求选择目标。
- 输出保留每个会话最近 2000 条、单条最多 16 KiB，每次最多返回 500 条。将返回的 `runId`、`nextCursor` 作为下次 `runId`、`after`，新会话自动从头读取。输出区保留最近 2000 条，支持手动查看历史和回到底部；进程与日志状态不跨后端重启持久化。
- 输出面板的运行状态、PID 选择、停止与 Arthas 操作和“运行输出”标题同排，不再额外占一行；窄屏缩为带提示和无障碍名称的图标按钮。正文不显示 stdout/stderr 等来源前缀。Spring Boot 标准日志按字段着色：时间与应用/线程上下文为次级文字色，INFO 绿色、WARN 黄色、ERROR/FATAL 红色、DEBUG 蓝紫色、TRACE 灰色，PID 洋红色、类名青色，普通正文保持正常文字色；警告/错误正文分别用黄色/红色，构建/启动成功正文为绿色。非标准格式仍识别级别文字并单独着色，异常堆栈延续错误/警告色，无级别 stderr 默认标红。保留原始空格、字段和文本，适配深浅主题，移除 ANSI 控制码，使用 React 文本节点渲染。
- 从 [Arthas 官方来源](https://arthas.aliyun.com/en/doc/quick-start.html) 下载 `arthas-boot.jar` 到 `.devflow/tools/arthas-boot.jar`，或配置 `DEVFLOW_ARTHAS_BOOT`。首次挂载官方启动器可能下载完整发行包。独立附加应用 JVM，使用 `--attach-only`，随机 HTTP/Telnet 端口仅绑定 `127.0.0.1`，附加最长等待 90 秒，端口就绪后才提供控制台链接；目标应用需允许 JDK Attach 且具有相同用户权限。Arthas 的控制台由官方工具提供，DevFlow 当前不转发任意诊断命令。

写操作需要先读取当前 `revision`，再以 `expectedRevision` 提交。HTTP 请求为 `{ expectedRevision, discardChanges?, operation: { type, path?, name? } }`；MCP 将 `expectedRevision`、`discardChanges`、`path`、`name` 放在工具 arguments 中。版本过期或文件被外部修改时返回 `conflict`，不得盲目重试覆盖；只有明确放弃未保存更改时才传 `discardChanges: true`。

`create_from_folder` 另需 `folder`，`close` 无路径参数。原有工具与请求保持兼容，文件浏览和工作区操作的授权均由共享核心校验。

兼容工具 `workspace_list` 继续返回项目数组的文本内容，新增 `workspace_get` 提供完整工作区快照。`runtime_list` 和 HTTP 指标接口改为跟随当前工作区的全部可访问项目。MCP 传输仍为实验性 JSON-RPC 基础，完整协议兼容待后续完善。

### Node.js 使用约定

- 聚合前端例如 `main-system/package.json` 的 `system: npm run serve --project=extreme-plus-system-ai`，显示为 `main-system / system`；`integration/file/notice/...` 各自有卡片、日志、配置和状态。同一服务禁止重复启动，不同脚本允许同时运行；服务共享根目录，操作身份使用 `projectPath + nodeScript`，不使用虚构文件路径。
- 点击启动直接采用已保存配置或默认脚本；更多操作中的“启动配置”可选择允许的脚本、包管理器及每行一个附加参数，保存仅影响下次启动。Node 服务不展示 JVM/Arthas 操作。启动/停止沿用按钮中间态和公共表单组件，日志复用 `RuntimeConsole`。
- 包管理器默认按 package.json 的 `packageManager`、pnpm-lock.yaml、yarn.lock、npm 顺序选择，可显式选择 npm/pnpm/yarn。Node 从 `DEVFLOW_NODE_HOME`、`NODE_HOME` 或 PATH 查找；包管理器优先使用 `DEVFLOW_NPM_CLI / DEVFLOW_PNPM_CLI / DEVFLOW_YARN_CLI` 指定的已安装 JS CLI，再查 Node/PATH 目录的安装入口、独立 exe、已安装 Corepack。Corepack 禁止网络下载和自动写入 packageManager，禁用 pnpm 的版本自动安装；不自动运行 install。缺少本机工具或依赖时保留实际失败日志。
- 核心直接执行 Node + 包管理器 CLI（或已安装包管理器 exe），HTTP/MCP 不接受任意命令或可执行路径。包管理器按项目自身语义执行 package.json 中已授权脚本及 pre/post 钩子；附加参数以参数数组传递，npm 使用 `--` 分隔。启动前重新读取有界 package.json，已删除/不允许的脚本、越界路径、工作区版本过期均拒绝。
- 状态 `running` 表示启动器已创建，不代表 Vite/业务健康检查通过；真实监听端口继续按服务 PID 归属显示。聚合工程的托管进程优先使用 PID + 启动时间记录；外部进程按包管理器启动脚本、精确 `--project` 选择器与进程树识别，证据不足时不任意分配给同目录的多个服务。`nodePids` 提供 Node 操作候选；停止仅作用于该服务进程树，并校验 PID 身份。
- Java 和 Node 共享有界日志与进程清理实现：最多保留 2000 行、单行 16 KiB、每次 500 行；通过 runId/nextCursor 增量读取。正常关闭 DevFlow 会清理托管进程，强制杀死后端不能保证清理。
- 默认后端仍绑定 `127.0.0.1:31080`；直接运行后端可用 `DEVFLOW_PORT` 指定另一回环端口，Host/Origin 校验同步跟随，用于不打断现有会话的隔离验收。Windows 双击启动器仍以 31080 作为固定健康检查端口。

## 开源复用与验证记录

### Node.js 聚合服务验收（2026-10-19）

- 用户明确授权自行验证后，`cargo fmt --all`、`cargo test --workspace`（47 项通过，2 项既有实机测试忽略）、`pnpm web:check`、`pnpm web:build` 通过；新版后端构建在 `target/node-runtime`，避免覆盖正在运行的旧实例。
- 可重跑 `python scripts/verify-node-runtime.py --real-project`：隔离的中文/空格路径样例、HTTP/MCP 共用状态、13 个真实聚合别名识别、鉴权/版本/脚本白名单、并行服务 PID/端口隔离、增量/有界日志、失败退出、取消启动、系统进程退出检查及 npm/pnpm/yarn 参数传递共 36 项通过。pnpm/yarn 验收工具仅安装在忽略的 `.devflow/acceptance/node-tools`，不改项目依赖。
- 真实 `D:\CompanyProject\main-system` 的 `file` 脚本通过 Vite 启动并返回 HTML，停止后所关联进程树在操作系统中全部退出，原有 8081 前端仍可访问。未重启原 31080 实例及其业务会话。
- 浏览器实际验证了 Node 卡片直接启动、启动中状态、端口/日志显示、配置参数保存与下次启动生效、输出工具栏停止，以及 450px 分屏布局；截图位于 `.devflow/acceptance/node-runtime-ui.jpg`，真实聚合列表见 `.devflow/acceptance/node-main-system-ready.jpg`。隔离实例使用 31081，验收结果与真实 Vite 日志由 `.devflow/acceptance/node-latest.json` 索引。



### Java 运行链路验证（2026-10-19）

- `cargo test --workspace` 共 37 项通过、2 项既有实机测试默认忽略；`pnpm web:check`、`pnpm web:build`、`cargo build --workspace` 通过。新版后端已启动，当前工作区恢复为 `D:\WorkSpace\framework.code-workspace`。
- 可重跑脚本为 `scripts/verify-java-runtime.ps1`，使用独立真实 Spring Boot 3.5.14 样例，17 项 HTTP/MCP 验证通过：中文/空格路径、Profile 候选与实际激活、JVM/应用参数、stdout/stderr、Java PID 筛选、增量日志、跨入口重复启动防护、错误 PID、过期版本、官方 Arthas 附加及 HTTP 控制台、MCP 停止 Web 启动的进程、父子进程清理、新会话及构建阶段取消。样例和 JSON 证据保存在 `.devflow/acceptance/Java 中文 空格/`；脚本 finally 恢复先前工作区并清理测试运行。
- 内置浏览器实际填写启动参数并启动，输出确认 `READY|qa|qa-value|页面 中文 空格|页面参数 空格`；点击卡片保持详情折叠，箭头可单独展开；页面挂载 Arthas 后展示控制台链接，页面停止后测试主/子进程均退出。截图：`.devflow/acceptance/java-launch-dialog.png`、`java-runtime-ui.png`。现有业务 Java 服务未停止或重启，业务模块完整启动仍需使用实际依赖和环境配置验收。
- Windows 实机修正了 Java classpath 的 `\\?\` 路径前缀、NIO socket 临时目录兼容问题，以及进程在停止瞬间已退出导致的权限误报。Arthas 启动器来自 [官方项目](https://github.com/alibaba/arthas)，挂载结果结合端口就绪确认，不只依据启动器退出码。

### 既有工作区验证记录

- 工作区字段、名称和相对路径语义参考 [VS Code 官方实现](https://github.com/microsoft/vscode/blob/main/src/vs/platform/workspaces/common/workspaces.ts)，未复制其工作台运行时。
- JSONC 解析及保留注释的编辑直接使用 MIT 许可的 [dprint/jsonc-parser](https://github.com/dprint/jsonc-parser)；本地 URI 复用 [url](https://github.com/servo/rust-url)，随机标识复用 [uuid](https://github.com/uuid-rs/uuid)。
- 已只读核对 `D:\WorkSpace` 的 9 个文件，共 46 个文件夹引用；其中 `ai-chat` 有 3 个当前不存在的目录，验收应显示不可用，其他引用目录当前存在。
- 已补充测试源码：JSONC/中文/相对路径、URI、配置与注释保留、另存为路径、鉴权、版本冲突、重复目录、未保存保护、外部修改和符号链接越界；另有默认忽略的 Windows 实机只读样本验收。
- 已完成 Rust 格式化、4 个 TypeScript/TSX 文件的语法解析、PowerShell 启动脚本语法解析及 Git 差异空白检查；这些检查不执行编译或业务逻辑。
- 2026-10-19 用户授权启动与验收后，已通过 `cargo fmt --all -- --check`、`cargo test --workspace --locked`（16 项通过，1 项实机测试默认忽略）、单独执行的 Windows 实机测试（9 个工作区全部通过）、`cargo build -p devflow --locked`、`pnpm web:check` 和 `pnpm web:build`。
- HTTP/MCP 端到端验收共 56 项检查通过，覆盖 9 个文件的目录与状态、双入口状态一致、新建/添加/保存/另存为、JSONC 保留、失效路径、鉴权与来源校验、版本及外部编辑冲突。另验证了 Vite 代理鉴权和运行指标跟随当前工作区。真实工作区文件验收前后的 SHA256 全部一致。
- 浏览器已操作验证打开现有工作区、新建文件、添加中文目录及显示名称、保存、未保存保护、MCP 修改后的并发提示和直接打开文件夹；检查了 400px、450px、550px 的实际 iframe 分屏布局。发现并修正空工作区提示/添加入口以及浅色主题下管理窗口标题颜色。
- 本机验收脚本、JSON 明细和截图保存在 `.devflow/acceptance/`（忽略目录，不提交）。服务当前打开 `main-system.code-workspace`，Web 地址 `http://127.0.0.1:5173/`，API/已构建页面地址 `http://127.0.0.1:31080/`。该记录仅表示工作区管理 a 的本地功能验收，项目启动、Arthas 和完整 MCP 客户端协议兼容不在本轮验收范围。
- 后续左侧树与按钮状态修正：后端测试共 18 项通过（实机样本仍默认忽略），前端类型检查/构建通过；实测 HTTP/MCP 目录子项一致、产物目录被排除，以及页面多层展开、方向键折叠和运行项目显示“停止”。当前服务已恢复重启前的 `framework-master` 工作区。
- 工作区入口与交互调整：完成源码修改、Rust 格式化、差异空白检查，以及运行中前端的顶部搜索、菜单分离、对话框、450px 窄屏、鼠标拖拽、键盘调宽、宽度记忆和双击复位检查。新增浏览边界、原子创建及关闭保护测试源码；依照默认不编译约定，本轮未执行编译或自动化测试命令。新增后端接口需重新编译并重启后才能端到端验证。
- 随后关闭操作的 HTTP 422 已确认来自仍运行的旧后端：其请求枚举不包含 `close`。用户明确授权后，已编译并重启新版后端，通过 Rust 测试 30 项（2 项实机测试维持默认忽略）、前端类型检查/构建及原有 56 项接口验收；新增 8 项接口检查验证了浏览的 HTTP/MCP 一致性、双入口从文件夹创建、关闭、无工作区时浏览及文件保留。浏览器实际点击关闭成功，HTTP/MCP 均返回空工作区，原 Java 服务进程仍在。当前页面保留已关闭状态；截图为 `.devflow/acceptance/workspace-close-fixed.png`。前端也增加旧后端不支持新增操作时的明确版本提示，避免仅显示 HTTP 422。
- 窄屏交互、根引用移除与自身指标调整：源码已补充，已完成 Rust 格式化、差异空白检查；浏览器验证桌面及 450px 真实鼠标拖拽、拖拽后的树节点折叠、右键菜单打开，并检查 400px 顶栏与资源树布局。新增了移除引用的 JSONC/重复引用/失效目录/版本保护和自身指标归一化/缓存测试源码。本轮新增后端功能的编译、运行验收仍等待明确授权，暂不把指标占位值或菜单显示视为后端验收通过。
- 用户随后授权编译重启：Rust 常规测试 33 项通过（2 项实机测试维持默认忽略），前端类型检查/构建通过；已编译并启动新版后端。新增 20 项 HTTP/MCP 验收通过，覆盖移除后的根/服务/dirty 同步、Windows 路径大小写、嵌套目录拒绝、版本冲突、未保存保护、失效目录清理、JSONC 保留、保存重开、磁盘文件保留、自身 PID 校验、CPU 预热与归一化、鉴权及关闭工作区后的指标可用。浏览器在临时工作区实际右键移除成功，并核对 HTTP/MCP 状态和文件保留；400px 分屏中实际拖拽从 327px 调至 247px，随后树节点展开与更多菜单仍可用，真实 CPU/内存与搜索同排显示。已恢复用户的 `framework.code-workspace`，真实工作区文件未被改写。脚本与结果位于 `.devflow/acceptance/tree-metrics-verify.ps1`、`tree-metrics-result.json`，截图为 `tree-remove-ui-passed.png`、`tree-metrics-ready.png`。

## 可启动服务识别

右侧列表展示可启动应用，不把构建描述符、聚合模块或依赖库直接当成服务；左侧目录树仍展示完整工作区。

- Maven 使用 [roxmltree](https://github.com/RazrFalcon/roxmltree) 解析 POM，通过声明的 `modules` 有界遍历。`packaging=pom` 只用于找到子模块；仅有 Spring Boot 依赖或插件不足以成为服务。
- Java 入口复用 [Tree-sitter Java](https://github.com/tree-sitter/tree-sitter-java) 的语法树识别：应用类必须有真实 `public static void main(String[]/String... args)`，并有 Boot 应用注解或启动调用；也支持 POM 实际插件配置明确指定的普通 Java mainClass。排除注释、字符串中的假入口和 `src/test`，不读取依赖 JAR、target 或生成的 class 文件。
- 此设计参考 [VS Code Spring Boot Dashboard 的主类解析](https://github.com/microsoft/vscode-spring-boot-dashboard/blob/main/src/BootApp.ts)。该扩展依赖 VS Code/JDTLS，不能直接嵌入本系统；这里复用开源解析器进行本地源码发现，不宣称具有 Java 编译器的完整类型解析能力。
- Node 工程识别非空 `dev`、`start`、`serve` 及其带冒号的变体，并有界追踪 npm/pnpm/yarn 对这些脚本的启动别名；build/test/lint/check/scan 等任务和循环别名不进入服务列表。聚合根工程存在启动别名时，每个别名作为独立 Node 服务展示，共用根 package.json 路径，以兼容新增的 `nodeScript` 区分身份；不要求子项目有独立 package.json，不扫描 node_modules，也不把它当作完整 npm workspace 图解析。
- `mainClasses`、`launchScripts` 作为兼容的可选响应字段返回，项目详情显示识别依据。状态未知或新服务尚未采样时，操作按钮显示“状态未知/读取中”，不误显示“启动”。
- 默认 Java 源目录为 `src/main/java`，支持本 POM 的 `build/sourceDirectory`；继承模块属性和解析默认激活 profile 的 modules，不解析任意 profile 条件、Gradle、Kotlin 或自定义构建生成入口。超出边界、无法解析或达到预算时返回 warnings，不执行构建猜测。
- 每个导入目录的服务发现缓存 30 秒，页面手动刷新及 MCP `workspace_refresh` 立即重查。最多 512 个模块、32 层目录、50000 个源码目录项、20000 个文件及 64 MiB 总读取预算；单 POM/package.json 2 MiB，单 Java 源文件 512 KiB。
- 实机 `D:\CompanyProject\extreme-plus` 识别到 14 个有源码启动入口的模块：12 个 extreme-module 业务模块，以及同样有真实 Boot main 的 common-docker、third-oa；不会根据 common/third 目录名武断隐藏真实入口。普通 API/common 库和聚合工程不再进入服务列表。
- 本轮验证：Rust 常规测试 27 项、实机测试 2 项全部通过，前端类型检查与构建通过，原有 HTTP/MCP 工作区验收 56 项全部通过。当前 framework-master 工作区共 28 个可启动服务；HTTP、MCP 查询和强制刷新列表一致，浏览器确认 System 服务显示“停止”及真实启动主类。截图见 `.devflow/acceptance/runnable-services-ready.png`。

## 工作区只读分享

工作区菜单新增只读邀请。默认访客使用独立局域网端口 31082，本机管理继续绑定 127.0.0.1:31080；邀请固定目录范围、可过期/撤销，允许文件、搜索、Git Diff 和服务日志读取，拒绝所有写入操作。详情、接口、配置和待验证项见 [只读分享](workspace-sharing.md)。本轮未编译或运行测试。

只读分享页面现完全复用本机 `App` 工作台及目录树、搜索、编辑器、Git、服务卡片和输出面板，只按只读模式隐藏写入与运行控制。原精简访客页面已移除；保持核心只读授权，详见只读分享文档。此次未编译或运行测试。

### 运行输出对应服务高亮

- 服务卡片以底部当前运行输出的 `projectKey` 为选中依据，复用 TaskRows 的受控活动行属性；点击其他服务、通过菜单查看输出或启动服务切换输出时，光晕随输出目标移动，关闭输出后消失。Node 同目录下不同脚本按独立运行键区分。
- 选中卡片使用主题强调色细边框、轻微底色与柔和内外光晕，无循环闪烁或额外动画；按钮通过 `aria-current` 表达当前目标。仅修改源码并检查差异，未编译或运行测试；待验收明暗主题、切换服务和关闭输出后的高亮状态。

### Git 历史浏览

Commit 顶部 History 入口在独立工具标签打开提交图、仓库/分支筛选、提交信息搜索、分页、文件树与提交说明；根提交和合并父节点可正确选择比较基线，文件点击使用现有 Diff 展示不可变历史内容，历史视图无工作区还原。HTTP/MCP/只读分享共用核心，未增加依赖或自动 fetch。详细接口、限制与未执行测试的验收说明见 [工作区 Git 管理](workspace-git.md#git-history)。

行号右键现可按需开启轻量行提交信息（日期与作者），仅点击批注才定位 Git History 提交详情；复用 Web/MCP 核心与有界 Git CLI，无后台索引。限制与待验收项见 [工作区 Git 管理](workspace-git.md#轻量行提交信息blame)。本轮未编译或运行测试。

## 资源管理器紧凑目录

- 文件管理与 Git 提交树共用 `components/files` 的紧凑目录判定、树行、箭头、路径段标签与基础键盘导航；文件管理继续负责异步按需读取与文件操作，Git 使用已有变更内存树。两处的“紧凑文件夹”按钮共享浏览器偏好，兼容原有设置，不新增后端接口。公共层边界见 `apps/web/src/components/files/README.md`。
- 默认合并连续的单子目录，例如 `src/main/java`；工作区根目录保持独立，只有一个文件的目录不合并，遇到多个子项、文件或截断结果立即停止合并。标题栏“紧凑文件夹”按钮可关闭，偏好保存在当前浏览器。
- 每个路径段可点击选中、右键执行现有目录操作，复制按钮针对当前选中段（未选中时为末段）；悬停任意段按 C 复制对应绝对路径。树节点聚焦时 Alt+左/右切换段，菜单键或 Shift+F10 操作当前段。
- 已确认没有可见子项的目录保留图标和名称，移除展开箭头及空提示行；未读取、失败或截断的目录不判空。关闭紧凑模式后仍在首次展开读取后判空。
- 仅探查当前可见目录及其单子目录链，每行最多探查 32 层，之后保留下一层供继续展开；不递归扫描兄弟目录或被后端忽略的目录。行内缓存读取结果，折叠/卸载取消在途请求，文件变更刷新所涉及路径链，刷新目录树或工作区修订变化重建缓存。HTTP/MCP 核心目录接口及权限规则保持共用，紧凑模式属于 Web 展示偏好。
- 本轮仅修改代码与文档，未执行编译或测试。待人工验收：中文与 Windows 路径、多根目录、单子目录链、单文件目录、空目录、截断/错误重试、路径段复制与右键、模式切换、刷新及折叠状态恢复。

Git 展示读取改为核心内存快照与按仓库后台刷新；History 使用独立仓库目录接口。HTTP/MCP/只读分享复用相同规则，本机与分享共用 32 MiB 历史内容 LRU 和 16 MiB 状态内容估算预算，授权缓存键隔离。详细行为与未运行的验证场景见 `docs/features/workspace-git.md` 的“Git 扫描与内存缓存”。

Commit 面板修复冷启动加载占位误报 Merge Conflicts，添加到 VCS 后显示进度/成功数量并强制同步列表；Unversioned Files 的目录支持右键按已列出的文件路径添加。状态解析限制对象数量，冷启动判断移除额外快照复制；前端复用未变化数组、卸载折叠后代与隐藏文件树。已补测试源码，未编译或运行测试，实际行为及内存下降待验收；History 优化留待后续。


Git Update Project 与三栏冲突合并：Git 面板新增拉取并合并上游的入口；后台任务、三方文本处理、整文件选择、完成/中止合并共享 devflow-core，并同时提供 HTTP/MCP。详见 [工作区 Git 管理](workspace-git.md) 的“Update Project 与三栏合并”。首版要求干净暂存区和已跟踪工作区，未实现 Rebase/自动 Stash；按约定未编译或运行测试，等待用户本机验证。


## 工作区交互终端

- 底部状态栏新增“终端”，`Ctrl+反引号` 打开或收起。终端标题栏 `+` 在单个可用工作区文件夹时直接创建，多个时先列出文件夹名称与绝对路径；工作目录通过 PTY 的 cwd 设置，不拼接 `cd` 命令，支持中文、空格与 Windows 路径。
- 使用与 VS Code 同类的 xterm.js + 系统 PTY 架构。参考 [terminalInstance.ts](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/terminalInstance.ts)、[xtermTerminal.ts](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/xterm/xtermTerminal.ts) 与 [terminalProcess.ts](https://github.com/microsoft/vscode/blob/main/src/vs/platform/terminal/node/terminalProcess.ts) 的展示/进程分层，未复制 VS Code 源码。Rust 使用 portable-pty 0.9（Windows ConPTY）；前端增加 @xterm/xterm、@xterm/addon-fit，依赖锁文件已同步。
- Windows 默认使用系统 Windows PowerShell，初始化控制台输入、输出及管道 UTF-8；类 Unix 使用服务端 SHELL 或 /bin/sh。支持真实交互输入、Ctrl+C、光标与 ANSI/256 色/真彩色输出；输出颜色由命令本身发出的 ANSI 控制序列决定，不对普通文本做日志级别着色。不包含 VS Code shell integration、命令导航、终端分屏或远程终端。
- 每个终端独立进程与输出，右侧列表切换，窄屏改为横向列表；明暗主题同步，拖拽底部面板自动同步 PTY 行列。清理按钮清除当前渲染器滚动历史，垃圾桶终止进程树并关闭会话。收起面板保留会话；刷新网页可重新列出进程并重放保留输出。切换/关闭工作区关闭全部终端，删除文件夹关闭对应终端，保存和添加文件夹保留仍被授权的终端。服务正常退出时显式清理，异常强制结束宿主的情况仍需本机验收。
- 核心业务统一在 `WorkspaceService` 的终端服务内。HTTP `POST /api/workspace/terminal` 与 MCP `workspace_terminal` 保留原有 `expectedRevision` 和 `operation`（`list/create/read/input/resize/close`）；WebSocket 输入/尺寸更新复用相同内部执行逻辑。创建只接受当前工作区显式声明且 canonicalize 后在授权根内的可用文件夹，不接受客户端指定 shell 可执行文件或启动参数。工作区修改持锁更新会话成员关系，实时操作持同一工作区锁检查 revision 和存活会话；输入/尺寸更新重新授权当前终端目录，避免逐键解析和检查全部工作区文件夹或收集全部会话状态。终端不挂载到局域网只读分享监听器。
- 此功能按用户要求提供本机交互 shell：工作区校验约束初始 cwd 与会话访问，不是 shell 文件系统沙箱，shell 中的命令以当前本机用户权限运行。MCP 工具声明为具有副作用和破坏可能性的能力。
- 最多 12 个会话，每个保留 1 MiB 原始输出字节。HTTP/MCP 读取仍每次最多 64 KiB，返回 `data: number[]`、`nextCursor`、`truncated`、`exited`、`exitCode`。Web UI 使用 `GET /api/workspace/terminal/socket` 升级为 WebSocket，取消输出的 120ms/800ms 定时轮询；PTY 追加输出时通过 watch 通知订阅者，通知可合并，实际输出按 cursor 从有界缓冲读取，等待期间不持工作区锁。每秒只做一次修订/会话生命周期检查，确保无输出或暂停确认时也能发现失效会话。
- WebSocket 协议：首条 JSON 为 `{token, expectedRevision, id, after}`，认证成功返回 `ready`；随后客户端发送 `input`、`resize`、`ack` 控制消息。服务端输出二进制帧为 **8 字节大端 nextCursor + 最多 64 KiB 原始 PTY 字节**，其他消息为 `reset`、`input_ack`、`input_error`、`exit`、`error`。xterm 消费完每帧后才提交游标并 ACK，服务端最多保留 256 KiB 未确认输出，超过窗口暂停发送；后端仍继续读取 PTY 到 1 MiB 环形缓冲，慢客户端溢出后收到截断提示，此流控不阻塞 shell。保留跨块 UTF-8 和 ANSI 序列。
- 连接继续验证本机 Host/Origin，升级请求必须携带允许的 Origin；令牌只在首帧传递，不写入 URL，认证前不访问终端业务，5 秒内未认证断开。最多 24 条实时连接，限制消息尺寸和发送等待时间。Vite `/api` 代理开启 `ws`。输入使用原有独立线程和有界队列；前端按 Unicode 字符分块（每块最多 4096 个码点），最多排队 256 KiB，收到 `input_ack`（表示已进入后端输入队列，不代表 shell 已执行）后发送下一块；后端队列满时取消剩余粘贴并提示。连接中断不自动重放输入。
- 仅当前可见且页面前台的终端建立实时连接；切换、展开或回到页面时立即连接，断线使用退避重连，并从 xterm 已消费游标补读；断开连接不关闭 shell。检测到 shell 退出且当前缓冲已发送完时通知退出码，继续接收可能晚到的 PTY 尾部输出，读到 EOF 后关闭连接（兼容 ConPTY 在 shell 退出后仍保持管道打开的情况）。尺寸更新按帧 fit，并在 32ms 防抖后发送；移除强制启用的 screenReaderMode。
- 已添加测试源码：中文/ANSI 跨块字节保留、有界缓冲区与游标、令牌/修订/精确工作区根目录校验、原生 PTY 中文空格路径、交互输入、调整尺寸与关闭工作区时的进程清理。本轮按要求未运行编译、类型检查或测试，仅更新依赖锁文件并检查源码差异。
- WebSocket 改造补充测试源码：实际升级连接的 Origin 拒绝及首帧认证顺序、输出通知的读/等待竞态和合并、实时接口鉴权/修订/关闭校验、原生 PTY 的订阅与补读、发送窗口上限/无效 ACK/截断游标跳跃。仅同步 Cargo 锁文件并静态检查源码，按项目约定未运行编译、类型检查或测试。
- 待本机验收：多文件夹 `+` 选择与 cwd；中文输入和输出；PowerShell/Node/Maven ANSI 输出；Ctrl+C 中断前台子进程；长输出时的输入响应、粘贴限流与退出尾部输出；多会话切换、收起再打开、页面后台恢复、断线/刷新重连且无重复输出；宽窄屏与主题；删除文件夹、切换工作区和退出 DevFlow 后的进程树清理；未授权、恶意 Origin、过期修订和无效 ACK 被拒绝；只读分享无终端入口。

终端 Windows cwd 修正：授权与会话归属继续使用 canonical 路径，传入 shell 的 cwd 使用普通盘符/UNC 路径，避免 Windows PowerShell 将 `\\?\` 路径显示为 `Microsoft.PowerShell.Core\FileSystem::...`。已有会话需关闭后新建才使用修正后的 cwd；未编译或运行测试。
