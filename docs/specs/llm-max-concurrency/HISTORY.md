# 演进记录（可配置 LLM 并行调度（取消固定限流））

## 生命周期

- Lifecycle: active
- Last: 2026-03-10

## 变更记录

- 2026-03-09: 新建规格，冻结 `AI_MAX_CONCURRENCY`、无固定限流、管理员并发状态改造范围与验收标准。
- 2026-03-09: 完成后端 permit scheduler、管理员并发状态 API、README / docs-site / e2e 同步，并通过 `cargo test`、`bun run build`、`bun run e2e -- admin-jobs.spec.ts` 本地验证。
- 2026-03-10: 根据主人确认的最新 UI 口径，LLM 调度页移除独立并发状态卡片，仅保留 24h 聚合摘要，并把调用列表排序固定为“进行中 -> 排队中 -> 终止态 / 组内按创建时间倒序”。
- 2026-03-10: 根据 review-loop 修正 retryable LLM 调用在退避窗口的状态回写，确保 permit 已释放后调用重新显示为 `queued`，且不影响实时任务列表原有的按创建时间倒序排序。
- 2026-03-10: 根据 review-loop 为 LLM 调用列表补齐专用排序索引，并为 `Retry-After` 增加最小退避下限，避免热路径整表排序与 0ms 重试抖动。
- 2026-03-10: 根据 review-loop 为 `AI_MAX_CONCURRENCY` 增加 Tokio semaphore permits 上限校验，避免超大误配置触发启动期 panic，并补齐越界配置回归测试。
- 2026-03-10: 根据 review-loop 用轻量观测状态覆盖吸收 `running -> queued/terminal` 的短暂落盘窗口，既保证管理接口不会读到半完成快照，也避免管理页反向压低 LLM 吞吐。
- 2026-03-10: 根据 review-loop 把 LLM 状态分组排序收敛为主列表显式排序模式；任务详情里的关联 LLM 调用恢复按创建时间倒序展示。
- 2026-03-10: 根据最新 review-loop 补齐 source / requested_by / parent_task 过滤场景下的排序索引，并让 override 中的 `started_at` 参与主列表时间筛选与总数计算，同时移除前端对服务端分页结果的二次重排。
- 2026-03-09: 根据 review-loop 补上 `AI_MAX_CONCURRENCY` 空字符串回退默认值 `1` 的配置兼容，并将 blank-value 容忍范围限制在该变量本身。
- 2026-03-09: 根据 review-loop 恢复 retryable LLM 请求的 `Retry-After` / 指数退避，避免取消固定节流后出现重试风暴。
- 2026-03-09: PR #34 已创建并更新到 `fd53622`，GitHub checks 全绿，review-loop 清零，spec-sync 收敛完成。
