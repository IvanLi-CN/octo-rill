# 仓库刷新治理页与预算调度收敛

## 背景 / 问题陈述

现有后台把用户“仓库数”展示成宽口径总数，容易把 owned baseline、disabled 用户和有效关注池混在一起，导致运营对实际调度压力与仓库新旧程度产生误判。与此同时，`sync.subscriptions` 的 system release demand 仍以“全池全量挂队列”为主，缺少预算窗口、全量闭环状态与老化治理解释面。

## 目标 / 非目标

### Goals

- 新增后台一级独立页 `/admin/repos`，作为“仓库治理”唯一展示入口，用于展示有效关注池、system budget 调度与仓库老化。
- 固定“有效关注池”语义：
  - `starred_repos` 恒纳入；
  - owned repo 仅当 `users.include_own_releases=1` 时纳入；
  - `users.is_disabled=1` 或 `users.paused_at IS NOT NULL` 的用户整池排除。
- system release 调度改为“每 10 分钟预算窗口内最多挑选一批仓库”，默认预算字段为 `repo_refresh_system_budget_per_window`，并与 Admin Jobs 共用同一 runtime config。10 分钟是调度预算窗口，不是全池刷新完成 SLA。
- 排序合同固定为：
  - `watcher_user_count DESC`
  - `watcher_repo_total_sum ASC`
  - `cached_stargazer_count DESC`
- 其中：
  - `watcher_user_count` 按同一用户去重；
  - `watcher_repo_total_sum` 按关系来源不去重，同一用户对同一 repo 的 `starred + owned` 需要累计两次；
  - `cached_stargazer_count` 使用 best-effort 本地缓存，允许过时，未知值落后于已知值。
- 新增 repo 治理物化快照与 system full-cycle 跟踪，页面读取和 scheduler 选仓都只读快照，不在请求链路里临时跨多表重聚合。
- 连续 30 天未访问的账号由每日维护任务标记暂停；暂停账号主动恢复后才重新进入有效关注池并触发访问同步。
- 已完成 cycle 的 member 明细在周期闭环后分批删除，只保留 cycle 汇总；存量明细通过可让步的在线维护任务渐进收敛。
- `/api/admin/users` 与用户详情中的 `repo_total` 改为有效关注池口径，并把 owner-facing 文案同步改成“有效关注仓库数”。

### Non-goals

- 不为长尾 repo 追加 24h/72h 的硬保底 SLA；只通过软目标频段和 aging 提升调度优先级。
- 不把交互式 `sync.access_refresh`、手动 sync、公开 release 访问 demand 并入这 10 分钟 system budget。
- Dashboard 全量同步的动态 Release 新鲜度只调整交互 demand 的复用窗口，不消费 system budget，也不改变系统调度闭环。
- 不把仓库老化信息塞回 `/admin` 主仪表盘；治理信息只在新的 `/admin/repos` 独立页展示。
- 不在治理页面读取链路里直接请求 GitHub；页面只读本地快照与共享队列状态。

## 范围（Scope）

### In scope

- DB migration:
  - `users.paused_at`
  - `admin_runtime_settings.repo_refresh_system_budget_per_window`
  - `starred_repos.repo_stargazer_count(_updated_at)`
  - `owned_repo_star_baselines.repo_stargazer_count(_updated_at)`
  - `repo_refresh_governance_snapshots`
  - `repo_refresh_governance_cycles`
  - `repo_refresh_governance_cycle_members`
  - `repo_refresh_governance_retention_state`
  - completed-cycle retention index on `repo_refresh_governance_cycles`
  - `admin_runtime_settings.dashboard_release_freshness_profile`
  - `repo_release_watchers` freshness assessment fields and audit index
- scheduler:
  - 有效关注池聚合
  - repo governance snapshot rebuild
  - fixed-order priority ranking
  - budgeted system repo selection
  - system full-cycle freeze/attempt/complete semantics
- admin HTTP APIs:
  - `GET /api/admin/repos/overview`
  - `GET /api/admin/repos`
  - `GET/PATCH /api/admin/jobs/sync/runtime-config` 扩展 budget 字段
  - `GET /api/admin/users` `repo_total` 新口径
  - `GET /api/me` 暂停会话状态读取与 `POST /api/me/resume` 自助恢复
- web admin:
  - `/admin/repos` route
  - `AdminHeader` 导航项“仓库治理”
  - summary cards / activity grid / filtered repo list
  - budget 只读展示与入口提示，实际编辑收口到 Admin Jobs 的“订阅同步设置”弹窗
