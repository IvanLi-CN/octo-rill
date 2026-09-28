# 命令面板与个人工作区搜索实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、验证结果与 rollout 事实。

## Current Status

- Implementation: 已实现并通过本地验证，视觉证据已由 owner 确认；FTS rowid 恢复、metadata queue 和可恢复分批回填已进入 PR 合并收口
- Lifecycle: active
- Catalog note: 已定义；本地缓存搜索、命令面板和 anchored fixed-window quota 合同已锁定

## Implementation Coverage

- `REQ-CPS-001` 至 `REQ-CPS-006`: 后端搜索文档投影、查询解析、权限过滤、统一响应和用户级窗口计数器。
- `REQ-CPS-007` 至 `REQ-CPS-010`: Dashboard 阅读壳层、响应式页头入口、Dialog 命令面板、受控 actions 与 lane deep link。
- `REQ-CPS-011` 至 `REQ-CPS-012`: 只读边界、稳定去重排序、Web Demo、Storybook 和交互/视觉回归。
- `REQ-CPS-013`: 历史 `0081` 已移入 `migrations/legacy/` 并由严格 checksum 兼容路径读取；`0082` 只建 schema，`0089_association_search_update_repair.sql` 将关联触发器限制到 repository identity/metadata projection 更新并增加 `(resource_type, repo_id)` 索引，`0090_search_fts_rowid_recovery.sql` 将旧 FTS cache 替换为 rowid mapping + v2 corpus、保留 point-update compatibility views，并将 repository metadata fanout 放入可去重队列；metadata source deletion 会入队，新的 metadata event 会重置 per-repo cursor；`src/search_index.rs` 以持久化阶段游标、每事务最多 100 个 source/release rows、`Background` writer、`statvfs` 水位和 foreground yield 执行可恢复回填，未 ready 或 metadata queue 未清空时由搜索回退到 `LIKE`。
- Web Demo 额外提供 `Command Palette · Search`、`Command Palette · Indexing`、`Command Palette · Low Disk`、`Command Palette · Actions` 与 `Command Palette · Admin` 五个可选场景；进入场景后分别自动打开搜索结果、渐进索引状态、低磁盘降级、actions 模式和管理员受控 action。
- Verification commands: `cargo test --all-features`、`cargo test database_migrations::tests`、`cargo test search::tests`、`cargo test api::tests::search_`、`web/bun run lint`、`web/bun run build`、`web/bun run test:storybook -- CommandPalette`。

## Coverage / rollout summary

- 搜索必须以本地缓存为唯一数据源；GitHub Search API 不属于本主题的运行时依赖。
- 配额状态必须与 SQLite writer 事务一致，服务重启和多标签页共享同一用户窗口。
- actions 仅复用既有导航、同步、日报生成和仓库关注边界，不新增任意命令执行器。
- 迁移启动不再扫描历史内容；后台 worker 在 listener 绑定后按阶段恢复投影，索引状态通过 `GET /api/search` 的 `index_status` 暴露给命令面板。

## Verification Coverage

