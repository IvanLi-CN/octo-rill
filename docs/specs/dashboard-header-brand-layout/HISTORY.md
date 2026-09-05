# Dashboard 页头品牌与账号菜单主题历史

> 本文件记录主题局部生命周期与兼容性背景；规范正文以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- Lifecycle: active.
- Dashboard、Storybook 与品牌展示面继续共用 `DashboardHeader`，避免品牌位和账号菜单行为分叉。
- 账号菜单保留 hover 与 click 两种桌面入口；触摸输入保持点击固定打开语义。

## Replacements / Background

- 页头从统计信息优先的布局收敛为品牌位、主同步操作和账号菜单三层结构。
- 账号菜单的可达区域扩展到头像与浮层之间的视觉间距，避免自然指针移动造成非预期关闭。

## Related Changes

- 初始品牌优先页头、账号浮层与后续 hover 连续性改进均属于本主题。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
