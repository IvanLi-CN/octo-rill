# 管理员 LLM 多模型路由实现

## Current State

管理员多模型设置、有序 failover、模型健康冷却、逐模型状态、50 小时活动接口、活动视图与可分享调用排障筛选均已实现。用户隔离的遗留 `translation_work_items` 和 `translation_batches` 仍由本主题管理其有序模型画像；全局内容工作身份不由本主题定义。

## Verification

- Rust tests: `cargo test`
- Rust lint: `cargo clippy --all-targets -- -D warnings`
- Web checks: `cd web && bun run lint`、`cd web && bun run build`
- Storybook: `cd web && bun run storybook:build`
- E2E: `cd web && bun run e2e -- admin-jobs.spec.ts`
