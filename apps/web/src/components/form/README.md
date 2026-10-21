# 公共表单组件

业务视图通过本目录组件使用表单控件，统一样式、焦点与禁用状态。

- `BaseSelect`、`BaseCheckbox`：封装 Radix 无样式交互原语，不向业务暴露其组件树或事件类型。
- `BaseInput`、`BaseTextarea`、`BaseButton`：保留原生输入、表单提交与无障碍语义，统一视觉；参数输入可使用 `mono` 等宽样式。
- `BaseField`：统一标签、间距与辅助说明；使用 `htmlFor` 关联控件，有提示时控件通过 `aria-describedby="${htmlFor}-hint"` 关联提示。
- `BaseDialogProvider` / `useDialogs`：统一异步 `confirm / alert` 确认与提示，业务只传标题、描述、按钮文案和 `tone`，不依赖底层弹窗实现，也不直接调用 `window.confirm / window.alert`。提供请求排队、Escape 取消、默认聚焦取消按钮、焦点约束和卸载时取消，弹窗挂载在主题外壳内。危险操作复用 `BaseButton` 的 `danger` 变体。
- `form.css`：使用工作区的字号、字体、颜色与主按钮令牌，不在各业务视图内重复覆盖控件外观。

`BaseSelect` 对外使用 `value / onValueChange / options`。下拉层自动挂到最近的原生 `dialog` 或工作区外壳内，继承主题并避免被原生模态窗口遮挡；Escape 先关闭下拉，再次按下才关闭父窗口。选项值必须非空且唯一。

启动配置和运行输出工具栏已接入；工作区管理中其他原有控件尚未整体迁移。新增或修改表单优先复用此层，仅在真实复用后扩展，不预建无调用的组件。

只读分享表单已接入 `BaseField / BaseSelect / BaseInput / BaseButton`：有效期使用公共下拉，地址与邀请链接使用公共等宽输入框，创建/复制/关闭复用公共按钮，撤销使用 `danger` 变体。分享样式仅负责字段和列表布局，不覆盖公共控件的颜色、边框、焦点与禁用样式。下拉仍通过 `BaseSelect` 挂载在分享对话框内。

文件未保存的关闭/重新读取、标签数量提示、文件删除、最近工作区切换已接入公共对话框。文件放弃确认期间暂停该文件自动保存，取消后恢复；确认后重新读取标签状态再操作。浏览器刷新或关闭页面的 `beforeunload` 安全提示由浏览器控制，无法自定义样式，仍保留其未保存保护。
