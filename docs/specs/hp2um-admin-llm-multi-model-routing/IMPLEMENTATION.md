# 实现状态（管理员 LLM 多模型路由与故障切换）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-06-28
- Last: 2026-06-28
- Summary: 已交付；local implementation completed；管理员后台现已支持多模型顺序配置、模型级冷却切换、按实际选模计算输入预算，并补齐 Storybook / E2E / 视觉证据。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/hp2um-admin-llm-multi-model-routing/SPEC.md`
- `docs-site/docs/config.md`
- `docs-site/docs/quick-start.md`
- `.env.example`

## 计划资产（Plan assets）

- Directory: `docs/specs/hp2um-admin-llm-multi-model-routing/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: `admin_runtime_settings.llm_models_json` migration、启动 seed/backfill、runtime snapshot 与 live scheduler 同步。
- [x] M2: `ai.rs` 多模型选模、连续最终失败计数、10 分钟冷却、预算按实际候选模型计算、状态接口扩展。
- [x] M3: 管理台多模型编辑、排序/增删、逐模型状态展示、Storybook 夹具与 E2E 更新。
- [x] M4: 完成 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`web` lint/build/storybook/e2e 与视觉证据落盘。
