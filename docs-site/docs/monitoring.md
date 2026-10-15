---
title: 性能监控接入
description: OctoRill Prometheus、Grafana 与只读诊断工具的通用接入合同。
---

# 性能监控接入

OctoRill 的性能观测默认关闭，启用后在独立 listener 提供带 Bearer token 的 `/metrics`。指标来自进程内事件和限频状态采样，scrape 只读取内存快照，不触发业务 SQL、GitHub 或 LLM 请求。业务进程不依赖 Prometheus 或 Grafana 才能启动和处理请求。

## 应用配置

```dotenv
OCTORILL_METRICS_ENABLED=true
OCTORILL_METRICS_BIND=127.0.0.1:58091
OCTORILL_METRICS_TOKEN_FILE=/run/secrets/octorill_metrics_token
OCTORILL_METRICS_SAMPLE_INTERVAL_SECONDS=5
```

token 文件只给 Prometheus 抓取身份使用。默认 listener 是 loopback；跨容器接入时把应用与 Prometheus 放进私网网络，并显式调整 bind 和网络 ACL，不把 metrics 端口发布到公网。

## 共享 Prometheus/Grafana

把 `ops/observability/prometheus/prometheus.yml` 中的 target 加入现有 Prometheus 的私有 scrape 配置，使用 `bearer_token_file` 指向部署侧 secret，并保留 `service`、`environment`、`instance` 三个 target label。把 `ops/observability/grafana/provisioning` 和 `grafana/dashboards` 导入一个独立 folder；datasource UID 可以替换，但不要覆盖共享栈其他项目的 datasource 或规则。

recording rules 与告警样例位于 `ops/observability/prometheus/rules/`，默认没有被示例配置加载。部署者应先确认阈值、持续时间、no-data 行为和通知 contact point，再显式加入规则文件。

## 独立监控 Compose

首次启动前显式创建 secret 文件，并确保应用使用同一个 metrics token：

```console
cd ops/observability
umask 077
openssl rand -hex 32 > prometheus/secrets/octorill_metrics_token
openssl rand -base64 32 > secrets/grafana_admin_password
docker compose up -d
```

把 `prometheus/secrets/octorill_metrics_token` 以只读方式挂载给应用并设置 `OCTORILL_METRICS_TOKEN_FILE`；不要把文件内容提交到仓库。样例固定 Prometheus 3.15.0 和 Grafana 13.2.3，Prometheus 使用 7 天/1 GiB 默认留存，Grafana 管理入口只绑定 `127.0.0.1:53000`，配置、dashboard 和 secret 以只读挂载提供。应用容器应加入 `octorill-monitoring` 私网并让 target 主机名解析为 `octorill`；主机进程部署则替换 Prometheus target 为受控私网地址。

## 只读诊断

```console
scripts/octorill-observe --url http://127.0.0.1:9090 --token-file /run/secrets/prometheus_read_token overview
scripts/octorill-observe --backend grafana --url http://127.0.0.1:53000 --datasource-uid octorill-prometheus --org 1 --token-file /run/secrets/grafana_token --json compare http
```

CLI 只接受固定的 `overview`、`http`、`sqlite`、`queues`、`upstream`、`health` 和 `compare` 命令，不接受任意 PromQL。默认窗口为 15 分钟，最长 24 小时；`compare` 默认比较前一个等长窗口，也可通过 `compare --baseline-from <时间> --baseline-to <时间>` 指定基线。每条曲线最多 1000 点，查询并发 2，单次超时 10 秒，响应上限 4 MiB。输出会标记 `ok`、`degraded` 或 `unavailable`，并报告最后样本时间与 freshness；缺样本或超过 60 秒未更新不会被转成零值。

## 排查

- target 不可达：先确认应用 metrics 已启用、独立 token 文件可读、listener 绑定地址和私网 ACL，再看 Prometheus target 的 HTTP 状态；不要用浏览器登录 cookie 代替 scrape token。
- sampler stale：查看 `octorill_sampler_stale`、`octorill_sampler_last_success` 和 `octorill_sampler_failures_total`；它表示状态采样没有在 30 秒内成功，不等同于队列为空。
- SQLite 背压：先看 foreground `octorill_sqlite_wait_seconds_bucket` 的 p95，再区分 operation、priority、outcome；不要把阶段分位数相加当作总耗时。
- 队列积压：对照 `octorill_queue_depth` 与 `octorill_queue_oldest_age_seconds`，未来定时工作不会进入 runnable backlog；进程重启后 sampler 从持久化表重建水位。
- 上游异常：按 provider、operation、outcome 区分 timeout、403、429、HTTP 5xx 与 LLM fallback；告警规则默认不启用，由部署侧配置通知。

## 101 适配边界

项目交付的是通用 exporter、查询目录、dashboard、规则和两种接入样例。101 上的 Prometheus/Grafana 版本、网络、datasource、provisioning 和机器 GET 查询路径由部署侧事实源核对；本项目不写入真实地址、凭据，也不在本轮修改线上 target 或告警通知。当前 CLI 的固定查询沿用监控平台已有只读权限，不额外承诺跨项目数据隔离。
