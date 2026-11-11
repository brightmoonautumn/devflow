# AI 上下文的渐进式读取

2026-11-07：三批源码优化覆盖看板/工作区、QA/诊断、文件/日志/终端与 MCP 工具说明。当前仅完成代码和静态审查；未编译、运行项目、执行测试或测量模型 token 用量。

## 双入口与兼容

- MCP 沿用原工具名和业务参数，新增可选的顶层 `read` 对象。省略时返回精简视图；`read: { "view": "full" }` 恢复该工具原有完整结果。需要完整数据的旧 MCP 调用方应显式指定 full。
- 现有 Web HTTP 路由和 DTO 保持完整，网页交互不需要迁移或丢弃历史数据。需要精简读取的 HTTP 客户端可用已认证的 `POST /api/context/read`，请求为 `{ "tool": "board_get", "arguments": { ... } }`，调用与 MCP 完全相同的 `DevFlowEngine::context_read`。
- 新 HTTP 入口只接受核心显式列出的只读工具，不能通过工具名调用写操作；仍检查本机令牌、工作区成员与原有版本边界。不对共享访客开放此入口。
- 精简视图是展示投影，不修改存储格式、任务状态机、认领归属、人工验收、脱敏或冲突保护。看板、QA、诊断写操作默认返回回执和新版本；`read.view=full` 恢复原快照/会话。写入响应选项在业务操作前验证。
- 后端 HTTP MCP 保留 `structuredContent` 与 JSON 文本以兼容既有调用方。本机 stdio 连接器仅在两份内容等价、只有一个文本块且非错误时去除重复 `structuredContent`，保留完整 JSON 文本；CLI 仍读取结构化数据。工具没有声明 outputSchema；未来添加时须同时调整此策略。媒体、错误及不等价内容不被裁剪。

## 默认读取与展开入口

| 工具 | 默认结果 | 按需展开 |
|---|---|---|
| workspace_get | 身份、根目录、版本、项目数量 | workspace_list 项目摘要；full 原快照 |
| workspace_list | 30 个项目的名称/类型/路径/nodeScript | java_options/node_options；full 原数组 |
| board_get | 状态总数、分页功能名称和任务摘要 | detail+id 任务及所属功能；history+id 任务活动；history 无 id 为看板活动；section=feature 单独分页功能；detail+section=feature+id 功能详情 |
| qa_get | 分页场景目录、运行状态及步骤/通过/失败数量 | detail+id 场景；history+id 运行步骤结果；detail+section=run+id+itemId(callId) 指定步骤请求响应 |
| diagnosis_get | 会话状态、进展、证据数量 | detail+id 问题/结论；history+id 证据预览；detail+section=evidence+id+index 证据正文 |
| runtime_list | 分页服务状态、主 PID、端口 | read.id 按 projectPath 筛选；detail 为进程指标 |
| workspace_children / workspace_search | 30 个目录项/匹配 | 分页；搜索原有 200 条/5 秒限制仍生效 |
| workspace_file_read | 最多 80 行、6000 Unicode 字符 | 行/字符续读；full 精确全文 |
| java_output / node_output | 状态及最多 50 行、6000 字符 | 游标续读；status 不返回日志；full 原最多 500 行页 |
| runtime_diagnostics | 脱敏服务定位信息，不附日志 | detail 分段日志节选；full 原结果 |
| workspace_terminal | 小段 UTF-8 文本，不回传十进制字节数组 | read.raw=true 或 full 恢复原始字节；原终端 WebSocket 不变 |

默认任务摘要不带描述、验收正文、交付正文、活动、布局坐标或连线；任务详情不带活动全文。功能和任务数据均保留在原存储中。看板摘要可用 `read.feature`（空字符串为未归类）、`read.status`、`read.query` 筛选，查询匹配任务编号/标题/描述，结果保留人工顺序。`counts` 是整个看板的数量，`tasks.total` 是筛选后的数量；`filters` 明示筛选条件，不默认隐藏已完成任务。

