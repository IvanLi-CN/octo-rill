# 管理员 LLM 多模型路由实现

## Current State

管理员多模型设置、有序 failover、模型健康冷却、逐模型状态、50 小时活动接口、活动视图与可分享调用排障筛选均已实现。用户隔离的遗留 `translation_work_items` 和 `translation_batches` 仍由本主题管理其有序模型画像；全局内容工作身份不由本主题定义。

PostgreSQL `translation.generate` 执行器也已接入同一有序列表与 `llm_model_health`：按管理员顺序选模、冷却/凭证停放、限流切换候选，并记录 `final_model`/`fallback_count`；`AI_MODEL` 仅作持久化列表为空时的运行期 fallback，settings/health 读取失败不会静默回退。

## Verification

- Rust tests: `cargo test`
- Rust lint: `cargo clippy --all-targets -- -D warnings`
- Web checks: `cd web && bun run lint`、`cd web && bun run build`
- Storybook: `cd web && bun run storybook:build`
- E2E: `cd web && bun run e2e -- admin-jobs.spec.ts`
