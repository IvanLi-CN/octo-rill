# 演进记录（shadcn/ui 全量整改与组件收敛）

## 生命周期

- Lifecycle: active
- Created: 2026-03-06
- Last: 2026-03-08

## 变更记录

- 2026-03-06: 新建规格，冻结 shadcn/ui 全量整改范围与验收口径。
- 2026-03-06: 已补齐官方 shadcn primitives，完成 Dashboard / Admin Users / Admin Jobs 的页面级收敛，并更新 stories / e2e。
- 2026-03-06: 已生成 Dashboard、Admin Users、Admin Jobs 的 Storybook 视觉证据并完成 `bun run lint`、`bun run build`、`bun run storybook:build`、`bun run e2e`。
- 2026-03-07: 完成 rebase 收口、修复 Storybook autodocs 自动弹层与 Admin Jobs LLM detail 刷新串位问题，PR #25 全部 checks 转绿且无阻塞 review。
- 2026-03-08: 修复 Dashboard 前台 release / brief / inbox 列表与 release detail 中 RFC3339 UTC 时间被当成本地时间直出的回归，统一改为浏览器当前时区格式化并补充 Playwright 覆盖。
- 2026-03-08: 继续收敛 brief 卡片标题与 Markdown 正文中的 RFC3339 时间，统一在浏览器渲染期做本地化，避免历史 UTC 字符串在日报正文中继续直出。
- 2026-03-08: 对齐 dashboard / sidebar 相关 Playwright 断言与合并门禁证据，保持浏览器时区回归覆盖稳定。
