# 公共编辑器防腐层

业务通过 `FileEditor` 的 `path / value / readonly / location / onChange / onSave / onPosition / onOpenFile / toolbarContainer` 使用编辑能力，不依赖 CodeMirror、Vditor 的组件树、实例或事件类型。文件鉴权、版本检查、草稿和保存由工作区业务与既有共享核心处理。

- 普通文本由 `SourceEditor` 封装 CodeMirror；语言加载归属 `editorLanguages.ts`。
- `.md / .markdown` 由 `MarkdownEditor` 封装 Vditor，默认所见即所得文档编辑，支持源码和对照模式。对照时编辑源码，文档区域只读，窄面板自动上下排列；实例在切换模式时保留，源码、文档各自保留撤销历史。
- 参考了 [Office Viewer 的 Markdown 接入](https://github.com/cweijan/vscode-office/blob/main/resource/markdown/index.js)：本地资源、禁用依赖缓存、输入回传和外部内容同步。Office Viewer 维护定制 Vditor；本项目直接使用 [Vditor 开源上游](https://github.com/Vanessa219/vditor) 的 npm 分发，未移植其 VS Code 协议或扩展源码。
- `markdownRuntime.ts` 管理本地脚本与 Mermaid 全局 API 适配；按调用串行初始化与绘图，使用 strict 模式和 SVG 清洗。Vditor 负责表格、目录、任务列表、脚注、代码及 KaTeX；依赖的磁盘资源由 `apps/web/vditorAssets.ts` 白名单提供，并在构建时输出到 `vendor/vditor`，无需在线 CDN。只枚举一个已知字体目录，不递归遍历依赖目录。
- 打开文件以原始内容建立基线，依赖的初始 Markdown 规范化不回传保存；真实编辑后才调用 `onChange`。可视化编辑会规范化表格、空行等 Markdown 排版，精确保留源码格式时使用源码模式。内容回传后不重新 `setValue`，以保留光标；外部更新才同步实例。
- 编辑正文时 Ctrl/Cmd+单击或双击链接跳转，对照文档单击跳转；相对路径交给 `onOpenFile`，由既有文件 API 校验工作区成员与授权。未引入本地图片二进制读取或附件上传接口。
- 双击 Mermaid 图表放大，支持 50%–300% 缩放与 Escape 关闭；错误图表显示提示，保留源码可继续修正。原文中的 `<br/>` 由 Mermaid 在 SVG 文本标签中换行，不使用任意 HTML 图表标签。

依赖采用各自发布的开源许可证，Vditor 运行资源保留其 MIT 许可证与代码高亮资源许可证；本项目自己的组件源码没有复制 Office Viewer 的实现。尚未执行编译、构建或运行测试，界面效果待用户验证。

文档、源码、对照切换通过 `toolbarContainer` 中性插槽挂到文件标签栏右侧，使用紧凑图标分段按钮，不增加独立模式行。文件路径工具栏已移除；重新读取按钮保留在标签栏，当前活动文件的保存状态通过 `onSaveStatusChange` 回传外壳，与 CPU/内存指标放在一起。

Vditor 挂载节点的 className 保持静态，加载可见状态放在独立外框，避免 React 状态更新覆盖依赖动态加入的 `vditor / vditor--dark` 类。各层 flex 明确收缩和高度约束，正文根 `pre` 独立负责滚动并保留可见滚动条；对照只读预览保持正常文字透明度。
