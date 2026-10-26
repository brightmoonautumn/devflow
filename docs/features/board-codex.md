# Codex 与 DevFlow 看板联动

DevFlow 保存功能、任务、状态、进展和交付；Codex 在你当前打开的项目里完成代码工作，再通过 MCP 更新看板。看板认领不会启动 AI，也不会让后台自动运行任务。

## 本机配置

本机 Codex 全局配置 `C:\Users\Administrator\.codex\config.toml` 已注册名为 `devflow` 的 stdio MCP：

```toml
[mcp_servers.devflow]
command = "D:/DevelopmentTools/Python/Python314/python.exe"
args = ["-u", "D:/PersonalProject/devflow/scripts/devflow-client.py", "mcp"]
enabled = true

[mcp_servers.devflow.env]
PYTHONIOENCODING = "utf-8"
```

以上是本机实际路径，其他机器需替换。配置前已在 `.codex/backups/` 保存原配置。无需新增第三方依赖，无需把 token 写入配置。原有 MCP 与其他 Codex 设置保持原值。

连接器固定访问 `http://127.0.0.1:31080`，按现有本机 session 规则取 token，仅保存在进程内存中；后台返回 401 时重新获取一次。不会自动重试执行失败或版本冲突的业务操作，避免重复写入或重复启动服务。不能用于远程或云端 Codex；DevFlow 和本地 Codex 必须运行在同一台机器。

1. 启动 DevFlow，并在网页打开需要工作的工作区。
2. 在 Codex 打开对应源码项目。
3. 重启 Codex 后新开对话，让它加载 `devflow` MCP；也可使用设置中的 MCP 重启入口。当前已经打开的会话不保证动态加载新增工具。
4. 让 Codex 先读取当前工作区和看板，核对工作区包含你当前项目。注册 MCP 本身不授权修改任务、切换工作区或执行代码。

如果以后移动仓库或 Python 安装目录，需要更新上述 `command/args`。

## 可以直接发送的提示词

先粗略记录，再优化原任务：

> 读取 DevFlow 当前工作区看板，核对项目后记录一个待办：优化启动失败提示。只记录标题，不归类、不认领、不执行。

> 优化 DevFlow 的 DF-001，结合已有功能和任务，提出描述、所属功能、验收条件和必要的拆分建议。信息不足时列出问题，等我确认后再更新原任务，不认领、不执行、不编译或测试。

Web 任务详情提供「复制 AI 整理指令」。`task_create` 最少只需要 `title`；`feature` 省略或空字符串表示未归类，其余默认空描述、空验收、普通优先级。`task_update` 仍提交完整任务元数据，避免省略字段意外清空。可选的 Gitee 同步通过 `board_sync` 读状态、配置和手动推送；恢复由用户在网页确认。同步配置与凭据说明见 [看板](board.md)。

先讨论，达成共识后建任务：

> 把刚才确认的方案写入 DevFlow 当前工作区看板，整理成一个功能和几项可验收任务。只记录核心描述、目标和关键约定，先不要执行。

执行指定任务：

> 读取 DevFlow 看板，确认工作区与当前项目一致，然后认领 DF-001 并执行。持续记录重要进展，完成后提交真实交付说明和验证情况到待验收，不要自行通过验收。不要编译或运行测试。

按范围连续推进：

> 读取 DevFlow 中“任务看板”功能下的待开始任务，逐项认领并完成；遇到阻塞记录原因并停下。每项完成后提交待验收，验收留给我。不要推进其他功能，不要编译或运行测试。

暂停并交接：

> 把已完成内容、剩余工作、阻塞和接手注意事项写回当前任务，然后释放任务，停止继续执行。

新对话接手：

> 读取 DevFlow 的 DF-001 及其所属功能和活动记录，核对当前源码，再认领继续。沿用已有目标和约定，提交交付时明确哪些实际验证过、哪些没有验证。

你在网页查看交付，点击“通过验收”或填写原因“退回修改”。新会话不能凭借旧会话身份释放其任务；中断会话未释放时，你可以先在 Web 释放，然后让新会话认领。执行中每个 Codex 会话应持续使用自己的稳定唯一 `sessionId`。

## CLI 备用入口

在 DevFlow 仓库根目录运行：

```powershell
.\DevFlow-CLI.cmd workspace
.\DevFlow-CLI.cmd board
.\DevFlow-CLI.cmd tools
```

三个命令均只读。也可以从任意目录使用完整路径 `D:\PersonalProject\devflow\DevFlow-CLI.cmd`。默认从 PATH 查找 Python；可用 `DEVFLOW_PYTHON` 指定 Python 可执行文件完整路径。MCP 配置已直接使用本机 Python 绝对路径，不依赖此环境变量。

写入时先读取最新状态，再准备 UTF-8 JSON 文件，调用：

```powershell
.\DevFlow-CLI.cmd call board_execute --args-file D:\Temp\board-request.json
```

参数文件示例（认领，必须替换为实际工作区身份和最新版本）：

```json
{
  "sessionId": "my-stable-conversation-id",
  "request": {
    "workspaceKey": "D:/WorkSpace/example.code-workspace",
    "expectedRevision": 3,
    "boardRevision": 5,
    "operation": { "type": "action", "id": "DF-001", "action": "claim" }
  }
}
```

`tools` 输出当前后端的真实输入 schema。不要照抄示例版本，不要通过自动填充最新版本覆盖并发修改。CLI 与 MCP 只有协议入口不同，权限和状态流转均由后端同一核心检查。

## 本轮连接检查

本机 31080 后台返回 38 个工具，包含 `board_get/board_execute`。连接器已完成 stdio initialize、initialized 通知、tools/list、workspace_get 与 CLI board 读取检查。只检查连接，不创建功能或任务，不认领、不启动服务、不编译、不运行项目测试。结果保存在忽略目录 `.devflow/acceptance/devflow-connection.json`。

当前看板读取结果为空（版本 0），这是实际状态；如需沿用原型任务，请先在网页主动导入草稿。连接检查不能代替完整的看板业务验收。Rust HTTP MCP 仍是现有最小 JSON-RPC 实现，连接器不宣称完成全部 MCP 生命周期和传输协议合规。
