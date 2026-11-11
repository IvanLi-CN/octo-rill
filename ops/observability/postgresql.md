# PostgreSQL 18 观测说明

本文补充 [性能监控接入](../../docs-site/docs/monitoring.md) 的 PostgreSQL 侧观测边界。现有应用 exporter 继续提供 HTTP、队列、上游和进程指标；数据库连接、锁、事务和备份状态由 PostgreSQL exporter 或部署侧只读 collector 提供。

## 观测边界

- collector 只读取 `pg_stat_activity`、`pg_stat_database`、`pg_locks`、`pg_stat_bgwriter` 和备份/迁移报告，不执行业务查询，不读取表内容和 secret。
- target label 只保留 `service`、`environment`、`instance`、`database`、`application_name` 等低基数维度；禁止把用户 ID、request key、完整 SQL、连接串或错误正文作为 label。
- 前台和后台连接必须使用不同的 `application_name`，这样才能验证后台没有挤占前台保留容量；命名由 runtime 配置固定，不能每个请求动态生成。
- 备份 SHA-256、migration head、manifest 批次和恢复结果属于发布/迁移证据，不能伪装成连续时间序列指标。

## 最小采集集

| 信号 | 来源 | 目的 |
| --- | --- | --- |
| 活动/空闲/等待连接数 | `pg_stat_activity` | 连接容量与前后台隔离 |
| 获取超时、事务等待、查询耗时 | 应用 metrics | 判断 pool 背压与请求影响 |
| `deadlocks`、提交/回滚、缓存命中 | `pg_stat_database` | 识别事务冲突与读放大 |
| 锁等待与阻塞关系 | `pg_locks` + `pg_stat_activity` | 定位迁移、备份和长事务阻塞 |
| checkpoint、写放大、临时文件 | `pg_stat_bgwriter`/实例 exporter | 评估容量和恢复窗口 |
| WAL/备份时间点与最后成功时间 | 部署侧备份 collector | 证明可恢复性，不替代恢复演练 |

## 只读检查样例

```sql
SELECT application_name, state, count(*) AS connections
FROM pg_stat_activity
GROUP BY application_name, state;

SELECT datname, numbackends, xact_commit, xact_rollback, deadlocks, temp_files
FROM pg_stat_database
WHERE datname = current_database();

SELECT blocked.pid AS blocked_pid, blocking.pid AS blocking_pid,
       blocked.application_name AS blocked_app,
       blocking.application_name AS blocking_app
FROM pg_stat_activity AS blocked
JOIN pg_locks AS blocked_lock ON blocked_lock.pid = blocked.pid AND NOT blocked_lock.granted
JOIN pg_locks AS blocking_lock
  ON blocking_lock.locktype = blocked_lock.locktype
 AND blocking_lock.database IS NOT DISTINCT FROM blocked_lock.database
 AND blocking_lock.relation IS NOT DISTINCT FROM blocked_lock.relation
 AND blocking_lock.granted
JOIN pg_stat_activity AS blocking ON blocking.pid = blocking_lock.pid;
```

## 告警与容量门禁

阈值由部署侧结合实例规格和压测调整，但必须保留前台连接余量并记录调整理由。建议至少配置以下告警：

- 连接使用率连续 5 分钟超过 70%，或前台获取超时持续出现。
- 存在超过迁移窗口上限的长事务，或锁等待持续超过 1 分钟。
- `deadlocks`、SQLSTATE `40001`/`40P01` 重试率在验收窗口内非零且没有已记录的竞争解释。
- 备份最后成功时间超过保留窗口，或恢复检查没有当前版本的成功证据。
- migration/backfill 报告在两个采样周期内没有进度，或异常批次持续增加。

告警只触发排查，不自动扩容、删除数据、重试迁移或切换数据库。容量不足时后台 worker 退避，不能绕过 pool 直接创建连接。

## 与现有资产的关系

PostgreSQL 查询不加入 `scripts/octorill-observe` 的固定 PromQL 目录，原因是 exporter schema 和部署权限可能不同；该 CLI 仍负责应用 exporter 的有界只读诊断。部署侧可以把本页 SQL 或等价 exporter 指标接入现有 Prometheus/Grafana 私网，但必须保留低基数标签和 token 隔离。
