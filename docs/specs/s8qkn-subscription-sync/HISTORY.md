# 演进记录（全局 Repo Release 复用与访问触发增量同步）

## 生命周期

- Lifecycle: active
- Last: 2026-05-02

## 历史摘要

- 2026-05-02: repo release work item 的 `deadline_at` 升级为硬业务超时；订阅同步等待 shared release queue 时会主动收敛过期 pending watcher，避免根任务长期 running 后让后续计划任务持续跳过。
- 2026-04-27: Admin Jobs 定时任务页新增全局自动获取间隔、最近三次 `sync.subscriptions` 链路用时和任务详情抽屉入口；该配置与账号访问状态无关。
- 2026-04-12: 已交付；PR #41; shared repo release cache + access refresh + scheduler social/inbox sync
