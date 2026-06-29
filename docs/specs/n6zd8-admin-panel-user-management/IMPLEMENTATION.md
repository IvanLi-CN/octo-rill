# 实现状态（管理员面板一期（首登管理员 + 用户管理））

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-02-25
- Last: 2026-06-29
- Summary: 已交付；local implementation completed；`repo_total` 已切换为有效关注池口径，并由 #rap6f 接管仓库治理扩展面
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增规格索引。
- `docs/specs/n6zd8-admin-panel-user-management/SPEC.md`: 记录里程碑推进。

## 计划资产（Plan assets）

- Directory: `docs/specs/n6zd8-admin-panel-user-management/assets/`

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 完成 DB 迁移与认证门禁改造（首登管理员 + 禁用拦截）。
- [x] M2: 完成管理员 API（列表、筛选、更新）与保护规则。
- [x] M3: 完成前端管理员模块与交互。
- [x] M4: 补齐自动化测试并通过质量门禁。
- [x] M5: 用户管理列表改为紧凑双层列表页，并补齐 `repo_total/include_own_releases` 合同与视觉证据。
- [x] M6: `repo_total` 切换为“有效关注仓库数”口径；后续 repo governance 扩展由 #rap6f 承接。
