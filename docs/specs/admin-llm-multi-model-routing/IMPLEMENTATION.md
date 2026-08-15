# 实现状态（管理员 LLM 多模型路由与故障切换）

## 当前状态

- Lifecycle: active
- Implementation: 已实现
- Created: 2026-06-28
- Last: 2026-08-15
- Summary: 多模型路由与固定 50 小时活动接口、默认活动图/卡片切换、列聚合交互及独立刷新状态均已实现。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/admin-llm-multi-model-routing/SPEC.md`
- `docs-site/docs/config.md`
- `docs-site/docs/quick-start.md`
- `.env.example`

## 计划资产（Plan assets）

- Directory: `docs/specs/admin-llm-multi-model-routing/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: `admin_runtime_settings.llm_models_json` migration、启动 seed/backfill、runtime snapshot 与 live scheduler 同步。
- [x] M2: `ai.rs` 多模型选模、连续最终失败计数、10 分钟冷却、预算按实际候选模型计算、状态接口扩展。
- [x] M3: 管理台多模型编辑、排序/增删、逐模型状态展示、Storybook 夹具与 E2E 更新。
- [x] M4: 完成 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`web` lint/build/storybook/e2e 与视觉证据落盘。
- [x] M5: 固定 50 小时管理员活动接口、override 对账、历史模型排序与 Rust API 覆盖。
- [x] M6: 活动网格、视图切换、列聚合交互、独立刷新状态及 mock / Storybook / Playwright 覆盖。
- [x] M7: 完成最终质量门禁与桌面/移动视觉证据。
