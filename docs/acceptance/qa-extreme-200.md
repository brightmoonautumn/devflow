# QA Extreme 200 验收记录

日期：2026-10-23（Asia/Shanghai）。以下结果来自 DevFlow 新实现的共享 QA 执行器发出的真实 HTTP 请求，不是前端模拟，也没有通过数据库直接写账号或角色。

## 交付结果

| 验收项 | 场景 / 接口证据 | 状态 |
|---|---|---|
| AC-QA-001 管理员成功登录 | 通用授权 / 管理员登录，POST login，HTTP 200 + code=200 + success=true + token 非空 | 已覆盖 |
| AC-QA-002 创建账号并赋予基本角色 | 通用授权 / 开通账号并赋予基本角色，查询现有角色、POST sys/account、查询 ID、POST batchAssignRole | 已覆盖 |
| AC-QA-003 新账号成功登录 | 系统验收 / 新账号登录并查看个人信息，以开户响应初始密码做 RSA 加密后登录，输出独立 token | 已覆盖 |
| AC-QA-004 查询个人信息与权限 | GET info?roleInfo=true，断言 ID、用户名、roles 包含基本角色且验收脚本额外确认不含 admin | 已覆盖 |
| AC-QA-005 多身份上下文隔离 | 最后用 adminToken 再查 info，断言用户名仍为 admin | 已覆盖 |
| AC-QA-006 Web/MCP 共用状态 | 第一轮 HTTP 启动，第二轮 MCP qa_execute 启动；qa_get 与 HTTP 可读取同一运行 ID，Web 显示 8/8 | 已覆盖 |
| AC-QA-007 认证/版本/凭证保护 | 无 token=401、旧工作区/QA revision=409、保存字面密码=400、报告中的密码/token 为 REDACTED | 已覆盖 |

## 最近一轮真实执行

- 目标站点：`http://192.168.1.200/extreme-plus-system-ai/`。
- API base：`http://192.168.1.200/prod-api/system/`。
- 工作区：`D:\WorkSpace\framework.code-workspace`。
- 执行 ID：`bb02e7cc35b8469998583bd6eff9e087`。
- 新账号：`qa_df_bb02e7cc35b84699`。
- 账号 ID：`2113554063655784450`。
- 基本角色：「测试」，编码 `cs_role_3035`，ID `2099691020244553729`。
- 结果：**8 个请求、37 条断言全部通过**。

| 顺序 | 真实接口步骤 | 执行身份 | HTTP | 断言 | 耗时 |
|---|---|---|---|---|---|
| 1 | 管理员登录 | anonymous | 200 | 4 条通过 | 235 ms |
| 2 | 查询基本角色 | admin | 200 | 6 条通过 | 55 ms |
| 3 | 创建账号 | admin | 200 | 5 条通过 | 187 ms |
| 4 | 读取新账号 ID | admin | 200 | 5 条通过 | 38 ms |
| 5 | 分配基本角色 | admin | 200 | 3 条通过 | 59 ms |
| 6 | 新账号登录 | anonymous | 200 | 4 条通过 | 226 ms |
| 7 | 查询新账号个人信息 | actorA | 200 | 6 条通过 | 32 ms |
| 8 | 查询管理员个人信息 | admin | 200 | 4 条通过 | 26 ms |

同一场景成功重复执行两轮，分别使用 HTTP 和 MCP 启动。第一轮账号 `qa_df_ecacc7715e9647f0`，第二轮账号 `qa_df_bb02e7cc35b84699`，均保留在线上供人工核验；未自动删除数据、未更改已有角色的权限。开户返回的初始密码用于新账号登录，不把密码或 token 写入验收文档。

## 编译与运行验证

- `pnpm web:check`：通过。
- `DevFlow-Restart.ps1`：最终两次启动均完成后端编译、前端 TypeScript/Vite 构建和 `/api/health` 检查；前端仍有现存大 chunk 提示。
- QA 核心定向测试 9 项：通过，覆盖引用去重/循环、目标边界、变量编码和数字类型、凭证脱敏、禁止保存字面凭证、认证/版本、中文路径存储和重启中断恢复。
- MCP QA 工具契约测试 1 项：通过。
- `cargo test --workspace` 已执行，当前 core 中有 6 项本次 QA 代码之外的失败，集中在既有 Arthas 标识校验、Java classpath 清单偏移、诊断/终端/分享测试的 Windows 路径授权。没有把全量测试标为通过，也没有为本任务修改这些运行时模块。
- 已确认第一次完成记录在真实重启后仍保留；前端公共场景能追溯引用执行历史。实际进行中运行被重启的恢复由核心测试覆盖；真实慢请求的超时/取消和多进程竞争仍需扩展验收。

## 查看与复现

Web：`http://127.0.0.1:31080/#qa`，选择「系统验收 / 新账号登录并查看个人信息」。最新记录显示 8/8；接口展开后可查看响应和断言，公共前置可点击追溯。

只校验已存在的成功记录（不创建账号）：

```powershell
node scripts/verify-qa-extreme.cjs
```

通过 MCP 再执行一轮（会新建一个唯一验收账号）：

```powershell
node scripts/verify-qa-extreme.cjs --run --mcp
```

管理员密码默认 admin；不同密码通过本次进程环境变量 `DEVFLOW_QA_ADMIN_PASSWORD` 提供，不写入脚本。脚本读取当前工作区、验证目标环境，先展开公共依赖，再轮询执行结果；同时验证跨入口一致性、认证/冲突和敏感字段脱敏。

本地详细证据在 `.devflow/qa-validation/accepted-run.json`、`acceptance-summary.json`、`qa-accepted.png`，均为脱敏记录，目录被 Git 忽略。核心场景定义在 `crates/devflow-core/src/qa/extreme.json`。没有新增独立 scenario-testing HTML 入口，沿用 DevFlow QA 工作台。
