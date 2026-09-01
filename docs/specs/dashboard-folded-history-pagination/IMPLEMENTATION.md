# Dashboard 折叠历史的可见进展分页 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 已实现，视觉证据已确认，PR #250 收口中
- Lifecycle: active
- Catalog note: 已确认采用显式续载，运行时、回归与 Storybook 覆盖已完成。

## Coverage / rollout summary

- 已完成运行时诊断：分页 API 成功返回并合并第二页，问题位于历史日报折叠后的前端哨兵状态。
- `FeedGroupedList` 以日组、视图和已渲染原始活动键组成可见投影；成功追加无投影变化时只在 `全部` tab 进入显式续载。
- 续载按钮复用既有追加请求、去重、错误与加载状态；投影产生进展、tab/scope/首屏刷新或历史组切换到“列表”时清除暂停。
- Dashboard mock cursor 序列已覆盖折叠暂停、显式续载、列表切换、分页失败重试和末页。
- Storybook 已增加 `Feed/FeedGroupedList/Folded History Pagination Continuation` 场景。
- Storybook 加载态使用无文字三点波浪胶囊，悬停 500ms 后显示 Tooltip；Dashboard mock E2E 已覆盖延迟行为。

## Remaining Gaps

- 等待 PR #250 的必需 CI 与最终交付事实收敛。

## Related Changes

- `web/src/feed/FeedGroupedList.tsx`
- `web/src/index.css`
- `web/e2e/dashboard-brief-detail.spec.ts`
- `web/src/feed/FeedGroupedList.stories.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`
