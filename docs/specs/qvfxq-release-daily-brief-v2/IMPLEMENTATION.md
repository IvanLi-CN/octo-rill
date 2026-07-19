# 实现状态（Release 日报内容格式 V2 与历史快照修复）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-16
- Last: 2026-07-19
- Summary: 已交付；fast-track / canonical brief markdown validator + refresh drift repair landed；日报生成已复用 release_smart 的 valuable/compare 事实链路，低信息 release 不再补伪摘要
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/product.md`: 日报内容结构改为 `项目更新 + 获星与关注`
- `docs/product.md`: 日报 release 要点默认中文倾向、release_smart 复用与 AI 失败兜底口径
- `docs/specs/README.md`: 登记本 spec，收口后写入 PR 号与状态
- `docs/specs/qvfxq-release-daily-brief-v2/SPEC.md`: 同步实现状态、视觉证据、release_smart 复用语义与验证结果

## 计划资产（Plan assets）

- Directory: `docs/specs/qvfxq-release-daily-brief-v2/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: Storybook docs / canvas

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 spec 并冻结日报 V2 / 历史修复 contract。
- [x] M2: 后端生成链路升级为 V2 正文，并补齐短链接 / fence 清洗 / 原位刷新。
- [x] M3: 内容刷新任务、admin diagnostics 与相关回归测试落地。
- [x] M4: Storybook、文档、视觉证据与快车道收口完成。
- [x] M5: 日报 summary / polish prompt 补齐默认简体中文契约；AI 不可用或 parse failed 时使用中文提示式 fallback，避免直接复用原始 release notes bullet。
- [x] M6: 日报 release 要点复用 `release_smart` 的 valuable / compare fallback 语义，低信息 release 不再补伪摘要。

## 验证结果

### Verification results

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test` → `386 passed`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `cd web && bun run e2e -- e2e/release-detail.spec.ts` → `21 passed`

## 变更记录

- 2026-07-19: 日报 release 要点改为复用 `release_smart`，低信息 release 不再生成伪摘要，历史 brief refresh 继续原位修复。
