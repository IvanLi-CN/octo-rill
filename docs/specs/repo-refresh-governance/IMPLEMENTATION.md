# 实现状态（仓库刷新治理页与预算调度收敛）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-06-29
- Last: 2026-08-10
- Summary: 已交付；effective repo pool、10 分钟 budgeted governance snapshots、attempt-based system cycle ledger、chunked governance rebuild、`/admin/repos` 独立治理页、闲置账号暂停与自助恢复、Dashboard Release 自适应新鲜度与任务审计、预算编辑收口到订阅同步设置弹窗、仓库明细目标窗口/迫切值筛选、带主题模式切换的暂停恢复页、策略帮助与移动弹窗适配、Storybook 和 build validation 已完成
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## Active initiative

- Integration branch: `prd/repo-refresh-governance`
- Account suspension: `users.paused_at`、03:15 时区维护与 missed-run recovery 已交付；暂停会话通过 `GET /api/me` 读取状态并从 `/account/paused` 自助恢复，业务 API、API key、后台候选池和有效关注池均排除暂停账号。
- Governance retention: active-cycle reconciliation excludes completed history; completed member rows are pruned in best-effort 500-row transactions under a SQLite-persisted, shared 50000-row-per-minute budget while cycle summaries remain queryable.
- Existing production databases require a separately approved maintenance window before converting to incremental auto-vacuum with a full `VACUUM`.

## 实现里程碑

- [x] M1: schema / runtime config / governance snapshot tables
- [x] M2: system budgeted repo selection and cycle tracking
- [x] M3: `/api/admin/repos/overview` and `/api/admin/repos`
- [x] M4: `/api/admin/users` `repo_total` effective-pool semantics
- [x] M5: `/admin/repos` route, nav entry, budget entry hint, summary, activity grid, detail list
- [x] M6: Storybook fallback and owner-facing visual evidence
- [x] M7: final frontend build / storybook / e2e validation sweep
- [x] M8: system attempt ledger, failure recording, and active cycle reconciliation
- [x] M9: governance rebuild 分阶段 chunked writer，避免大候选池与 cycle member reconciliation 长时间占用 SQLite writer。
- [x] M10: completed-cycle member retention uses active-only queries, short best-effort deletes, and a restart-safe shared budget; new databases configure incremental auto-vacuum without scheduling a production full `VACUUM`.
- [x] M11: inactive account suspension: schema、daily maintenance、paused access guards、self-service resume、eligible-pool exclusions、admin filters、stable UI demo、浅色/深色 Storybook states and approved visual evidence.
- [x] M12: Dashboard 手动全量同步在 Release 阶段冻结 `latest / balanced / capacity` 新鲜度策略，按有效仓库规模、共享快照与实时压力逐仓决定抓取或复用；审计结果写入 watcher、任务结果与 progress event，并由管理员详情摘要与分页接口展示。

## 本轮收口

- `/admin/repos` 补齐概览/明细分离的 loading 与 error 状态，避免单一请求失败污染整页反馈。
- 活动图新增屏幕阅读器等价摘要与颜色图例，同时把页面与格子颜色迁回 token / dark-mode-safe 语义色。
- 预算入口从静态提示改为真实 CTA：跳转到任务中心订阅同步页签并自动展开“订阅同步设置”弹窗。
- “订阅同步设置”弹窗统一系统预算术语，将标题、Dashboard 新鲜度与链路用时帮助按钮收紧为 32px 可访问点击区；新鲜度提示以高层文案说明三档策略、1–30 分钟边界与压力约束，移动端以视口高度上限和内部滚动保留完整弹窗入口。
- 治理页文案收口到中文优先表达，补充“颜色 / 目标窗口 / 迫切值”解码说明，并把仓库明细改成以排序、目标窗口与迫切值为先的比较型信息结构。

## Attempt 账本收口

