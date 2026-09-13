# 命令面板与个人工作区搜索实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、验证结果与 rollout 事实。

## Current Status

- Implementation: 已实现并通过本地验证，等待 owner-facing 视觉证据确认后收口
- Lifecycle: active
- Catalog note: 已定义；本地缓存搜索、命令面板和 anchored fixed-window quota 合同已锁定

## Implementation Coverage

- `REQ-CPS-001` 至 `REQ-CPS-006`: 后端搜索文档投影、查询解析、权限过滤、统一响应和用户级窗口计数器。
- `REQ-CPS-007` 至 `REQ-CPS-010`: Dashboard 阅读壳层、响应式页头入口、Dialog 命令面板、受控 actions 与 lane deep link。
- `REQ-CPS-011` 至 `REQ-CPS-012`: 只读边界、稳定去重排序、Web Demo、Storybook 和交互/视觉回归。
- Web Demo 额外提供 `Command Palette · Search`、`Command Palette · Actions` 与 `Command Palette · Admin` 三个可选场景；进入场景后分别自动打开搜索结果、actions 模式和管理员受控 action。
- Verification commands: `cargo test`（815 项）、`cargo test search::tests`（9 项）、`cargo test api::tests::search_`（2 项）、`cargo check`、`web/bun run lint`、`web/bun run build`、`web/bun run build:demo`、`web/bun run test:storybook`（32 项）、`web/bun run storybook:build`。

## Coverage / rollout summary

- 搜索必须以本地缓存为唯一数据源；GitHub Search API 不属于本主题的运行时依赖。
- 配额状态必须与 SQLite writer 事务一致，服务重启和多标签页共享同一用户窗口。
- actions 仅复用既有导航、同步、日报生成和仓库关注边界，不新增任意命令执行器。

## Verification Coverage

- `search::tests::migration_and_projection`：覆盖 Release、翻译投影、日报、通知、仓库投影及源缓存删除。
- `search::tests::rate_limit_is_atomic_under_concurrency`：覆盖并发 anchored fixed-window counter。
- `api::tests::search_contract`：覆盖过滤器、lane、结果合同及非法请求不计费。
- `api::tests::search_permission_and_rate_limit`：覆盖跨用户可见性、50/51 次边界、429 元数据及窗口重置。
- `search::tests::announcement_key_change_rebuilds_cached_lanes`：覆盖公告事件 ID 到讨论键变更时的 lane 恢复。
- `search::tests::late_global_projection_cannot_replace_newer_ready_projection`：覆盖晚到旧投影不覆盖较新的 ready 投影。
- `web/src/search/CommandPalette.stories.tsx`：覆盖空态、搜索结果、动作、日报确认、busy 键盘保护、错误、限流、管理员及 393px 视口。

## Visual Evidence Status

- 当前资产：`./assets/command-palette-desktop.png`、`./assets/command-palette-mobile393.png`。
- 状态：两张均为 mock-only `current-only` 证据，等待 owner-facing 确认后写入 `SPEC.md` 的 `## Visual Evidence`。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
