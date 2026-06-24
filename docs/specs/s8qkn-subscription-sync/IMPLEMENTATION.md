# 实现状态（全局 Repo Release 复用与访问触发增量同步）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Last: 2026-06-24
- Summary: 已交付；PR #41; shared repo release cache + access refresh + scheduler social/inbox sync；Admin Jobs 支持全局自动获取间隔和最近定时任务链路用时详情；repo release deadline 现在会硬收敛 queued/running work item；订阅同步提速追加 Release worker 热配置、GitHub release conditional request、ReleaseEvent 快速 demand、`/admin/jobs/subscriptions` 列表页和 `/admin/jobs/subscriptions/{task_id}` 工作流详情页；跳过记录不进入设置弹窗最近链路用时，跳过详情显示“已跳过”；运行中详情阶段总览会在 `result_json` 尚未写入时从实时 `task.progress` 事件派生，Release Queue 等长阶段会持续写入节流后的根任务阶段内进度事件；Release queue 的 claim / attach 写事务已改为提前获取 SQLite writer slot，避免多连接 WAL 下的陈旧快照写锁升级失败；定时订阅同步已改为 Release 小窗口增量抓取、Starred 水位浅同步、Social partial snapshot 保护和真实 Release 扫描/新增/更新/未变统计；本次补丁把 `replace_starred_repos`、`store_sync_state_value`、social activity snapshot、feed activity event 与 `sync_subscription_events` 写入都接入 sqlite writer，订阅历史裁剪在 writer pressure / SQLite busy 下改为可观测的 best-effort skip，避免 `sync.subscriptions` 的后台写放大成新的 claim/heartbeat 锁竞争或任务告警；GitHub REST repo-read 现在使用独立 redirect-enabled client，并支持测试替换 canonical REST base，避免 rename 仓库的 `301 Moved Permanently` 被误判成 release/social repo 读取失败；release smart 批处理源查询现在先锚定目标 `release_id` 再回查前序 tag，并补充 repo 级排序表达式索引，避免少量 release 请求触发整批可见 release 的全局窗口排序
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)