- web account recovery:
  - `/account/paused` 暂停账号恢复页与访问同步 SSE 状态，复用应用全局画布并保留主题模式切换
  - 管理员用户列表的 `all / enabled / paused / disabled` 状态筛选
- Storybook fallback / visual evidence for the new admin page

### Out of scope

- 改造公开 release 仓库登记页的职责边界。
- 引入新的 GitHub 在线探测或页面级实时数据拉取。
- 在普通用户界面展示或配置 Release 新鲜度策略。

## 功能与行为规格（Functional / Behavior Spec）

### 有效关注池

- scheduler 与后台展示共享同一口径：
  - `starred_repos`
  - `owned_repo_star_baselines` only when `include_own_releases=1`
  - disabled 用户整池排除
- `/admin/users` 的 `repo_total` 表示该用户当前有效关注池中的去重 repo 数，而不是“历史处理过的所有仓库总数”。

### Budget 调度

- system window 固定为 10 分钟；其语义是“本窗口最多选中多少个 repo 发起 system release 尝试”，不是“10 分钟内刷新完整个仓库池”。
- 每次 scheduler window 先用 set-based SQL 重建 active pool 快照，并按固定排序写入 `priority_rank`。
- 软目标频段定义为：
  - `target_window = ceil(priority_rank / budget_per_window)`
  - `target_interval_minutes = target_window * 10`
- 目标频段是按当前排序和预算推导的软目标窗口，用于解释 repo 在 system 预算下大约应多久被再次尝试；它不承诺 GitHub API 一定成功，也不代表全池必须在 10 分钟内完成。
- system 选仓顺序固定为：
  - 未有 system success 的 repo 优先；
  - 其后按 `system_age / target_interval` 形成的 `urgency_score` 倒序；
  - 再按 `priority_rank ASC`
- 每轮最多挑选 `repo_refresh_system_budget_per_window` 个 repo 挂入 shared repo release queue。
- 交互式/手动 demand 不消费该 budget；如果某 repo 已被本轮 system 选中，而对应 release work item 后续被交互式需求复用或提升，work item 到达 `succeeded` 或 `failed` 终态时仍必须结算本轮 system attempt。
- 失败的 system attempt 记录 `system_last_attempt_at/status/error`，但不更新 `system_last_success_at`；成功的 attempt 同时更新 `system_last_success_at` 和实际刷新成功时间。

### Dashboard 手动刷新新鲜度

- 只有 `POST /api/sync/all?return_mode=task_id` 创建的 `sync.access_refresh` 任务携带 Dashboard 自适应标记；其他访问同步继续使用共享缓存的 30 分钟默认窗口。
- Release 阶段开始时冻结评估时刻、有效仓库数量、Release 队列数量、Release worker 并发、活跃 Dashboard 自适应任务数和管理员策略档位。每个仓库随后独立计算 1–30 分钟窗口。
- 压力分档分别按 `(queued + running) / repo_release_worker_concurrency` 与活跃 Dashboard 任务数计算，最终压力取两者最大档位；压力只增加窗口，不减少有效关注仓库覆盖。
- `latest / balanced / capacity` 使用固定的规模、共享人数和压力加权矩阵，结果钳制在 1–30 分钟。共享人数读取治理快照，缺失时按 1 计算并记录 `snapshot_missing`。
- 任务 `result_json.release_freshness_assessment`、`release_freshness_assessed` 事件和 `repo_release_watchers` 逐仓字段共同构成审计合同。管理员任务详情展示摘要，并通过分页接口查询仓库决策明细。
- `release.releases` 保留复用缓存后的兼容总量；`current_run_releases` 与抓取/写入字段只统计本轮实际处理的 work item。

### 全量 cycle

- system full-cycle 在开始时冻结成员集。
- cycle member 的完成条件是“本轮 system 选中后，对应 release work item 已处理到终态”，不是“必须成功刷新”。终态包括 `succeeded` 与 `failed`。
- cycle 完成条件只看冻结成员：
  - 新入池 repo 进入下一轮；
  - 离池 repo 不得永久阻塞当前轮完成；
  - 当前轮完成后写入“上次完成全量更新时间”。
