# 演进记录（全局 Repo Release 复用与访问触发增量同步）

## 生命周期

- Lifecycle: active
- Last: 2026-06-26

## 历史摘要

- 2026-06-26: `sync.access_refresh` 失败链路补齐诊断闭环：Star 阶段现在会写入 `task.progress(stage=star_failed)` 与 access-refresh task log，管理员可直接看到 `error_kind`、`error_stage`、`elapsed_ms`、`timeout_ms`、`http_status` 与 `error_chain`；同轮只读 probe 也确认 101 到 GitHub 会出现 `HTTP 200` 后 body 读取超时 / EOF，问题更接近上游传输链路而非业务逻辑分支。
- 2026-06-26: 新增本地主诊断回路 `web/e2e/access-sync-admin-link.spec.ts`，用同一条 mock task state 证明 `Dashboard 点击同步 -> /api/sync/all?return_mode=task_id -> task-access-click -> Admin Jobs 实时任务详情` 的 `task_id` 一致；此前的 Storybook 截图只保留为 Admin 失败详情 UI 预览，不再作为该链路的主证据。
- 2026-06-26: 交互式 `sync.access_refresh` 的 Star 阶段补齐与订阅同步一致的可恢复网络重试预算；当 GitHub GraphQL / 网络短时超时或连接失败时，会先做 3 次退避重试，再决定是否把顶部“同步”任务判为失败，避免偶发上游抖动直接落成前台错误。
- 2026-06-24: release smart 批处理源查询改为先按目标 `release_id` 收口、再按 repo 内排序键回查 `previous_tag_name`，并补充 `repo_releases(repo_id, COALESCE(published_at, created_at, updated_at), release_id)` 表达式索引，避免少量 release 的翻译/润色请求在生产上退化成整批可见 release 的全局窗口排序慢查询。
- 2026-06-25: 订阅同步的 release fan-out 不再通过 `load_release_ids_for_repo_ids` 在 release phase 前后各扫一次共享 `repo_releases` 差集；repo release worker finalize 现在持久化本轮新增 `release_id` 集合，后续 translation / smart preheat 直接复用当前任务命中的 work item 结果，避免大 repo 上再次触发 repo 级全量 release 扫描。
- 2026-06-24: GitHub rename 仓库的 REST repo-read 现在使用独立的 redirect-enabled client，并把 REST / GraphQL base URL 明确挂到 `AppState`；`sync.releases.repo_read`、followers、received events、stargazers 与 public repo metadata 不再把 GitHub 返回的 `301 Moved Permanently` 误判成 repo 读取失败。
- 2026-06-24: `sync_subscription_events` 插入改为通过 sqlite writer 串行提交；`repo_release_watchers` 与 `sync_subscription_events` 历史裁剪在 writer permit 不可得或 SQLite busy 时降级为可观测的 best-effort skip，避免订阅同步后台清理继续放大为新的写锁竞争。
- 2026-06-23: `store_sync_state_value` 也改为通过 `sqlite_writer.begin_immediate_with_priority` 运行，补齐 `sync.access_refresh` / `sync.subscriptions` 的 `sync_state` 写锁保护；Dashboard 的同步进度泡泡现在在点击 Sync 后立即打开，不再依赖 hover 才出现。
- 2026-06-23: `replace_starred_repos` 改为通过 `sqlite_writer.begin_immediate_with_priority` 运行，并按调用方区分 foreground/background lane，避免 `sync.access_refresh` / `sync.subscriptions` 在 SQLite WAL 写锁竞争下继续把 `failed to clear starred_repos` 冒泡给用户；Dashboard 现在会在 `task.running` 与 `star_refreshed` 之间展示独立 warmup 态，而不是把该窗口继续渲染成“等待后台任务开始”。
- 2026-05-03: 订阅同步提速方向冻结为“不降频、不扩大 freshness window”；Release 阶段新增 worker 热配置、repo 级 conditional request 状态和 timeout 退避，`ReleaseEvent` 仅作为快速发现信号提升 repo demand，Discussions Announcements 继续独立表示仓库公告；Admin Jobs 新增订阅同步列表页和独立工作流详情页。
- 2026-05-05: `sync.subscriptions` 的 Star、Release Queue、Social、Notifications 长阶段新增节流后的根任务 `task.progress` 阶段内进度事件；详情 diagnostics 识别 `release_progress` 等增量事件，前端通过既有 SSE 后刷新详情链路显示阶段内实时成功、失败与 Release 写入数。
- 2026-05-05: Admin Jobs 订阅同步详情的运行中阶段总览改为在 `result_json` 尚未写入时从当前 `task.progress` 事件派生 Star、repo collect、Release queue、social 与 Inbox 统计，避免执行时间线已有进展但阶段卡片仍显示等待或零值。
- 2026-05-08: Release queue 的 task claim、repo release claim 与 watcher attach 事务切换为提前获取 SQLite writer slot，解决主连接池放大后 `sync.subscriptions` 因 `database is locked` / `SQLITE_BUSY_SNAPSHOT` 连续失败的问题。
- 2026-05-09: 定时订阅同步增加取数预算与水位语义：Release 默认小窗口抓取并记录真实扫描/新增/更新/未变统计；Starred 已有水位时只 upsert 最近窗口；followers 与 owned repo stargazers 达到页预算时按 partial 处理，避免浅窗口误删历史成员；Release worker 默认并发收敛到 8。
- 2026-05-04: 明确跳过的订阅同步记录不进入设置弹窗最近链路用时；跳过任务详情展示“已跳过”与未执行阶段语义。
- 2026-05-02: repo release work item 的 `deadline_at` 升级为硬业务超时；订阅同步等待 shared release queue 时会主动收敛过期 pending watcher，避免根任务长期 running 后让后续计划任务持续跳过。
- 2026-04-27: Admin Jobs 定时任务页新增全局自动获取间隔、最近三次 `sync.subscriptions` 链路用时和任务详情抽屉入口；该配置与账号访问状态无关。
- 2026-04-12: 已交付；PR #41; shared repo release cache + access refresh + scheduler social/inbox sync
