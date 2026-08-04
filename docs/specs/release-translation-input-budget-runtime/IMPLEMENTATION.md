# 实现状态（Release 翻译输入预算与运行时设置收口）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-13
- Last: 2026-07-07
- Summary: 已交付；local implementation completed; runtime ai_model_context_limit; release_detail chunk translation unified; visual evidence landed; release batch translation now accounts for output budget and retries split batches before falling back to single-release detail translation
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `README.md`: 删除 `AI_MODEL_CONTEXT_LIMIT` 环境变量说明，改为后台运行参数口径
- `.env.example`: 删除 `AI_MODEL_CONTEXT_LIMIT`
- `docs-site/docs/config.md`: 删除该环境变量，并说明输入预算来自后台运行参数/模型目录
- `docs/specs/release-feed-body-translation/SPEC.md`: 标注已被当前 contract 取代
- `docs/specs/admin-jobs-runtime-worker-settings/SPEC.md`: 标注已被当前 contract 取代

## 计划资产（Plan assets）

- Directory: `docs/specs/release-translation-input-budget-runtime/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: Storybook canvas

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 后端 runtime setting、状态接口与输入预算解析改成 `ai_model_context_limit`
- [x] M2: release feed / batch preheat 统一切到 `release_detail` 分块译文，不再按正文长度拒绝翻译
- [x] M3: 文档、Storybook/E2E 与视觉证据收口
- [x] M4: release detail batch 翻译按输入与输出预算共同分组，batch 响应截断或缺项时先拆小重试，再退回单条 detail/chunk 路径