`board_execute` 回执包含 `saved/revision`、受影响 ID 和最小任务/功能状态，创建任务时直接返回新 ID。`qa_execute` 回执包含配置版本和 `latestRun`；`diagnosis_execute` 包含会话 ID/版本/状态/进展。回执不包含其他任务、历史报告或旧证据。

## 分页与轮询

- 数组页统一包含 `items/total/offset/nextOffset/hasMore`。默认 30 项，`read.limit` 范围 1–200；文件和日志分别默认 80/50。
- 数组页同时按序列化字符预算提前结束，避免一次读取几十条长活动/搜索结果；不拆分单个记录，至少返回一项以保证前进，单个超长记录可能超过软预算。功能与任务各自分页，任务摘要附带 featureName，不要求先拉取全部功能。
- 精简读取返回 `snapshotId`。下一页带回同一个 `read.snapshotId` 并保留筛选，数据变化时返回 conflict，重新读取第一页，不能混用两次快照。它是完整来源内容的稳定指纹，不是安全凭据或修改授权。
- 同工具、同视图和同筛选的重复轮询可带 `read.ifSnapshotId`；来源未变化仅返回 `{ "snapshotId": "...", "unchanged": true }`。切换目标或展开新视图时不带 ifSnapshotId。QA 运行进展和诊断证据也参与指纹，不依赖配置版本判断运行变化。
- 历史活动按新到旧；证据按原始顺序，`read.index` 为从 0 开始的原始证据索引。
- QA exchange、诊断 output、排查 excerpt 返回文本页：`text/totalChars/offset/nextOffset/hasMore`。`read.offset` 在这些正文中是字符偏移；QA exchange 是完整步骤的 JSON 字符串分段，单个分段不保证独立构成 JSON。
- `read.maxChars` 限制正文/日志字符数，默认 6000，范围 256–24000；它不是整个 JSON 响应或精确 token 的硬上限。
- 文件 `read.offset` 是从 0 开始的行偏移；继续使用返回的 `nextOffset` 和 `nextCharOffset` 作为 `read.offset/charOffset`，并带 `snapshotId`。长行也能分段前进。片段不能作为写文件的完整 `expectedContent`，写前必须 full 精确读取。
- 运行日志继续使用 `runId` 和 `after=nextCursor`，并带 `read.charOffset=nextCharOffset`。长行未读完时 nextCursor 保持在该行之前，字符游标前进，不丢弃剩余文本。运行切换或 `truncated` 时重置游标；正在变化的日志流使用 runId/游标，不跨页固定 snapshotId。
- 终端文本页预算按源字节保守截取，不切断正常连续 UTF-8 字符；`nextCursor` 仍按实际消费的 PTY 字节计算，ANSI 展示裁剪不改变游标。文本是尽力可读展示，精确终端渲染继续使用 raw/WebSocket。

## 常用调用

核对工作区后读取单个任务：

```json
{
  "workspaceKey": "D:/Work/example.code-workspace",
  "expectedRevision": 3,
  "read": { "view": "detail", "id": "DF-001" }
}
```

按功能定位待办，再按需展开相关任务：

```json
{
  "workspaceKey": "D:/Work/example.code-workspace",
  "expectedRevision": 3,
  "read": { "feature": "实际功能 ID", "status": "todo", "limit": 20 }
}
```

HTTP 使用同样的参数与投影：

```json
{
  "tool": "diagnosis_get",
  "arguments": {
    "projectPath": "D:/Work/service",
    "expectedRevision": 3,
    "read": { "view": "detail", "id": "实际会话 ID", "section": "evidence", "index": 0 }
  }
}
```

编辑任务先读 detail 的完整元数据；编辑 QA 套件先读 full 的完整 suite，不要用摘要重建写入对象。`read` 位于 MCP 工具参数最外层，不能放在 board_execute.request 或 operation 里。

## 待用户运行验收

核对分页/筛选/并发变化提示、单任务认领与新版本续写、QA 运行轮询与失败交换详情、诊断证据续读、中文/CRLF/超长行续读、日志窗口截断、终端中文与原始模式，以及 full 模式与旧 Web 页面兼容。源码静态检查不能代替 Rust 编译和真实 MCP 客户端验收。