- 已选中但卡在旧语义下的 active cycle 必须在治理快照重建时自动 reconciliation：若系统选中时间之后已经存在终态 release work item，则补写 member `completed_at/attempt_status/attempt_error` 与 snapshot `system_last_attempt_*`，并按新语义推进 cycle 计数。
- reconciliation、终态补账与候选选择只能读取 `status='active'` 的 cycle；已完成历史不得进入在线治理查询。
- cycle 完成后以最多 500 行的短事务删除其 member 明细，cycle 汇总行继续保留。
- 存量历史以 SQLite 持久化的共享时间窗限流，每分钟最多删除 50000 行；预算跨进程 owner 与重启连续生效。SQLite busy 或前台写入积压时立即让步，并在后续维护轮次继续。
- 新建 SQLite 数据库在启用 WAL 前配置 incremental auto-vacuum；既有生产库不得由在线任务自动执行 full `VACUUM` 转换，转换仍需单独批准的维护窗口。

### 闲置账号暂停

- 每日 03:15 按 `APP_DEFAULT_TIME_ZONE` 执行一次幂等维护，重启时补跑当天错过的轮次。
- `COALESCE(last_active_at, created_at) <= now - 30 days` 且未禁用、未暂停的账号写入 `paused_at`；所有角色均适用。
- 登录不会解除暂停或更新暂停账号的 `last_active_at`。
- 暂停会话允许的接口面仅为 `GET /api/me`、`POST /api/me/resume` 与退出登录；不新增第二个自状态接口。
- `GET /api/me` 对暂停会话保持可用，返回用户有效 `account_status=paused` 与 `paused_at`，`access_sync` 为 `none`，且不触发业务数据或访问同步副作用，也不更新 `last_active_at`。
- 主动恢复通过 `POST /api/me/resume` 清除 `paused_at`、更新 `last_active_at`，随后创建或复用 `sync.access_refresh`；入队失败不回滚恢复状态。
- 状态优先级为 `disabled > paused > enabled`。暂停账号的业务 API 与 API key 请求返回稳定错误码 `account_paused`。

### 治理页

- `/admin/repos` summary 固定包含：
  - 去重仓库数
  - 压力值
  - 上次完成全量更新时间
- 压力值定义为：
  - `sum(max(0, min(overdue_ratio, 4) - 1)) / budget_per_window`
  - 语义是“按当前预算清掉超期积压还需多少个 10 分钟窗口”
- 活动图：
  - 每格代表一个 repo
  - 顺序按治理优先级
  - 颜色按实际最后成功刷新时间（任意来源）分桶
  - 不把交互 demand 的表面新鲜度误当成 system 频段完成
  - 需要能暴露 system attempt 失败状态，避免绿色新鲜度掩盖 system 闭环卡住或失败事实
- 明细列表：
  - 按迫切值与优先级排序
  - 支持搜索 repo full name
  - 支持老化筛选，`全部 / 仅超 24 小时 / 仅未成功` 三个互斥状态收口为单个下拉选择
  - 支持按目标窗口多选筛选，选项来自 `GET /api/admin/repos` 返回的真实 `target_window_options`
  - 支持按迫切值范围筛选，前端用滑杆表达范围，API 使用 `urgency_min` / `urgency_max`
  - 搜索、老化、目标窗口与迫切值筛选在用户停止操作后 300ms 自动应用，并重置到第 1 页；状态下拉、目标窗口筛选、迫切值筛选作为三个并列控件展示，右侧均提供下拉箭头提示，其中目标窗口与迫切值打开各自的筛选面板
  - 同时解释实际刷新时间、system 最近尝试、system 最近成功与目标窗口：
    - 实际刷新时间：任意来源成功刷新后的表面新鲜度
    - system 最近尝试：本轮或最近一次 system 账本终态，可成功也可失败
    - system 最近成功：最近一次 system 尝试成功时间
    - 目标窗口：按排序和预算推导的软目标，不是硬 SLA
  - 桌面与窄屏均需稳定展示

## 验收标准（Acceptance Criteria）

- Given 同一 repo 同时存在 `starred` 与 `owned` 关系
  When 计算治理排序
  Then `watcher_user_count` 只把同一用户算一次，`watcher_repo_total_sum` 按关系来源重复累加。

- Given `repo_refresh_system_budget_per_window=1000`
  When 任一 10 分钟 system scheduler window 触发
  Then 附着到 shared repo release queue 的 repo 数量不超过 1000。

- Given 某 repo 仅被交互刷新而未被本轮 system 选中
  When 查看治理页
  Then 活动图颜色可显示为新，但其 system `urgency/band/full-cycle` 不提前结算。

- Given 某 repo 已被本轮 system 选中
  When 该 repo 的 release work item 被 interactive demand 复用并最终成功
  Then 当前 cycle member 完成，`system_last_attempt_status=succeeded`，`system_last_success_at` 更新，同时实际刷新来源保留 interactive 事实。

