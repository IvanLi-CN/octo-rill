# OctoRill 核心性能监控演进记录

## Origin

本主题由 Monitoring dashboard 需求、`docs/design/monitoring-dashboard.md` 与 ADR 0020 收敛而来；首期选择 Prometheus + Grafana 核心 metrics、固定只读诊断 CLI、独立 Compose 和共享栈接入样例。

## Boundary

采集入口与业务 API 分离，默认关闭且使用独立 token；项目提供通用接入能力，不把 101 的地址、凭据或共享 Prometheus 数据权限写入仓库。项目不实现服务端跨项目指标查询隔离，可信运维身份的 Prometheus 读取边界由部署侧管理。

## Compatibility

本主题不增加数据库 schema、不改变既有公开治理指标、不改变 SQLite 事务和取消语义。指标只从已有运行事件和只读 sampler 更新，监控后端不可用时业务继续运行。

## Follow-up

tracing、函数/SQL hotpath、CPU profile、日志平台和应用内性能页属于后续主题；线上 101 target 启用、连通性和告警通知属于部署侧后续授权。
