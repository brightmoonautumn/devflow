# 公共表单组件

业务视图通过本目录组件使用表单控件，统一样式、焦点与禁用状态。

- `BaseSelect`、`BaseCheckbox`：封装 Radix 无样式交互原语，不向业务暴露其组件树或事件类型。
- `BaseInput`、`BaseTextarea`、`BaseButton`：保留原生输入、表单提交与无障碍语义，统一视觉；参数输入可使用 `mono` 等宽样式。
- `BaseField`：统一标签、间距与辅助说明；使用 `htmlFor` 关联控件，有提示时控件通过 `aria-describedby="${htmlFor}-hint"` 关联提示。
- `form.css`：使用工作区的字号、字体、颜色与主按钮令牌，不在各业务视图内重复覆盖控件外观。

`BaseSelect` 对外使用 `value / onValueChange / options`。下拉层自动挂到最近的原生 `dialog` 或工作区外壳内，继承主题并避免被原生模态窗口遮挡；Escape 先关闭下拉，再次按下才关闭父窗口。选项值必须非空且唯一。

启动配置和运行输出工具栏已接入；工作区管理中其他原有控件尚未整体迁移。新增或修改表单优先复用此层，仅在真实复用后扩展，不预建无调用的组件。
