# ADR 0020: 性能观测与业务统计、部署实例的边界

## Status

Accepted

## Context

OctoRill 已有 JSON 日志、管理员业务统计、LLM 调用明细和公开治理指标，但缺少跨时间的性能趋势。若复用业务 SQLite 保存性能历史或扩展现有管理图表承担监控，将增加写竞争并混淆任务状态、业务结果与运行性能；若把单个共享监控实例的地址和权限写进项目，又会妨碍其他自托管部署。

## Decision

- 首期由应用从真实运行事件产生低基数 metrics，Prometheus 保存性能历史，Grafana 通过文件化交付提供看板与基础规则；既有 SQLite、管理仪表盘和公开治理指标继续承担业务事实与统计。
- 性能观测提供 Grafana 和固定查询的只读诊断工具两种使用面；共享 Prometheus / Grafana 接入与独立监控 Compose 使用同一套指标、看板与规则。
- 采集默认关闭，启用后使用独立 listener 和 scrape token；业务成功语义独立于 exporter、sampler 和外部监控后端的可用性。
- 项目交付通用配置合同；实际域名、内部网络、org / datasource 绑定、凭据、留存容量和通知接收人由部署侧管理，不能硬编码为维护者的实例。
- 机器查询沿用监控平台的只读权限；诊断工具固定查询 OctoRill。本期不增加服务端跨项目数据隔离层，也不声称独立 datasource UID 提供了这种隔离。
- tracing、函数 / SQL hotpath、按需 CPU profile 与应用内新性能图表后置。

## Considered Options

- 仅从 JSON 日志生成图表：不足以可靠表达当前队列水位、取消边界和独立于日志过滤的计数，不作为核心指标来源。
- 在应用内新增性能历史库与图表：增加 SQLite 写压力及重复历史存储，不用于首期。
- 为诊断工具新增服务端跨项目查询隔离层：需要额外查询权限系统，不纳入首期范围。

## Consequences

任务尝试、最终业务结果、响应生成、body / SSE 生命周期以及 SQLite 不同等待阶段需要分别定义，不能用单个“成功率”或“总耗时”替代。接入共享设施仍需验证网络、认证、target、新鲜度和查询口径；target up 或 dashboard 导入成功本身不构成适配验收。

具体需求与验收见 [Monitoring dashboard 需求与设计边界](../design/monitoring-dashboard.md)。
