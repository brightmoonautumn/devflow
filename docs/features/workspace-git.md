# 工作区 Git 管理

## 范围与交互

左侧活动栏新增 Git 入口。面板参考 IDEA Commit 窗口：上方 `Changes`，下方 `Unversioned Files`，各区按仓库、目录分组，显示分支、数量和状态。每个仓库的复选框只选择本仓库文件；子模块有独立仓库分组。面板显示时每 5 秒刷新，可手动刷新；仓库状态变化会清除该仓库的旧选择和提交预览。

- **Add to VCS**：工具栏或文件右键菜单添加所选未管理文件，等同于暂存新增文件。添加后在 Changes 中重新选择提交。不会添加整个目录或静默加入其他文件。
- **Rollback**：列出目标文件并确认，将普通已跟踪文件的工作区、暂存区恢复到 HEAD。新增文件只取消暂存，保留磁盘内容；未管理文件不删除。重命名恢复原路径，若原路径已有文件则拒绝覆盖。子模块指针不执行递归回滚或切换子仓库 HEAD。
- **Commit**：勾选文件、填写信息，生成只读预览，点击文件可在右侧 Diff 标签中查看；再次确认才提交。仅提交勾选路径的当前磁盘内容，保留未勾选文件已有的暂存内容。Git hooks、签名和过滤器仍遵循本机 Git 配置；不提供自动 Push、Amend、Shelf、冲突解决或行级提交。
- **Diff**：HEAD 对比工作区磁盘快照；新增/删除文件一侧为空。宽编辑区使用 CodeMirror MergeView 左右对比，小于 680px 使用统一对比。复用现有语言加载和配色；不引入 Monaco。文本规范化 CRLF 为 LF，二进制、非 UTF-8 或任一侧超过 1 MiB 时只展示说明。预览比较的是磁盘内容，Git 过滤器和 hooks 可能影响最终提交对象。

## 子模块与多仓库

通过 Git 索引 mode `160000` 发现已初始化子模块，支持嵌套子模块及 `.git` 为文件的情况。父仓库显示子模块指针行，子仓库显示其内部的文件改动。子仓库也可以作为显式工作区根目录，按规范路径去重后仍保留父子关系。

1. 子仓库只有未提交文件改动时，父指针行只读，提示到子仓库分组提交。
2. 提交子仓库仅改变子仓库 HEAD，父仓库不自动提交；刷新后可单独勾选父指针提交。
3. 父指针与对应子仓库文件不能在同一提交计划中出现，防止子仓库提交后父指针与预览不一致。父仓库其他普通文件可以与子仓库文件分别生成提交。
4. 多仓库逐一提交，不是跨仓库事务。前面的成功提交保留；首个提交失败时停止并返回实际结果。先刷新检查结果，再决定后续处理。

未初始化子模块不会自动下载或初始化。detached HEAD、合并/变基/拣选/回退进行中的仓库拒绝提交，冲突文件不能勾选；先使用 Git/IDE 处理。

## 核心与入口

业务位于 `crates/devflow-core/src/workspace/git`，HTTP 与 MCP 使用同一个 WorkspaceService、Git 操作锁和提交计划。适配层只负责鉴权参数、反序列化和结果映射。

| HTTP POST | MCP 工具 | 功能 |
| --- | --- | --- |
| `/api/workspace/git/status` | `workspace_git_status` | 返回仓库、变化和仓库 version |
| `/api/workspace/git/diff` | `workspace_git_diff` | 按仓库 version 和路径读取差异 |
| `/api/workspace/git/mutate` | `workspace_git_mutate` | `add` 或 `rollback`，返回逐仓库结果 |
| `/api/workspace/git/commit/preview` | `workspace_git_commit_preview` | 固定文件快照与提交信息，返回 planId 和 Diff |
| `/api/workspace/git/commit/apply` | `workspace_git_commit_apply` | 校验并消费计划，执行选择提交 |

