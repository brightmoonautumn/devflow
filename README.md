# DevFlow Starter

**简体中文 | [English](README.en.md)**

本地优先的开发工作台。采用 **Rust + Tokio + Axum + React + TypeScript + MCP** 技术栈，所有代码统一维护在一个仓库中。

> **当前聚焦工作区，并非完整 IDE。** 已有工作区管理、Maven/Node 服务发现、进程指标、Java/Node.js 启停和实时输出，以及官方 Arthas 附加。Web 与 MCP 共用核心引擎。支持聚合前端按启动脚本分别管理，详见 [工作区文档](docs/features/workspace.md)。

## 环境要求

- Rust 稳定版工具链（通过 rustup 安装，需支持 Rust 2024 edition）
- Node.js 22+ 和 pnpm 11.25.0（可使用 `corepack enable` 或 `npm i -g pnpm@11.25.0`）
- JDK 和 Maven：供后续管理 Spring Boot 项目使用，运行当前脚手架暂不需要

## 快速开始

### 双击打开浏览器（Windows）

已构建的目录或完整发布包中，双击根目录 `DevFlow.vbs` 即可启动后台并自动打开 **http://127.0.0.1:31080**。不需要单独启动前端开发服务器。重复双击会复用已就绪的后台；关闭浏览器不会停止后台或业务项目。

如果系统禁用了 VBScript，可双击 `DevFlow.cmd` 使用相同启动流程（会短暂显示命令窗口）。

首次没有启动配置时选择一个项目目录，该目录会作为初始工作区与允许访问的根目录。配置以 UTF-8 保存在安装目录的 `.devflow/launcher.json`，可修改 `workspace`（文件夹或 `.code-workspace` 文件）和 `allowedRoots`（额外授权目录数组）；启动器不自动保存页面中后来切换的工作区。也可从 PowerShell 运行 `scripts/launch.ps1 -Workspace 'D:\code' -AllowedRoots @('D:\code', 'D:\WorkSpace')`。优先级为显式参数、`DEVFLOW_*` 环境变量、保存的启动配置。

给其他人使用时，由开发者在依赖安装完成后运行一次以下打包命令：

```powershell
.\scripts\package.ps1
```

该命令显式执行前端和 release 后端构建，并在 `releases/DevFlow-时间戳/` 生成独立目录。将整个目录交给使用者，解压到可写位置后双击 `DevFlow.vbs`。使用者启动 DevFlow 无需安装 Rust、Node.js 或 pnpm；运行 Java/Maven 项目仍需对应工具链。普通启动绝不自动安装依赖或编译；缺少文件、端口占用、启动失败会弹窗提示，后台日志位于 `.devflow/backend-*.log`。尚未提供后台重启按钮、崩溃自动恢复或托盘退出入口。

### 源码开发

在 Windows 上安装依赖后，可以通过一条命令同时启动前后端：

```powershell
pnpm install --frozen-lockfile
.\scripts\dev.ps1
# 可选：.\scripts\dev.ps1 -Workspace 'D:\code'
# 工作区文件及跨目录工程：
# .\scripts\dev.ps1 -Workspace 'D:\WorkSpace\main-system.code-workspace' -AllowedRoots @('D:\WorkSpace', 'D:\CompanyProject', 'D:\AiFlow')
```

启动脚本会自动定位仓库根目录，等待后端 API 就绪，并在前端退出时结束后端进程树。后端日志保存在 `.devflow/` 目录下。

如果没有指定 `-Workspace` 或 `DEVFLOW_WORKSPACE`，初始项目列表将为空。可设置 `-AllowedRoots` / `DEVFLOW_ALLOWED_ROOTS` 后在页面打开工作区；引用其他目录的工程也必须在授权范围内。`.env.example` 仅供参考，程序不会自动加载环境文件。

也可以使用两个终端分别启动：

```powershell
# 终端 1：在仓库根目录执行
$env:DEVFLOW_WORKSPACE='D:\code'  # 修改为实际存放项目的目录
cargo run -p devflow

# 终端 2
pnpm install
pnpm web:dev
```

在 Codex 内置浏览器或其他浏览器中访问 **http://127.0.0.1:5173**。

### 本地构建运行

```powershell
pnpm install
pnpm web:build
cargo run -p devflow
```

在仓库根目录运行后，访问 **http://127.0.0.1:31080**。如果存在 `apps/web/dist`，后端会提供已构建的前端静态文件。

### 验证

```powershell
Invoke-RestMethod http://127.0.0.1:31080/api/health
$session = Invoke-RestMethod http://127.0.0.1:31080/api/session -Headers @{'X-DevFlow-Client'='web'}
Invoke-RestMethod http://127.0.0.1:31080/api/projects -Headers @{Authorization="Bearer $($session.token)"}
cargo test --workspace
pnpm web:check
```

### Codex MCP 接入（实验性）

```bash
codex mcp add devflow --url http://127.0.0.1:31080/mcp --bearer-token-env-var DEVFLOW_API_TOKEN
```

后端与 MCP 客户端进程需设置相同的 `DEVFLOW_API_TOKEN`。`/mcp` 当前提供最小化 JSON-RPC 接口，支持 `initialize`、`ping`、`tools/list` 和 `tools/call`，包括工作区查询、打开、新建、添加文件夹、保存及只读指标工具。详细参数见工作区文档。

部分 MCP 客户端可能需要更完整的会话和通知机制。正式用于生产环境前，建议升级为持续维护的 Rust MCP SDK。**请勿将此端口直接暴露到公网。**

Web 开发代理仅用于浏览器 API 请求，Codex 则直接与 Axum 服务通信。

## 支持的工作区

`DEVFLOW_WORKSPACE` 可以指定目录或 VS Code `.code-workspace` 文件。支持页面及 MCP 打开、新建、添加文件夹、保存和另存为；兼容 JSONC 注释、尾随逗号、相对路径、显示名称和本地 file URI，保留配置字段。远程 URI 暂不支持。

右侧只列出可启动应用：沿 Maven 声明的多层 modules 找到真实 Java 启动主类，排除聚合模块、普通依赖库和测试入口；Node 工程识别 dev/start/serve 及关联启动别名，像 `system: npm run serve --project=...` 会作为独立服务展示。发现不运行 Maven、不扫描依赖 JAR，不等价于完整 effective POM 或 npm/pnpm workspace 图解析。详细边界见工作区文档。

## 仓库结构

- `crates/devflow-core/src/<feature>`：业务逻辑与领域操作
- `crates/devflow/src/http`：HTTP 接口适配层
- `crates/devflow/src/mcp`：MCP 适配层（实验性）
- `apps/web/src/features/<feature>`：React 前端功能模块
- `apps/web/src/components`：共享 UI 组件（按实际需要逐步添加）
- `docs/features`：功能设计规范

更多信息请参阅 [AGENTS.md](AGENTS.md) 和 [架构文档](docs/architecture.md)。