- `search::tests::migration_and_projection`：覆盖 Release、翻译投影、日报、通知、仓库投影及源缓存删除。
- `search::tests::rate_limit_is_atomic_under_concurrency`：覆盖并发 anchored fixed-window counter。
- `api::tests::search_contract`：覆盖过滤器、lane、结果合同及非法请求不计费。
- `api::tests::search_permission_and_rate_limit`：覆盖跨用户可见性、50/51 次边界、429 元数据及窗口重置。
- `search::tests::announcement_key_change_rebuilds_cached_lanes`：覆盖公告事件 ID 到讨论键变更时的 lane 恢复。
- `search::tests::late_global_projection_cannot_replace_newer_ready_projection`：覆盖晚到旧投影不覆盖较新的 ready 投影。
- `search::tests::projection_backfill_resumes_in_bounded_batches`：覆盖可恢复索引的阶段推进、公告回填和结果可见性。
- `search::tests::projection_backfill_recovers_persisted_cursor_after_restart`：覆盖 FTS cache 恢复时持久化 cursor 在重建 AppState 后继续推进。
- `search::tests::metadata_fanout_resumes_in_release_row_batches`：覆盖单仓库 250 条 release metadata fanout 的 100/100/50 分批、队列 drain 与 FTS 一致性。
- `search::tests::work_item_deletion_repairs_release_metadata`：覆盖删除最后一个 release work item metadata source 后的队列入队与 metadata 清理。
- `search::tests::metadata_change_restarts_release_cursor`：覆盖 metadata fanout 进行中变更 repository identity 后 cursor 重置并重新刷新前段 rows。
- `search::tests::fts_doc_id_maintenance_uses_indexed_point_updates`：覆盖 global 与 user-lane FTS `doc_id` 维护均走 mapping rowid point lookup，且不出现旧 virtual-table scan plan。
- `search::tests::production_sized_fts_maintenance_benchmark`：手动 ignored 基准，覆盖 397 associations、39,700 release documents、397 lanes、旧/新 plan、writer batch 和并发 search/GET/session workload。
- `search::tests::backfill_prefers_ready_projection_over_newer_running_projection`：覆盖恢复回填与触发器一致的 ready 投影优先级。
- `search::tests::owned_release_visibility_repairs_cached_release_metadata`：覆盖历史 Release 投影在自有仓库可见性启用、仓库改名和回填恢复后的元数据与深链。
- `search::tests::repository_rename_deduplicates_star_and_association_projection`：覆盖 star 同步先改名、association 随后写入时按 `repo_id` 清理旧 projection。
- `search::tests::association_non_projection_updates_do_not_rebuild_release_fts`：通过真实迁移触发器验证 source flag、follow state 和 `updated_at`-only association updates 不重建 Release FTS。
- `api::tests::repo_association_upsert_skips_timestamp_only_noop`：验证有效值不变时 association upsert 不因新的 `updated_at` 产生写入，并保留显式 follow state。
- `api::tests::association_source_clear_preserves_explicit_unfollow`：验证单仓库 source 清理保留显式取消关注。
- `sync::tests::repeated_social_snapshot_only_updates_association_observation_once`：验证重复 production-shaped owned-repository snapshot 不清除再重建当前 association。
- `sync::tests::social_snapshot_clears_stale_associations_and_preserves_explicit_unfollow`：验证 NULL-ID/过期 association 清理、空快照和显式取消关注在仓库重新出现时的状态保持。
- `sync::tests::replace_starred_repos_preserves_explicit_unfollow`：验证完整 starred-repository replacement 路径清理旧 source 时保留显式取消关注。
- Synthetic SQLite benchmark: the old global and lane FTS tables both produced `SCAN ... VIRTUAL TABLE INDEX 0:` plans; the new compatibility views produced mapping-index lookups plus `SCAN f VIRTUAL TABLE INDEX 0:=` (the `SCAN f` row is the rowid point lookup, not an FTS corpus scan). With 397 associations, 39,700 release documents and 397 user lanes, 32 maintenance rounds measured global FTS `1161.407 ms -> 30.259 ms` and lane FTS `34.288 ms -> 29.192 ms`; one metadata Background batch held the writer for `20.566 ms` and left 397 queued repos. The same run completed 32 concurrent search tasks, 32 authenticated GET/quota tasks and 32 coordinated session writes. The benchmark is intentionally ignored by the normal suite because the local run takes about 32 seconds; it uses a temporary real-schema SQLite database and is not a production deployment measurement.
- `web/src/search/CommandPalette.stories.tsx`：覆盖空态、搜索结果、动作、日报确认、busy 键盘保护、错误、限流、管理员及 393px 视口。
- `web/src/search/CommandPalette.stories.tsx`：追加 `IndexBuilding` 与 `IndexPausedLowDisk`，验证渐进索引的可见降级提示。

## Visual Evidence Status

- 当前资产：`./assets/command-palette-desktop.png`、`./assets/command-palette-mobile393.png`。
- 状态：两张均为 mock-only 证据，已在 Chrome Demo 中由 owner 确认，并写入 `SPEC.md` 的 `## Visual Evidence`。

## Related Changes

- Repository rename projection cleanup keeps one canonical repository result when star synchronization updates the name before the user association projection.
- Association source/follow/observation updates no longer rebuild repository/release search projections; repository metadata changes retain the existing rename and deduplication repair path.

## References

- `./SPEC.md`
- `./HISTORY.md`