所有入口需要认证、当前 `expectedRevision`、活动工作区成员资格。文件选择携带 `repository/version/paths`。只授权工作区内的完整仓库根目录，不因打开了仓库中的某个子目录而向上扩大权限。

Git 自动检测已有 PATH 和 Windows 常规安装位置，可通过 `DEVFLOW_GIT_PATH` 指定已有可执行文件绝对路径。找不到时提供安装提示；不自动下载安装 Git。固定参数调用 Git CLI，不通过 shell 拼接。路径使用 NUL 分隔的 literal pathspec，支持中文、空格、通配符字符和以连字符开头的文件名。

## 冲突保护与预算

- 选择提交使用 Git 自身 `commit --only --pathspec-from-file` 临时索引流程，避免将用户未勾选的暂存内容一起提交，也不需要自行交换真实索引。
- 计划 5 分钟有效，内存中只保留最近一个。HEAD、porcelain 状态及文件元数据构成 version，另保存所选磁盘文件的原始字节；提交前再次检查精确字节和工作区 revision。外部 Git 操作或保存导致变化时要求重新预览。Git 与任意外部编辑器之间不存在跨进程文件系统事务。
- Web 在替换/回滚/提交前检查所选文件及重命名前路径的编辑器草稿、保存和读取状态；未完成保存时阻止。回滚后刷新已打开的干净文件标签。MCP 不感知浏览器内存中的草稿，依靠核心磁盘与版本校验。
- 不遍历 `.git/node_modules/target/build/dist`。普通工作区目录发现预算：深度 4、10000 目录项、64 仓库、20 秒；仓库内只通过索引发现子模块。未管理文件由 `ls-files --others --exclude-standard` 配合目录排除获取，提前剪枝依赖/构建目录。
- 仓库变化最多展示 2000 项，不完整或包含不安全路径时禁用该仓库修改。每批最多 32 仓库，每仓库最多 500 个选择，单文件快照最多 4 MiB，提交快照与预览总计最多 16 MiB。符号链接、越界路径、依赖/构建目录禁止操作。
- 每条只读 Git 命令最多 15 秒，修改及 hooks 最多 120 秒，输出最多 16 MiB、错误最多 128 KiB。Windows 隐藏命令窗口；超时复用已有进程身份校验和进程树清理。超时可能发生在 Git 已创建提交后，必须刷新确认，不自动重试提交。

## 依赖与验证状态

新增前端依赖 `@codemirror/merge` 固定为 `6.12.2`，已更新 pnpm lockfile，未放宽依赖发布时间策略。安装依赖后由用户自行编译验证。

本轮只修改源码、更新依赖锁、解析 TS/TSX 语法和检查差异；遵循项目约定，未运行编译、构建、测试或重启。新增测试源码覆盖 porcelain v2、中文与空格重命名、冲突、父指针与子仓库状态、literal pathspec、路径边界和认证/revision；Git 集成测试默认忽略，覆盖选择提交保留未选暂存内容、新增文件回滚保留磁盘、真实子模块独立提交、父指针分批提交及预览后修改阻止提交。测试尚未执行。

待运行验收：400–550px 面板/窄屏统一 Diff、宽屏左右 Diff、Git 缺失提示、普通文件新增修改删除重命名、右键操作、回滚确认、Git hooks/签名失败、多仓库部分成功、子模块和嵌套子模块、Windows 超时清理。收到明确授权后再运行对应构建和测试。

## 实现参考

使用 [Git porcelain v2](https://git-scm.com/docs/git-status)、[选择提交](https://git-scm.com/docs/git-commit)、[restore](https://git-scm.com/docs/git-restore) 与 [CodeMirror Merge](https://codemirror.net/docs/ref/#merge) 的公开接口。面板采用 IDEA 的 Changes/Unversioned 分组交互，参考 [VS Code Git 扩展](https://github.com/microsoft/vscode/tree/main/extensions/git) 的状态与命令划分；未复制整套 VS Code/IntelliJ UI 或引入其平台运行时。