- Given 某 repo 已被本轮 system 选中
  When 对应 release work item 最终失败
  Then 当前 cycle member 完成，`system_last_attempt_status=failed` 且记录错误，但 `system_last_success_at` 不更新；该 repo 不得在同一个 active cycle 内重复抢占 system budget。

- Given 历史 active cycle 中存在已选中且选中后已有终态 work item 的 member
  When 治理快照重建
  Then reconciliation 自动补写 attempt 与 completed 状态，不需要人工 SQL 修复。

- Given active cycle 开始后新增或移除 repo
  When 当前轮继续推进
  Then 新入池 repo 归下一轮，离池 repo 不阻塞本轮闭环完成时间。

- Given `/admin/users` 与用户详情展示 `repo_total`
  When 用户关闭 `include_own_releases`、账号被 disabled 或 paused
  Then owned baseline 不再虚增其有效关注仓库数，disabled/paused 用户的有效关注仓库数归零。

- Given 用户连续 30 天未访问
  When 每日账号维护任务执行
  Then 用户被标记暂停，且不再进入仓库池、发布候选、订阅同步和用户后台任务。

- Given 暂停用户重新登录并点击“立即恢复”
  When 恢复状态已持久化但访问同步入队失败
  Then 用户仍保持启用状态，可返回首页并重试同步。

- Given 暂停用户持有有效会话
  When 调用 `GET /api/me`
  Then 返回 `account_status=paused` 与 `paused_at`，不创建访问同步、不产生业务副作用且不更新活动时间。

- Given 暂停账号通过会话或 API key 调用业务接口
  When 请求不属于 `GET /api/me`、`POST /api/me/resume` 或退出登录
  Then 返回 `403 account_paused`。

- Given 暂停会话打开 `/account/paused`
  When 用户查看或切换浅色、深色、跟随系统主题
  Then 页面不引入独立背景层，恢复操作、主题控件与页脚在各主题下均可访问。

- Given 治理库存在已完成 cycle 的大量 member 历史
  When 治理重建与在线清理运行
  Then 在线查询只扫描 active cycle，历史以最多 500 行短事务渐进删除，所有 owner 与重启共享每分钟 50000 行预算，cycle 汇总仍可查询。

- Given 新建 SQLite 数据库初始化治理功能
  When 应用配置运行时数据库
  Then 在 WAL 启用前配置 incremental auto-vacuum，且不会自动对既有生产库执行 full `VACUUM`。

- Given 管理员在仓库明细中选择目标窗口 `W1` 与 `W3`，并把迫切值范围设为 `2.0-4.0`
  When 前端防抖后请求 `GET /api/admin/repos`
  Then 请求包含 `target_windows=1,3`、`urgency_min=2.0`、`urgency_max=4.0`，返回的 `items` 与 `total` 都只统计匹配全量筛选条件的仓库。

## Visual Evidence

- source_type: `ui_demo`
  scenario: `paused account recovery idle state, light theme`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `1440x900`
  viewport_strategy: `chrome-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A`
  submission_gate: `approved`
  evidence_note: 证明暂停账号页在无演示控件或覆盖层的浅色桌面稳定 mock 场景中，复用应用画布，恢复、返回首页、退出登录、主题控件与页脚可同时清晰呈现。
  PR: include
  ![暂停账号恢复浅色桌面证据](./assets/paused-account-light-desktop.png)

- source_type: `ui_demo`
  scenario: `paused account recovery idle state, light theme`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `393x852`
  viewport_strategy: `chrome-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A`
  submission_gate: `approved`
  evidence_note: 证明暂停账号页在精确 `393x852` CSS px 浅色移动视口中没有 Demo 控件覆盖页脚，按钮、状态、主题控件与版本信息均清晰可读。
  PR: include
  ![暂停账号恢复浅色移动证据](./assets/paused-account-light-mobile.png)

- source_type: `ui_demo`
  scenario: `paused account recovery idle state, dark theme`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `1440x900`
  viewport_strategy: `chrome-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A`
  submission_gate: `approved`
  evidence_note: 证明暂停账号页在无演示控件或覆盖层的深色桌面稳定 mock 场景中，深色画布来自应用主题而非页面独立背景，恢复操作、主题控件与页脚均清晰可用。
  PR: include
  ![暂停账号恢复深色桌面证据](./assets/paused-account-dark-desktop.png)