- `repo_refresh_governance_snapshots` 记录 `system_last_attempt_at/status/error`，用于区分 system 最近尝试、system 最近成功与实际刷新新鲜度。
- `repo_refresh_governance_cycle_members` 记录 `attempt_status/error`，cycle member 在本轮 system 选中后只要对应 release work item 到达 `succeeded` 或 `failed` 终态即完成；成功才更新 `system_last_success_at`。
- release work item 成功、失败、deadline/recovery 失败都会调用同一 governance attempt 记录路径；interactive demand 复用或提升 system 已选中的 work item 时，不会吞掉 system selection credit。
- 治理快照重建会 reconciliation 历史 active cycle：只补结算 system 选中时间之后的终态 work item，避免选中前的旧成功误完成当前轮。
- 治理快照重建分为 stale cleanup、snapshot upsert chunks、member reconciliation chunks、snapshot completion 与 cycle reconciliation 阶段；snapshot/member chunk size 固定为 500，并在 tracing 中记录 chunk count 与最大 chunk elapsed。
- `/api/admin/repos` 与 `/admin/repos` 返回并展示 system attempt 状态；活动图保留实际新鲜度颜色，同时用失败角标和明细 badge 解释 system 尝试结果。

## 闲置账号暂停收口

- `users.paused_at` 记录暂停时刻；每日 03:15 使用应用默认时区幂等标记连续 30 天未活动账号，并在重启后补跑当天遗漏轮次。
- 认证层保留暂停会话；`GET /api/me` 直接返回有效账号状态和 `paused_at`，不触发访问同步、业务副作用或活动时间写入。业务 API 与 API key 一律返回 `account_paused`。
- `/account/paused` 通过 `POST /api/me/resume` 自助恢复，展示恢复、排队、SSE 同步、完成和入队失败重试状态；页面复用应用画布并保留浅色、深色、跟随系统主题控件。恢复写入先于同步入队，入队失败不会回滚账号恢复。
- 有效关注池、订阅同步、发布候选、brief backfill、AI 历史与翻译重试候选，以及用户任务执行入口均排除暂停账号；翻译批次的新认领、旧批次回收和执行前检查同样排除暂停账号，暂停前已排队的暂停项会重新等待恢复而不阻塞同批活跃项。管理员可按 enabled、paused、disabled 查看有效状态。

## 明细筛选收口

- 仓库明细列表新增服务端全量筛选：`target_windows` 多选、`urgency_min` / `urgency_max` 范围、真实目标窗口 options；前端搜索、老化、窗口与迫切值筛选统一 300ms 防抖自动应用，并将 `全部 / 仅超 24 小时 / 仅未成功` 三个互斥状态收口为单个下拉选择。
- 目标窗口与迫切值保留各自的筛选面板，并在触发按钮右侧展示下拉箭头，和状态下拉形成一致的筛选控件 affordance。

## Dashboard Release 新鲜度收口

- `POST /api/sync/all` 异步任务通过 `dashboard_adaptive_release_freshness` payload 标记启用动态策略；单项同步、系统订阅同步与旧 `sync.access_refresh` 任务保持既有 30 分钟语义。
- Release 阶段开始时冻结评估时刻、策略档位、有效仓库集合、队列压力和活跃 Dashboard 任务数。窗口严格钳制在 1–30 分钟，压力只放宽窗口，不跳过有效仓库或拒绝同步。
- `admin_runtime_settings.dashboard_release_freshness_profile` 只接受 `latest / balanced / capacity`，缺省为 `balanced`；共享快照缺失时按 1 人计算并保留 `snapshot_missing`。
- `repo_release_watchers` 保存逐仓窗口、决策和评估 JSON；`result_json.release_freshness_assessment` 与 `release_freshness_assessed` 事件保存任务级汇总。管理员查询仅限 `sync.access_refresh`，沿用 watcher 生命周期清理。
- `release.releases` 继续表示兼容总量；`current_run_releases`、抓取/写入/分页字段只统计本轮 work item，`reused_fresh_count` 与 `reused_running_count` 单独记录缓存和进行中任务复用。
