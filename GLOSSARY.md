# OctoRill

本词表区分 OctoRill 的产品视图、业务统计与运行性能观测，避免不同数据口径共用同一个“仪表盘”或“指标”名称。

## Language

**性能监控看板（Monitoring dashboard）**：面向 OctoRill 运维与性能排查的运行时趋势视图，用于观察请求、资源竞争、任务积压和上游调用的变化。
_Avoid_: 管理仪表盘、公开指标 API

**管理仪表盘（Admin dashboard）**：面向管理员的业务运行总览，展示用户规模、活跃、任务状态与业务结果等统计。
_Avoid_: 性能监控看板

**公开治理指标（Public governance metrics）**：可向匿名客户端公开的仓库刷新治理汇总，包含去重仓库数、系统压力和刷新新鲜度。
_Avoid_: 性能指标、Prometheus 指标