- source_type: `ui_demo`
  scenario: `paused account recovery idle state, dark theme`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `393x852`
  viewport_strategy: `chrome-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A`
  submission_gate: `approved`
  evidence_note: 证明暂停账号页在精确 `393x852` CSS px 深色移动视口中没有 Demo 控件覆盖页脚，恢复操作、主题控件与版本信息均清晰可读。
  PR: include
  ![暂停账号恢复深色移动证据](./assets/paused-account-dark-mobile.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-admin-repos--evidence-desktop`
  state: `desktop governance`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `1440x1200`
  viewport_strategy: `playwright-viewport`
  sensitive_exclusion: `N/A`
  submission_gate: `pending-owner-approval`
  evidence_note: 证明 `/admin/repos` 在桌面视口下同时展示有效关注池 summary、可访问活动图图例、单跳预算 CTA，以及使用状态下拉、目标窗口与迫切值范围筛选的仓库明细。
  ![仓库治理桌面证据](./assets/admin-repos-desktop.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-admin-repos--evidence-narrow-tablet`
  state: `narrow tablet governance`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `768x1180`
  viewport_strategy: `playwright-viewport`
  sensitive_exclusion: `N/A`
  submission_gate: `pending-owner-approval`
  evidence_note: 证明 `/admin/repos` 在窄平板视口下仍能稳定展示 summary、预算 CTA、活动图图例与明细筛选，不退化为不可滚动的密集表格。
  ![仓库治理窄屏证据](./assets/admin-repos-narrow-tablet.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-admin-repos--evidence-desktop`
  state: `desktop governance with system attempts`
  evidence_note: 证明 `/admin/repos` 在桌面视口下明确区分实际刷新新鲜度、system 尝试成功/失败、system 成功时间与软目标窗口。
  ![仓库治理桌面 system 尝试证据](./assets/admin-repos-desktop-attempts.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-admin-repos--evidence-narrow-tablet`
  state: `narrow tablet governance with system attempts`
  evidence_note: 证明 `/admin/repos` 在窄平板视口下仍能展示 system 尝试成功/失败、失败原因与软目标窗口说明。
  ![仓库治理窄屏 system 尝试证据](./assets/admin-repos-narrow-attempts.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-admin-jobs--subscription-sync-settings-auto-open`
  state: `subscription sync settings dialog auto-open from governance cta`
  evidence_note: 证明仓库刷新 budget 的唯一编辑入口已经收口到任务中心“订阅同步设置”弹窗，并支持从治理页 CTA 单跳自动展开。
  ![订阅同步设置预算弹窗证据](./assets/subscription-sync-settings-budget-dialog.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-task-type-detail--sync-access-refresh-freshness-audit`
  state: `adaptive dashboard release freshness audit, desktop browser viewport`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `1440x900`
  viewport_strategy: `storybook-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A`
  submission_gate: `pending-owner-approval`
  evidence_binding_sha: `68026dd100b38992e5e3480c77e6e4d41377a387`
  evidence_note: 证明正常 1440x900 浏览器视口首屏中，管理员任务详情的统计卡片按三列排列，并展示 balanced 策略、压力档位、窗口范围、本轮抓取与缓存复用统计。
  PR: include
  ![Dashboard Release 新鲜度桌面证据](./assets/dashboard-release-freshness-desktop.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-task-type-detail--sync-access-refresh-freshness-audit-mobile`
  state: `adaptive dashboard release freshness audit, mobile browser viewport`
  target_program: `mock-only`
  capture_scope: `browser-viewport`
  requested_viewport: `393x852`
  viewport_strategy: `storybook-viewport`
  margin_policy: `trim_only`
  evidence_surface: `page`
  sensitive_exclusion: `N/A`
  submission_gate: `pending-owner-approval`
  evidence_binding_sha: `68026dd100b38992e5e3480c77e6e4d41377a387`
  evidence_note: 证明正常 393x852 CSS px 移动浏览器视口首屏中，统计卡片保持单列可读，并可继续滚动查看 Release 新鲜度审计区。
  PR: include
  ![Dashboard Release 新鲜度移动证据](./assets/dashboard-release-freshness-mobile.png)

## 关系 / Supersede

- supersedes:
  - `subscription-sync` 中“**不新增专门的 repo release 管理后台页面**”这一旧非目标
  - `admin-panel-user-management` 中 `repo_total = starred ∪ owned baseline` 的旧宽口径定义
- related:
  - [subscription-sync](../subscription-sync/SPEC.md)
  - [admin-panel-user-management](../admin-panel-user-management/SPEC.md)

## 参考

- `docs/product.md`
- `docs/solutions/backend/sqlite-wal-write-transactions.md`
