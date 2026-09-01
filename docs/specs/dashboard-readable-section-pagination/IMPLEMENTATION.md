# Dashboard 可读区块分页 实现状态

> 有效行为以 `./SPEC.md` 为准；此处只记录实现覆盖、验证与兼容性事实。

## Current Status

- Implementation: 已实现，待视觉证据确认与 PR 门禁收口。
- Lifecycle: active。
- Catalog note: 该主题替代客户端可见投影暂停与显式续载路径。

## Implementation Coverage

- `REQ-READABLE-SECTIONS-001`、`REQ-READABLE-SECTIONS-006`: Rust 增加 `/api/dashboard/feed` 与不透明用户绑定的区块 cursor；区块按自然日最多三组推进，同步或日报刷新不使旧时间边界失效。
- `REQ-READABLE-SECTIONS-002`: 日报区块直接返回完整 `content_markdown`，并按窗口与覆盖关系投影未覆盖补充动态；公开响应不返回 `release_ids` 或 `preview_markdown`。
- `REQ-READABLE-SECTIONS-003`: `/api/dashboard/feed/sections/{section_id}/items` 使用独立的用户/区块绑定明细 cursor，每页最多 30 条；Dashboard 只在“列表”切换后请求并独立自动续页。
- `REQ-READABLE-SECTIONS-004`: 无日报日期作为 raw 区块返回首批活动，保留显式“生成日报”，滚动不会触发生成。
- `REQ-READABLE-SECTIONS-005`: 根页 `全部` 使用 `FeedReadableSectionList`，主区块 observer 单 cursor 去重；请求中显示无文字居中三点波浪胶囊，失败原位重试，正常路径不显示继续加载按钮。
- `REQ-READABLE-SECTIONS-007`: 新迁移为日报、共享发布与社交活动增加用户/时间排序索引；原始 `/api/feed`、日报 tab 与 scoped 页面保持原调用合同。
- Verification: Rust cursor/time-zone 单测通过；Dashboard Playwright 覆盖区块续页、列表按需加载、主区块/明细加载态和失败重试；Storybook 覆盖完整日报、补充动态、列表、加载、失败和窄屏场景；`bun run lint`、`bun run build` 与 `bun run storybook:build` 已通过。

## Remaining Gaps

- 视觉截图是当前基线中不存在的新增证据，需主人确认后持久化到本主题并进入 PR 描述。
- `cargo test` 全量、构建产物审计和远端必需 CI 需在提交当前 head 后重新收口。

## Related Changes

- 先前的客户端折叠历史分页实现见 `../dashboard-folded-history-pagination/`。

## References

- `./SPEC.md`
- `./HISTORY.md`
