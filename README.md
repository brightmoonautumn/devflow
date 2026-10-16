# DevFlow Starter

**简体中文 | [English](README.en.md)**

本地优先的开发工作台。采用 **Rust + Tokio + Axum + React + TypeScript + MCP** 技术栈，所有代码统一维护在一个仓库中。

> **当前处于可运行的脚手架阶段，并非完整 IDE。** 已实现的功能包括：健康检查 API、Maven/Node 项目浅层扫描、只读的 `workspace_list` MCP 工具以及 React 基础界面。运行时管理、Arthas、上下文篮子及 QA 测试等能力目前作为扩展点，供后续使用 Codex 逐步实现。

## 环境要求

- Rust 稳定版工具链（通过 rustup 安装，需支持 Rust 2024 edition）
- Node.js 22+ 和 pnpm 11.25.0（可使用 `corepack enable` 或 `npm i -g pnpm@11.25.0`）
- JDK 和 Maven：供后续管理 Spring Boot 项目使用，运行当前脚手架暂不需要

## 快速开始

在 Windows 上安装依赖后，可以通过一条命令同时启动前后端：

```powershell
pnpm install --frozen-lockfile
.\scripts\dev.ps1
# 可选：.\scripts\dev.ps1 -Workspace 'D:\code'
```

启动脚本会自动定位仓库根目录，等待后端 API 就绪，并在前端退出时结束后端进程树。后端日志保存在 `.devflow/` 目录下。

如果没有指定 `-Workspace` 或 `DEVFLOW_WORKSPACE`，初始项目列表将为空。`.env.example` 仅供参考，程序不会自动加载环境文件。

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
Invoke-RestMethod http://127.0.0.1:31080/api/projects
cargo test --workspace
pnpm web:check
```

### Codex MCP 接入（实验性）

```bash
codex mcp add devflow --url http://127.0.0.1:31080/mcp
```

`/mcp` 当前提供最小化、无状态的 JSON-RPC 接口，支持 `initialize`、`ping`、`tools/list` 和 `tools/call`，并提供一个**只读**的 `workspace_list` 工具。

部分 MCP 客户端可能需要更完整的会话和通知机制。正式用于生产环境前，建议升级为持续维护的 Rust MCP SDK。**请勿将此端口直接暴露到公网。**

Web 开发代理仅用于浏览器 API 请求，Codex 则直接与 Axum 服务通信。

## 支持的工作区

`DEVFLOW_WORKSPACE` 指定项目所在的父目录，例如 `D:\code`。目录下可以包含基于 Maven `pom.xml` 和／或 Node.js `package.json` 的项目。

目前仅扫描工作区根目录及其直属子目录，暂不解析 Maven 多模块元数据，也不解析 npm/pnpm workspace 依赖关系。该限制是 v0.1 阶段有意采用的设计，以保证扫描行为安全、可预测。

## 仓库结构

- `crates/devflow-core/src/<feature>`：业务逻辑与领域操作
- `crates/devflow/src/http`：HTTP 接口适配层
- `crates/devflow/src/mcp`：MCP 适配层（实验性）
- `apps/web/src/features/<feature>`：React 前端功能模块
- `apps/web/src/components`：共享 UI 组件（按实际需要逐步添加）
- `docs/features`：功能设计规范

更多信息请参阅 [AGENTS.md](AGENTS.md) 和 [架构文档](docs/architecture.md)。
