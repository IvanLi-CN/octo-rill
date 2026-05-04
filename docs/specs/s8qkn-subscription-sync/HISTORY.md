# 演进记录（全局 Repo Release 复用与访问触发增量同步）

## 生命周期

- Lifecycle: active
- Last: 2026-05-03

## 历史摘要

- 2026-05-03: 订阅同步提速方向冻结为“不降频、不扩大 freshness window”；Release 阶段新增 worker 热配置、repo 级 conditional request 状态和 timeout 退避，`ReleaseEvent` 仅作为快速发现信号提升 repo demand，Discussions Announcements 继续独立表示仓库公告；Admin Jobs 新增订阅同步列表页和独立工作流详情页。
- 2026-05-02: repo release work item 的 `deadline_at` 升级为硬业务超时；订阅同步等待 shared release queue 时会主动收敛过期 pending watcher，避免根任务长期 running 后让后续计划任务持续跳过。
- 2026-04-27: Admin Jobs 定时任务页新增全局自动获取间隔、最近三次 `sync.subscriptions` 链路用时和任务详情抽屉入口；该配置与账号访问状态无关。
- 2026-04-12: 已交付；PR #41; shared repo release cache + access refresh + scheduler social/inbox sync
