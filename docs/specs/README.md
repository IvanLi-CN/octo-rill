# 规格（Spec）总览

本目录用于管理工作项的**规格与追踪**：记录范围、验收标准、任务清单与状态，作为交付依据；实现与验证应以对应 `SPEC.md` 为准。

> Legacy compatibility: historical repos may still contain `docs/plan/**/PLAN.md`. New entries must be created under `docs/specs/**/SPEC.md`.

## 快速新增一个规格

1. 生成一个新的规格 `ID`（推荐 5 个字符的 nanoId 风格，降低并行建规格时的冲突概率）。
2. 新建目录：`docs/specs/<id>-<title>/`（`<title>` 用简短 slug，建议 kebab-case）。
3. 在该目录下创建 `SPEC.md`。
4. 在下方 Index 表新增一行，并把 `Status` 设为 `待设计` 或 `待实现`，并更新 `Last`。

## 状态（Status）说明

仅允许使用以下状态值：

- `待设计`
- `待实现`
- `跳过`
- `部分完成（x/y）`
- `已完成`
- `作废`
- `重新设计（#<id>）`

## Index（固定表格）

| ID | Title | Status | Spec | Last | Notes |
| --- | --- | --- | --- | --- | --- |
| n6zd8 | 管理员面板一期（首登管理员 + 用户管理） | 已完成 | `n6zd8-admin-panel-user-management/SPEC.md` | 2026-02-25 | local implementation completed |
