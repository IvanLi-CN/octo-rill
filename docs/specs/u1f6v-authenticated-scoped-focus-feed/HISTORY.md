# Authenticated Scoped Focus Feed History

## 2026-06-26

- 创建新 spec，冻结 authenticated scoped focus feed 的路由、scope、feed 过滤与视觉证据路线。
- 明确 `#2x7av` 与 `#w5gaz` 仅作为 related prior art 引用，不直接改写其原有主题边界。

## 2026-07-03

- 明确 scoped focus 页是只读范围聚焦面，不暴露全局历史日组的按天“生成日报”动作；补齐 Storybook 断言、Playwright 覆盖与视觉证据。
- 收紧 scoped focus 页的状态隔离：全局 `全部` tab 遗留的 pending/error 日报生成状态不得在 scoped 页渲染为占位或错误面板。
