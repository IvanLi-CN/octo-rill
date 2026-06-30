# 实现状态（全局 Repo Release 复用与访问触发增量同步）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Last: 2026-06-26
- Summary: 已交付；PR #41; shared repo release cache + access refresh + scheduler social/inbox sync；Admin Jobs 支持全局自动获取间隔和最近定时任务链路用时详情；repo release deadline 现在会硬收敛 queued/running work item；订阅同步提速追加 Release worker 热配置、GitHub release conditional request、ReleaseEvent 快速 demand、`/admin/jobs/subscriptions` 列表页和 `/admin/jobs/subscriptions/{task_id}` 工作流详情页；跳过记录不进入设置弹窗最近链路用时，跳过详情显示“已跳过”；运行中详情阶段总览会在 `result_json` 尚未写入时从实时 `task.progress` 事件派生，Release Queue 等长阶段会持续写入节流后的根任务阶段内进度事件；Release queue 的 claim / attach 写事务已改为提前获取 SQLite writer slot，避免多连接 WAL 下的陈旧快照写锁升级失败；定时订阅同步已改为 Release 小窗口增量抓取、Starred 水位浅同步、Social partial snapshot 保护和真实 Release 扫描/新增/更新/未变统计；本次补丁把 `replace_starred_repos`、`store_sync_state_value`、social activity snapshot、feed activity event 与 `sync_subscription_events` 写入都接入 sqlite writer，订阅历史裁剪在 writer pressure / SQLite busy 下改为可观测的 best-effort skip，避免 `sync.subscriptions` 的后台写放大成新的 claim/heartbeat 锁竞争或任务告警；GitHub REST repo-read 现在使用独立 redirect-enabled client，并支持测试替换 canonical REST base，避免 rename 仓库的 `301 Moved Permanently` 被误判成 release/social repo 读取失败；release smart 批处理源查询现在先锚定目标 `release_id` 再回查前序 tag，并补充 repo 级排序表达式索引，避免少量 release 请求触发整批可见 release 的全局窗口排序；订阅同步的 release fan-out 现在直接消费本轮 `repo_release_work_items.last_new_release_ids_json`，不再在 release phase 前后各扫一次共享 `repo_releases` 差集；交互式 `sync.access_refresh` 的 Star 阶段现在与订阅同步共享可恢复网络重试预算，避免 GitHub 短时 GraphQL/网络抖动直接把顶部“同步”判成失败；访问增量同步现已补齐 `star_failed` 诊断事件与 task log，管理员可以直接区分 timeout/connect/response/decode/http_status/local 等失败阶段，并把 101 上 `HTTP 200` 后 body 读取超时/EOF 的链路异常与业务逻辑失败分开；本轮再补管理面热点索引与查询改写，`/api/admin/jobs/overview`、`/api/admin/jobs/realtime`、最近订阅同步链路用时和 Dashboard LLM 24h 健康统计不再依赖已证实的全表扫描热路径；release governance rebuild、subscription history prune 与 llm retention cleanup 也补齐了结构化耗时/批次/降级埋点，方便把订阅同步慢点和管理面自发查询压力分开诊断；本地新增 Playwright 主诊断回路 `web/e2e/access-sync-admin-link.spec.ts`，专门证明 Dashboard 点击同步后返回的 `task_id` 会以同一条 `sync.access_refresh` 记录出现在 Admin Jobs 实时任务详情中

## 主诊断回路

- 命令：`cd web && npm run e2e -- --project=chromium e2e/access-sync-admin-link.spec.ts`
- 目标：在本地 mock-only 环境中证明 `Dashboard 同步按钮 -> POST /api/sync/all?return_mode=task_id -> task-access-click -> /admin/jobs 实时任务 -> 任务详情失败定类` 是同一条链路。
- 当前结果：已执行并通过。
- 合成探针：`scripts/probe_github_upstream.sh <github-url>` 可在只读环境下复跑 `101 -> GitHub` 的 DNS/connect/tls/ttfb/total/body 下载观测，用来区分“应用任务失败但上游探针成功”与“二者同时超时/截断”。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)
