# 实现状态（日报快照与时区去敏改造）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-13
- Last: 2026-07-05
- Summary: 已交付；把 brief 升级为不可变快照，并修正 raw fallback 日组显示日期，使 08:00 边界前的本地次日 release 显示窗口结束日；Dashboard brief 列表 API 默认只返回摘要，完整 Markdown 通过单条详情接口懒加载。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)
