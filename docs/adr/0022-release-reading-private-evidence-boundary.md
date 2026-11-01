# ADR 0022: Release 阅读的私有差异依据边界

## Status

Accepted

## Context

Release 正文不足时，授权用户可以使用正确基线的 GitHub 代码差异形成阅读结论。私有仓库的差异可能包含尚未公开的文件名、提交标题、补丁和构建身份；同一 Release 又可能通过公开发布入口、搜索索引、缓存或未登录页面被读取。

## Decision

差异证据只进入服务端冻结快照和授权阅读结果。公开 Release 页面、公开 API、搜索和共享缓存只使用基于已公开 Release 原文的投影；它们不得用私有代码差异派生标题、正文、日报或搜索文本。服务端根据当前请求的仓库可见性和发布状态选择 audience/variant，并在读取投影时校验对应 source hash；客户端或模型不能声明 audience，也不能把内部 evidence 字段直接返回给读者。

证据采集是 `release.evidence` 后台任务。请求路径只创建或复用按 audience 隔离的 durable snapshot，并返回 queued/running/ready/failed 状态；GitHub compare、commit、tree、blob 和 file 读取均在 worker 中完成。snapshot 通过 task 去重、租约、有限重试和指数退避恢复；过期租约在启动或运行时恢复为 queued。资源索引以 repository、audience、用户范围、资源类型和资源键去重，授权索引不能被公开索引复用。

依赖差异证据的内容工作项在 snapshot ready 前保持 pending，failed 保持不可领取；worker 发布 ready 后才唤醒内容调度器。网络响应分页、树截断、超出文件/提交/blob 上限或缺少 blob 内容都属于不完整证据，不能生成“无变更”的替代结论。

处理权限变化、仓库可见性变化或公开发布状态变化时，服务端重新准备证据并重新校验发布面。差异读取失败、权限失效和模型等待保持各自状态，不能降级成无有效变更信息。

## Consequences

- 授权 Feed、详情和日报可以消费同一份差异事实，但公开读者只能看到公开原文结果。
- source snapshot、compare SHA、覆盖范围和内容指纹保留在内部审计面；机器核验材料可在授权阅读中折叠，日报省略。
- 新的搜索、缓存或公开 API 必须复用 audience 校验，而不能按“最新投影”直接读取全局结果。
- 私有证据不会因为 Release 被发布就自动变成公开内容；公开化需要重新通过服务端权限与发布状态检查。

## Alternatives Rejected

- 把私有差异直接写入共享 Release 正文被拒绝，因为公开入口无法可靠区分正文来源。
- 由客户端声明 private/public audience 被拒绝，因为客户端声明不能作为权限事实。
- 为每个用户复制同一份差异结果被拒绝，因为它破坏全局去重、租约和统一结果写入边界。
