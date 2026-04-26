# 演进记录（管理后台仪表盘与 rollup 统计）

## 生命周期

- Lifecycle: active
- Last: 2026-04-24

## 历史摘要

- 2026-04-18: 已交付；local implementation completed; Recharts dashboard + daily rollup analytics
- 2026-04-24: 修复“前台失败、后台零失败”的观测失真；dashboard 追加 business outcome 计数、30 天 rollup 回填与 LLM 24h 健康摘要
- 2026-04-27: Hotfix release-batch 结果兼容；dashboard/rollup/任务诊断改为兼容 legacy `result_json.items[]`，并把后台批任务结果写成 summary-enriched superset，避免业务失败再次被统计成 0
