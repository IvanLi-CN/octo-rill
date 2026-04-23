# 实现状态（多 GitHub 绑定与 LinuxDO 首登补绑改造）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-21
- Last: 2026-04-21
- Summary: 已交付；PR #117; fast-track / multi-github + linuxdo onboarding / 去主账号语义并收口运行时迁移；legacy schema cleanup deferred
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增索引行，并在 PR 收口后更新状态 / 备注。
- `docs/specs/2v2aw-multi-github-linuxdo-onboarding/SPEC.md`: 维护接口契约、视觉证据与 spec sync 结果。
- `docs/specs/2v2aw-multi-github-linuxdo-onboarding/contracts/db.md`: 明确 runtime 真相源与 legacy 保留边界，并记录后续 cleanup 建议。

## 计划资产（Plan assets）

- Directory: `docs/specs/2v2aw-multi-github-linuxdo-onboarding/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 `github_connections`、PAT owner 字段与 migration/backfill，完成多 GitHub 与 LinuxDO 补绑主链路。
- [x] M2: GitHub / LinuxDO OAuth 流程改造成“多 GitHub 连接 + LinuxDO 首登补绑”，并开放连接管理 API。
- [x] M3: Landing、`/bind/github`、Settings、Storybook、Playwright 与 owner-facing 视觉证据完成首轮落地。
- [x] M4: 去除“主账号”语义、完成运行时迁移收口并推进到交付路径。
