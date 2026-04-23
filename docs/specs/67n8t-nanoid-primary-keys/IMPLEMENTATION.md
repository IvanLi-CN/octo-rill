# 实现状态（全库主键 NanoID 化与公开标识收口）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-07
- Last: 2026-03-08
- Summary: 已交付；PR #29; checks green; review-loop clear; destructive SQLite rebuild
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `README.md`: 说明本地 ID 已统一为 NanoID，以及旧 SQLite 文件需重建。
- `docs/specs/README.md`: 新增并更新本规格状态。

## 计划资产（Plan assets）

- Directory: `docs/specs/67n8t-nanoid-primary-keys/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 统一后端 NanoID 生成/校验与所有本地主键类型定义
- [x] M2: 追加破坏性迁移并完成 schema/查询/外键切换
- [x] M3: 更新前端类型、管理员页面与关键测试/构建
- [x] M4: 完成快车道交付（验证、PR、review-loop、spec 同步）
