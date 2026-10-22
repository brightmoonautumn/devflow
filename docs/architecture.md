# 架构

DevFlow 提供两个原生入口：面向人类的 Web UI，以及面向 AI 的 MCP。两个入口调用同一套核心引擎，共享业务规则、状态与权限校验。

## 职责边界

- `apps/web`：React UI，负责展示、输入和交互，通过 HTTP 使用业务能力。
- `crates/devflow`：HTTP 与 MCP 适配器，负责协议、参数和结果转换，不重复实现业务逻辑。
- `crates/devflow-core`：共享核心引擎，负责工作区、项目启动、进程状态和 Arthas 附加等业务能力。
- 服务启动时创建共享的核心服务实例，两个入口操作和查询相同的业务状态。新增业务能力同时规划 UI 与 MCP 的使用方式。

## 当前范围

当前包含 [工作区](features/workspace.md) 与 [看板](features/board.md)。启动和 Arthas 可继续使用内部 runtime、diagnose 模块。看板以工作区身份隔离，持久化、认领、交付与验收规则位于 `devflow-core` 的工作区服务，HTTP 与 MCP 复用同一事务入口。

工作区管理的是用户的 Maven/Spring Boot 和 Node 工程；根据实际工程配置识别模块及启动方式，不能假设目标工程是 Rust 项目。

## 访问边界

- 本地服务绑定回环地址；涉及进程操作前落实认证及权限校验。
- 当前发现入口使用 `DEVFLOW_WORKSPACE`。后续打开工作区文件或文件夹时，明确登记授权访问的目录；工作区文件所在目录与其引用的工程目录可能不同。
- 文件访问校验规范化后的路径和授权范围，防止符号链接、Windows junction 或路径穿越越界。
- 不通过 HTTP/MCP 暴露任意 Shell 执行或任意文件系统遍历；不递归扫描 `node_modules`、`target`、`.git`、`build`、`dist`。
