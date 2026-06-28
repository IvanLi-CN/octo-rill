# 实现状态（管理员 LLM 多模型路由与故障切换）

## Summary

- 待实现；目标是在现有 admin runtime / LLM runtime 基础上补齐多模型顺序配置、模型级冷却切换、预算按实际选模收敛，以及管理台状态展示。

## 当前覆盖

- 现状仅支持单一 `AI_MODEL`。
- `admin_runtime_settings` 仅持久化 `llm_max_concurrency` 与 `ai_model_context_limit`。
- 管理端设置弹窗仅支持并发与输入长度上限。

## 计划实现项

- [ ] M1: `admin_runtime_settings.llm_models_json` migration + seed/backfill + runtime snapshot。
- [ ] M2: `ai.rs` 选模、冷却、预算按实际模型计算、状态接口扩展。
- [ ] M3: 管理台多模型编辑、逐模型状态显示、Storybook 夹具更新。
- [ ] M4: Rust / Web / Storybook / E2E 验证与视觉证据落盘。
