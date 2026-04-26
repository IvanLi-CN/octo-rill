# 实现状态（管理后台仪表盘与 rollup 统计）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Last: 2026-04-27
- Summary: 已交付；dashboard release-batch business counts now accept legacy `items[]` result payloads and persist summary-enriched task results
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/m2k8d-admin-dashboard-rollups/SPEC.md`
- `docs/specs/m2k8d-admin-dashboard-rollups/IMPLEMENTATION.md`
- `docs/specs/m2k8d-admin-dashboard-rollups/HISTORY.md`
- `docs/specs/m2k8d-admin-dashboard-rollups/contracts/http-apis.md`
- `docs/specs/m2k8d-admin-dashboard-rollups/contracts/db.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/m2k8d-admin-dashboard-rollups/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: Storybook

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 rollup 表、窗口化 dashboard 接口与系统时区统计口径
- [x] M2: 完成管理后台仪表盘页面、路由调整与 Recharts 图表
- [x] M3: 接入 scheduler 预聚合、补齐 Storybook stories 与视觉验收链路
- [x] M4: 为 `translate.release.batch` / `summarize.release.smart.batch` / `brief.daily.slot` 引入统一 business outcome 派生与 rollup 回填
- [x] M5: 在 dashboard 暴露 LLM 24h 健康摘要，并让趋势 / 今日分布 / 任务占比统一切换到 business-first 口径
- [x] M6: 兼容 `job_tasks.result_json = { items: [...] }` 的历史批任务结果，并把后台批任务持久化升级为 `{total/ready/missing/disabled/error, items}` superset
