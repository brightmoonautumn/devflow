# 构思画布

用于开发前与 AI 讨论功能、需求、架构和页面原型。入口在左侧「构思」，深链接 `/#ideas`；同一工作区的 Web 与 MCP 共用 `WorkspaceService` 的状态和事务，不增加内嵌 AI 聊天。

## 页面与交互

- 每个工作区可创建、命名、切换和删除多张画布，初始为空，不自动注入示例数据。画布选择与看板分类下拉框共用菜单样式：列表中切换画布，每行提供重命名和删除，底部提供新建画布；重命名在菜单内编辑、确认或取消，删除保留后果提示。移除画布底部名称、节点计数与删除栏。
- 参考 `neural-creator-dashboard` 的银灰、中性底色、细曲线、分层展开与节点聚焦；沿用 DevFlow 的明暗主题和控件令牌。编辑节点保持稳定，不使用漂浮动画。
- 一张画布混合功能导图和自由布局。`parentId` 表示导图父子关系，自动绘制曲线；自由连线有方向和说明文字。可添加分组，节点通过 `groupId` 加入分组。
- 节点保存标题、简述、图片和 HTML 原型，可同时含图片与原型。图片支持文件选择、粘贴和拖入；HTML 支持文件拖入/选择，在画布节点上预览或替换，不展示原始 HTML 编辑表单。原型适合自包含 HTML/CSS/内联 JS，不依赖 CDN。
- 移除详情侧栏。双击节点、按 Enter/F2 或点击节点浮动工具栏中的编辑按钮，直接修改标题与简述；分组就地修改名称。失焦或 Ctrl/Command+Enter 完成编辑，Escape 结束编辑并保留已输入内容。折叠按钮隐藏分支；分组通过就近工具栏折叠。整理仅作用于所选分支或分组，不移动其他自由节点。
- 拖动空白平移，滚轮平移，Ctrl/Command + 滚轮缩放；底部可缩放、适应全部、定位选中节点。拖动父节点/分组同时移动其后代。节点获得焦点后，方向键移动 10，Shift + 方向键移动 40，界面不显示坐标、父节点和分组归属等高级表单，数据与 MCP 能力保持兼容。
- 保留最近 20 步本地撤销，Ctrl/Command+Z 撤销、Ctrl/Command+Shift+Z 重做。远端更新当前画布后清空旧撤销历史，避免将 AI 修改整图撤销。
- 选中节点后显示就近浮动工具栏，提供文字、附件、连线、整理、转入看板和删除操作，工具栏不随画布缩放。节点外侧的圆形加号用于添加子节点，悬停、选中或键盘聚焦时显示；节点原有右上角箭头入口已移除。连线选中后在连线附近直接编辑说明和删除。
- 交互参考 [xyflow NodeToolbar 源码](https://github.com/xyflow/xyflow/blob/main/packages/react/src/additional-components/NodeToolbar/NodeToolbar.tsx) 与 [React Flow Button Handle](https://reactflow.dev/ui/components/button-handle)，在现有画布实现上采用对应模式，不新增画布库依赖。

## 保存、并发与附件

- 编辑停顿 850ms 后自动保存，拖动过程仅本地预览、结束后保存；页面空闲每 4 秒检查变更。编辑、拖动、保存、错误待处理时暂停远端应用，不抢占视口。
- AI 新增/修改的节点高亮，并提供「定位本次更新」。默认保持用户当前视口。
- `workspaceKey` 来自 `workspaceFile` 或 `workspaceRoot`，`expectedRevision` 来自工作区版本；写入额外校验 `ideasRevision`。认证、活动工作区和授权范围在共享核心校验。工作区锁覆盖读写，写操作另持有跨进程文件锁。
- 数据存于 DevFlow 自身 `.devflow/boards/<工作区哈希>.ideas.json`，图片使用受限 raster base64 data URL，HTML 以字符串保存；附件内嵌在版本化文件中，不向受管理项目写入文件。
- 保存使用同目录唯一临时文件、同步内容和重命名替换。损坏、非法或身份不符的文件报错并保留，不自动覆盖为空。
- 保存冲突时保留本地编辑并停止写入，不盲目重试。可先导出 JSON，再确认放弃本地修改并重新读取；非冲突校验错误可继续修改后保存。
- 有未保存修改或请求仍在进行时，拦截页面入口、文件搜索快捷键、工作区管理和刷新离开，避免正常切换时丢失输入。浏览器关闭/刷新使用原生未保存提示。
- 原型预览使用独立 opaque-origin sandbox iframe，不允许同源权限、弹窗、顶层导航或表单提交；CSP 限制资源加载与连接，图片/字体仅 data URL，脚本与样式仅内联。HTML 内容不插入 DevFlow DOM，缩略预览禁用脚本。
- 当前上限：每工作区 30 张画布、每画布 500 个节点/1000 条自由连线，整体 JSON 12 MiB；图片 data URL 4 MiB，HTML 512 KiB。网页原图限制约 3 MiB；不接受 SVG 或远程图片 URL。文本长度沿用核心 UTF-8 字节限制。

## 转入任务看板

选中节点/分组，在其就近工具栏点击「转为看板功能」，预览功能与后代任务，再确认创建。根节点成为功能，其余非分组后代进入待开始；描述保留回到原画布和节点的链接：`/#ideas?canvas=<id>&node=<id>`。

调用现有看板 HTTP 写入口的 `idea_branch` 操作，核心在一个事务内创建功能和全部任务，校验失败不写入任何部分。功能 ID 根据画布及根节点稳定生成，重复导入被拒绝，已有看板内容不被覆盖。该操作仅允许本机人类入口，AI 可继续使用现有经授权的看板工具，不绕过预览流程。导入不启动 agent，也不执行构建或项目代码。

## HTTP / MCP

- `GET /api/workspace/ideas`：读取快照；`workspaceKey`、`expectedRevision` 必填。`canvasId` 可选；`includeMedia=true` 包含附件内容，默认清空 `image/html` 内容以减小上下文；`ifRevision` 等于当前版本时返回 JSON `null`。
- `POST /api/workspace/ideas`：`{ workspaceKey, expectedRevision, ideasRevision, operation }`。操作为 `canvas_save`、`canvas_delete`、`patch`。网页始终读取完整附件后保存选中画布。
- MCP `ideas_get` / `ideas_execute` 使用相同请求和核心。读取正文在 `structuredContent`，文本块为简短回执，避免重复传输附件；未变化时 structuredContent 为 `{ "result": null }`。写操作返回版本和画布概要。
- `canvas_save` 完整替换一张画布，适合首次创建；替换既有画布必须先 `includeMedia=true` 读取。`patch.nodes` 按稳定 ID 合并提供的字段，未提供的坐标、附件和折叠状态保持；新节点必须提供 `id/title/x/y`。`parentId/groupId=null` 解除归属，`image/html=""` 删除附件。自由连线按 ID 完整更新。
- 删除节点自动移除其自由连线，直接子节点解除归属并保留。核心拒绝重复 ID、缺失端点、越界坐标、循环父子/分组关系及无效媒体。

AI 创建导图示例（先 `workspace_get`、`ideas_get` 获取真实身份与版本，以下版本仅示意）：

```json
{
  "workspaceKey": "D:/project/example.code-workspace",
  "expectedRevision": 3,
  "ideasRevision": 0,
  "operation": {
    "type": "canvas_save",
    "canvas": {
      "id": "account-planning",
      "name": "用户管理",
      "nodes": [
        { "id": "account", "title": "用户管理", "body": "账号、权限与登录流程", "x": 0, "y": 100 },
        { "id": "login", "title": "登录", "body": "密码登录及会话管理", "parentId": "account", "x": 318, "y": 100 }
      ],
      "edges": []
    }
  }
}
```

后续修改简述时只需：

```json
{
  "workspaceKey": "D:/project/example.code-workspace",
  "expectedRevision": 3,
  "ideasRevision": 1,
  "operation": {
    "type": "patch",
    "canvasId": "account-planning",
    "nodes": [{ "id": "login", "body": "补充验证码登录" }]
  }
}
```

## 验证状态

修复分组矩形的画布 `x/y` 未转换为 CSS `left/top` 导致多个分组在原点重叠的问题；分组标题限制在分组宽度内，分组连线与连线编辑位置按实际矩形边界计算。本轮仅静态审查，未运行构建、测试或浏览器验证。

已按界面反馈统一画布下拉框、移除详情侧栏、改为画布内编辑和节点外加号；仅代码审查与差异检查，未运行构建、测试或浏览器验证。

补齐布局持久化 `WorkspaceViewState.activePage` 的 `ideas` 页面类型，修复前端构建报告中的 TS2367 / TS2322；未重新运行构建。

本轮仅代码审查与差异检查；遵循项目约定，未编译、构建、运行测试或打开浏览器。更新并重启后端/前端后，需人工验收：多画布持久化、树形折叠与自由布局、图片/HTML、MCP 局部更新、两入口冲突、拖动取消、撤销重做、窄屏及看板原子导入。运行效果尚未经验证。
