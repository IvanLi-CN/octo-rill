# 实现状态（可配置 LLM 并行调度（取消固定限流））

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Last: 2026-03-10
- Summary: 已交付；PR #34; 24h summary-only UI; retry backoff requeues as queued with floor; main-list-only status grouping; non-blocking observable overrides
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `README.md`
- `.env.example`
- `docs-site/docs/config.md`
- `docs-site/docs/quick-start.md`
- `docs/specs/README.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: `AI_MAX_CONCURRENCY` 配置解析与 runtime semaphore 落地
- [x] M2: 管理员 LLM 状态 API / UI / mock / e2e 切换到并发语义，并同步新的列表排序/摘要展示口径
- [x] M3: README / `.env.example` / docs-site 配置说明同步
- [x] M4: PR、checks、review-loop 与 spec-sync 收敛完成
