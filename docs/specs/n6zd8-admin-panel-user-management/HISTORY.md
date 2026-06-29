# 演进记录（管理员面板一期（首登管理员 + 用户管理））

## 生命周期

- Lifecycle: active
- Created: 2026-02-25
- Last: 2026-02-25

## 变更记录

- 2026-02-25: 新建规格并冻结一期范围与验收标准。
- 2026-02-25: 实现完成并通过 Rust + Web 质量门禁验证。
- 2026-06-23: 扩展 `/admin/users` 列表合同，新增“项目处理仓库总数”与紧凑双层列表页目标。
- 2026-06-29: `repo_total` 口径被 [#rap6f](../rap6f-repo-refresh-governance/SPEC.md) 收口为“有效关注仓库数”，disabled 用户与未纳入 owned releases 的 baseline 不再计入该字段。
