# Monitoring dashboard 需求与设计边界

本文整理 OctoRill 核心性能观测的已确认范围、由代码事实提炼的具体需求与验收要求，作为后续 topic Spec 的输入，不替代 Spec 或实施计划。

## 已确认的目标与范围

- 目标是支持跨时间的性能分析，能够区分 HTTP 请求慢、SQLite 竞争、任务积压、GitHub / LLM 上游慢及进程资源压力。
- 首期交付核心 metrics 与 Prometheus / Grafana 看板；单请求 tracing、函数 / SQL hotpath 和按需 CPU profile 保留为可选扩展，不进入首期交付范围。
- 使用入口为 Grafana 和固定查询的只读诊断工具；诊断工具供维护者与 Agent 使用，沿用看板口径。
- 同时交付接入既有 Prometheus / Grafana 的说明与可独立运行的监控 Compose 样例，复用指标、看板和规则。
- 首期提供基础告警规则与排查说明，部署侧明确启用并配置阈值及通知路由；通知由监控平台负责。
- 采集默认关闭；启用后使用独立 listener 和独立 scrape token。独立 Compose 样例显式启用，既有设施通过受控网络抓取。
- 机器查询沿用监控平台的只读权限；诊断工具固定查询 OctoRill，本期不新增服务端跨项目数据隔离层。
- 项目需要提供可复用的自托管集成能力，同时能接入维护者已经运行的共享监控设施。现有部署只是一种适配实例，不构成项目的固定地址、网络或凭据约束。
- 本轮进行只读事实调查与需求收敛；应用实现、构建交付和线上配置变更另行处理。

## 现状与证据

| 当前能力 | 仓库证据 | 对本需求的影响 |
| --- | --- | --- |
| JSON stdout、request-id、HTTP access log | `src/observability.rs`、`src/server.rs` | 可复用观测入口；metrics 需要由真实事件独立更新，不依赖日志级别或日志解析 |
| HTTP 中间件等待 `next.run` 返回后记录耗时 | `src/observability.rs` | 当前耗时止于响应生成，不能冒充响应 body 或 SSE 全生命周期 |
| 路由优先用 `MatchedPath`，缺失时回退原始 path | `src/observability.rs` | metrics 必须有独立路由归一化和固定 fallback，避免动态路径形成无界序列 |
| 专用 SQLite writer pool、前后台优先级、deadline、重试和准入控制 | `src/sqlite_write.rs`、`src/session_store.rs`、`docs/specs/sqlite-writer-coordinator/SPEC.md` | 已有明确的等待与执行阶段，适合在 source event 处接入指标 |
| LLM 等待数、在途数、并发额度和调用明细 | `src/ai.rs`、`docs/specs/admin-llm-scheduler-observability/SPEC.md` | 可复用运行状态，但不能以 scrape 时扫描调用明细的方式计算运行指标 |
| LLM 请求使用 `stream: false`，`first_token_wait_ms` 在 `send().await` 后测量 | `src/ai.rs` | 实际为上游 HTTP 响应等待，不应以真实 token 首字延迟命名新指标 |
| 多类持久化任务与独立 worker | `src/jobs.rs`、`src/translations.rs`、`src/content_processing.rs`、`src/sync.rs` | 必须分别定义逻辑工作、单次尝试、恢复和延后，避免重复计数 |
| 管理仪表盘业务 rollup | `docs/specs/admin-dashboard-rollups/SPEC.md` | 继续承担业务统计；性能时间序列不替代 raw status 与 business outcome |
| 匿名公开治理指标 | `src/public_metrics.rs`、`docs/specs/public-metrics-api/SPEC.md` | 已有 API 的字段、权限和缓存合同独立于新 exporter |
| 尚未提供 Prometheus / Grafana 交付面 | `Cargo.toml`、`src/server.rs`、`.env.example` | 首期需要新增 exporter、采集配置样例、看板和查询验证 |

架构文档仍有“仅慢请求与错误记 access log”的描述，但当前实现及快速成功请求测试已覆盖普通成功请求。本需求以实际事件为采集来源，不以旧文档中的日志过滤假设计算流量。

## 参考模式与项目适配

参考 Style Playbook 的 `Monitoring dashboard` Topic 与 `Codex Vibe Monitor` 样本，复用有界采集、低基数、历史与业务事实分工、文件化 provisioning、故障隔离及查询验收的方法。样本的 dashboard 数量、端口、留存、采样率、容量预算和特定代理请求阶段不直接继承。

推荐的数据流是：OctoRill 真实运行事件 → 进程内有界聚合 / 有界状态采样 → Prometheus scrape → Grafana 查询、看板与规则。SQLite 继续保存业务状态；日志继续承担已有 request-id 关联排查。首期不新增性能历史数据库，也不要求部署 Loki 或 trace backend。

## 核心需求

以下条目落实已确认的首期范围与职责边界。采集周期、留存和开销阈值属于实施前需要声明并实测的工程参数，不从参考项目或单个部署实例继承。

### 采集与指标语义

| 编号 | 需求 | 必须回答的问题 / 边界 |
| --- | --- | --- |
| MON-01 | 提供 Prometheus 兼容的应用 exporter；Counter、Gauge、Histogram 的单位、事件边界和 reset 语义进入指标目录 | 哪个事件被计数，重启后哪些值重置，哪些 Gauge 是当前快照 |
| MON-02 | 请求与任务路径只做有界内存更新；exporter 读取聚合快照 | 抓取不得触发 GitHub / LLM 请求、业务写入、全表扫描或逐请求外部发送 |
| MON-03 | HTTP 记录请求量、在途量、响应状态类别和响应生成耗时分布 | 覆盖快速成功、慢请求、4xx、5xx、超时和取消；每个阶段只记录一次 |
| MON-04 | HTTP body 与 SSE 生命周期单独观测 | 分开响应生成耗时、body 完成 / 错误 / 客户端丢弃、SSE 当前连接与持续时长；长连接不混入普通 API 延迟分位数 |
| MON-05 | SQLite 写路径拆分 writer wait、write-pool wait、BEGIN、transaction / COMMIT 与总操作耗时 | 区分排队慢、取连接慢、锁竞争、事务执行慢和提交超过 deadline；阶段分布不可直接相加成总体分位数 |
| MON-06 | SQLite 记录 writer 活跃 / 前后台等待、准入拒绝、cadence 合并、busy / locked、重试、deadline 和 best-effort skip | foreground retryable 503、background 延后、best-effort 跳过必须可区分；重试与最终逻辑操作结果分别计数 |
| MON-07 | 读池提供容量、已用 / 空闲连接及选定读操作组的 acquisition / 执行耗时 | writer pool 与 reader pool 分开；未测量阶段显示未知，不借用其他阶段或填零 |
| MON-08 | 各任务执行器记录等待量、在途量、可运行任务最老等待年龄、尝试耗时和结果 | 区分 job、repo refresh、内容处理与翻译等固定执行器；未来定时任务不算已逾期积压 |
| MON-09 | 任务恢复、claim backoff、retry / deferred / blocked / superseded / canceled 与最终业务结果分开 | 复用现有状态和业务结果定义；一次 provider 成功不能掩盖内容校验失败，取消和合并不能算业务失败 |
| MON-10 | GitHub / LLM 观测逻辑调用与单次 upstream attempt 的数量、耗时和结果 | 按固定 provider / operation / outcome 分类，覆盖连接错误、超时、429、403、重试和路由回退；403 不能统一视为限流 |
| MON-11 | LLM 分开调度等待、并发额度、在途量、上游响应等待与完整调用耗时 | 当前非流式请求的 `first_token_wait_ms` 实为响应头等待；新指标按真实阶段命名，不冒充 token 首字延迟，既有业务明细字段迁移不属于本次需求 |
| MON-12 | GitHub 配额 / 冷却与 repo refresh 压力提供不含账号身份的有界摘要 | 配额是按实际 credential / resource 作用域产生的状态；不得把不同账号剩余额度无条件相加为“全局额度” |
| MON-13 | 资源面覆盖进程 CPU 时间、RSS、启动时间、连接 / worker 预算，以及可用的 SQLite DB / WAL 文件大小 | 主机 / 容器指标可由共享基础设施补充；平台不支持、采样失败或无文件型数据库时明确 unknown / unavailable |
| MON-14 | 观测系统自报 sampler 最后成功时间、采集失败、丢弃 / 上限命中和功能启用状态 | 区分应用无流量、target 不可达、样本过期、sampler 故障和功能关闭 |

队列深度等持久化状态可以由异步、限频、带超时的有界查询更新；scrape 本身只读快照。启动恢复后必须重建当前水位，不能仅依靠进程启动后的增减计数推导数据库中已有的 queued / running。采样失败保留最后成功时间并标记过期，不能清零伪装为无积压。

### 基数、隐私与开销

- MON-15：为 method、route、operation、phase、priority、executor、outcome 等 label 提供固定集合或明确上限；未知值归入固定 fallback。未知 HTTP method、动态 SPA 路由、release tag 和 404 也必须有界。
- MON-16：用户 / 账号 / 仓库 / 任务 / 调用 / request ID、IP、原始 URL / query、SQL / 参数、prompt / response、token 和它们的 hash 均不得成为 metrics label。模型或 provider 名称如需维度，必须先定义规范化方式、运行时累计上限与溢出行为。
- MON-17：指标族、label 组合、Histogram buckets、registry 条目、采样队列和序列化结果必须有可验收上限。仅在 Grafana 限制 Top N 不能替代采集侧限额。
- MON-18：支持显式启停与能力状态。关闭、scrape timeout、Prometheus / Grafana 停机、采样失败或指标上限命中时，业务请求、session 写入、任务状态与持久化结果保持原有成功语义；有限观测资源释放不得延长业务 shutdown。
- MON-19：需在代表性流量与 SQLite 竞争场景下比较启用前后 CPU、RSS、吞吐和 p95 / p99；实现前记录本项目基线、测试环境和允许开销，超出预算须调整采集方案或显式重审，不能仅报告“测试通过”。预算不复制参考项目数值。

### 通用集成与实例适配

- MON-20：应用以环境变量配置启用状态、监听 / 抓取访问边界和采样策略；dashboard / rules 使用可替换的 datasource 绑定及 `service`、`environment`、`instance` 筛选。实例标识由部署配置或 scrape target labels 提供，不以随机进程 owner ID 作为长期筛选维度。多 target 的这些标识必须唯一，不引入多应用实例共享 SQLite 的新能力。
- MON-21：提供复用既有 Prometheus / Grafana 的接入合同，以及包含 Prometheus、Grafana、私网、持久卷和只读配置挂载的独立监控 Compose 样例；同一份指标、看板与规则能够接入两种方式，应用启动不以监控后端在线为前提。真实地址、凭据和容量配置由运行者注入。
- MON-22：采集默认关闭，启用时通过独立 listener 提供 `/metrics` 并校验独立 scrape token；scrape 身份与机器查询身份分离。通用默认监听 loopback，容器样例显式绑定受控网络且不发布公网端口，跨主机由部署侧配置 TLS。抓取面独立于公开业务 API / 浏览器 session，不进入 SPA fallback 或 credential CORS；无凭据或错误凭据不能读取指标。
- MON-23：项目仓库保存通用 exporter 配置说明、scrape 样例、dashboard JSON、recording / alert rules 和验证工具；自家实例真实域名、内部地址、网络成员、凭据、通知接收人和部署变更记录保留在部署侧事实源。
- MON-24：实例适配验收应确认真实 scrape target、网络可达性、认证方式、datasource 映射、实例筛选、provisioning 路径与规则归属。仅看到容器运行或 dashboard 成功导入不算完成。

### 看板与查询

| 视图 | 核心内容 |
| --- | --- |
| 服务总览 | 流量、错误、延迟、资源与当前瓶颈线索；显式标识数据新鲜度和功能关闭 |
| HTTP / SSE | 路由组吞吐、响应生成分位数、body / cancel、长连接独立趋势 |
| SQLite | reader / writer pool、writer wait、BEGIN / transaction / COMMIT、背压、准入、重试与 deadline |
| 任务 / 内容处理 | 各执行器深度、最老可运行等待、运行并发、退避与最终业务结果 |
| GitHub / LLM | provider / operation 延迟与错误、限流、调度等待、回退与冷却摘要 |
| 观测数据质量 | target up、scrape error / duration、sampler freshness、序列 / 样本量与限额状态 |

这些是观测视图分组，不固定成六个 dashboard；实际布局按排障路径组织。

- MON-25：dashboard、recording rules 与 alert rules 由文件管理，使用稳定命名与 UID，能在全新环境恢复。不得覆盖共享栈其他应用的 datasource 或规则。
- MON-26：实例间可以汇总吞吐与错误；延迟分位数从相同桶边界的 Histogram buckets 聚合计算，不能平均各实例 p95。Gauge 只在同一业务语义与采样范围内聚合。
- MON-27：PromQL 必须匹配实际 exporter 的 Histogram 类型；请求延迟首期推荐 classic Histogram，具体 buckets 待目标延迟与容量预算确定。
- MON-28：无流量、缺样本、低样本量、stale、采集失败和真正为零要可区分；分位数显示对应样本量，计数趋势正确处理 Counter reset。
- MON-29：首期交付 target 不可达、前台延迟 / 5xx、SQLite 背压、队列积压、上游失败及 sampler 过期的基础规则与排查说明；延迟 / 比率类规则应有低流量样本门槛。规则由部署侧明确启用，并配置阈值、持续时间与通知路由；通知 contact point 由监控平台管理。
- MON-30：首期交付只读机器诊断工具，采用固定查询、受限窗口 / 返回大小 / 并发与独立只读凭据，输出与看板相同的口径；工具不提供任意 PromQL / SQL / shell 参数。支持人类可读输出与结构化 JSON，包含目标实例、绝对查询时间窗、样本量、freshness 和明确的 unavailable / degraded 状态，不从缺失数据推导根因。

只读诊断工具的最小查询集为服务总览、HTTP、SQLite、任务队列、上游与采集健康；支持比较同一实例两个时间窗。连接地址、datasource / org 绑定及只读凭据从部署配置获取；不能固定为维护者实例或要求复用其他项目的凭据。共享设施支持经 Grafana datasource 查询代理访问；独立部署可配置受控网络内的 Prometheus 只读查询入口。不得为了工具可达而公开 Prometheus。

机器身份沿用监控平台实际授予的只读权限，不新增服务端对其他项目指标的查询限制。CLI 的固定查询是使用合同，不是多租户安全隔离；独立 datasource UID 或凭据名称也不构成数据隔离承诺。

## 通用交付物

| 交付物 | 建议落点 | 必须包含的内容 |
| --- | --- | --- |
| 应用采集与 exporter | `src/observability.rs` 及其子模块，`src/config.rs`、`src/server.rs` | 有界 registry、生命周期埋点、后台 sampler、独立 listener、独立 token、启停与降级 |
| source event 接入 | `src/sqlite_write.rs`、`src/session_store.rs`、`src/jobs.rs`、`src/sync.rs`、`src/translations.rs`、`src/content_processing.rs`、`src/ai.rs` 等实际调用入口 | 覆盖真实生产路径；GitHub REST、GraphQL 和 webhook 投递路径都要核对，不能只修改 `src/github.rs` |
| 指标目录 | 后续 topic Spec 的指标合同 | 每个指标的名称、类型、单位、事件起止、label 白名单、零值 / 缺测、reset 和 cardinality 预算 |
| 监控部署样例 | `ops/observability/` | Compose、scrape 示例、持久卷、只读配置挂载、独立凭据注入与版本 / 协议兼容说明 |
| 看板和规则 | `ops/observability/grafana/` 与相邻规则目录 | datasource 参数化、dashboard / folder 稳定 UID、recording / alert rules、无数据和低流量语义 |
| 只读诊断工具 | `scripts/octorill-observe` | 固定查询、人类可读与 JSON 输出、时间窗比较、数据质量与查询限额、Grafana / Prometheus 访问适配 |
| 使用说明 | `.env.example`、`docs-site/docs/monitoring.md` 及内部架构 / 排查入口 | 两种部署方式、参数说明、告警启用、credential 分工、常见无数据 / stale / 背压排查 |
| 验证合同 | 现有 CI 与隔离验收环境 | 指标与生命周期、PromQL / rules、配置恢复、权限、基数、故障隔离和开销报告 |

这些是后续实现的交付边界，不表示上述新文件或能力已经存在。首期展示面为 Grafana 和诊断工具，不增加应用管理页内的性能图表或新业务统计接口。

## 实施前的定量配置与验证要求

| 参数组 | 要求 |
| --- | --- |
| 抓取与状态采样 | 声明 scrape interval / timeout、sampler interval / timeout、freshness 容忍窗；采样查询有索引与读预算，超时显示 stale，scrape 不查询业务数据库 |
| Histogram | 按普通 API、SQLite 等实际耗时量级声明 classic bucket 边界与最低有效样本数；发布前通过指标目录与 PromQL 一致性检查 |
| 内存与基数 | 声明 registry 最大序列、label 组合、动态类别累计上限、响应大小、采样资源上限及溢出行为；先计算桶展开后的序列预算，再用异常输入验证 |
| 历史容量 | 独立样例必须明确 retention time 与 size 上限；共享后端由平台配置其全局容量，接入新项目时评估增量，不由应用擅自修改 |
| 诊断查询 | 明确默认 / 最大窗口、最小 step、最大点数、返回大小、并发、超时与错误码；拒绝用户输入绕过固定查询模板 |
| 告警 | 为每条规则声明阈值、持续时间、样本门槛、no-data 行为、恢复条件与排查入口；部署者显式启用通知 |
| 开销 | 预先固定代表性 workload、无监控基线、允许的 CPU / RSS / 吞吐 / 延迟增量和重复测量方式；通过报告证明达标，不以 target up 代替 |

当前应用的慢日志阈值、writer deadline 等业务运行参数可作为图表参照，不能不经语义核对直接充当告警或监控开销预算。具体配置随后进入项目自己的指标合同与验证卡；定量配置未声明、未验证时，不得宣告监控实现验收完成。

## 验收场景

1. 全新环境按通用说明接入后，Prometheus target 可见，Grafana 使用可替换 datasource 查询真实指标；不需要复制维护者实例配置或代码补丁。
2. 同一套交付物能接入维护者现有共享栈，按 deployment / instance 筛选 OctoRill，并保持其他应用配置完整；线上变更需另行授权。
3. 注入普通成功、快速错误、慢响应、body 失败与 SSE 断连，核对计数及生命周期，确认长连接不污染普通请求延迟。
4. 制造 writer 占用、reader pool 压力、BEGIN busy、deadline、准入拒绝和 best-effort skip，指标能区分等待位置与最终业务结果。
5. 制造队列积压、未来定时任务、重试、冷却与进程重启，深度和最老可运行等待符合真实持久化状态，不把计划等待当作故障。
6. 注入上游超时、429、非限流 403、一次失败后恢复和内容校验失败，核对 attempt / logical call / final outcome 口径。
7. 发送大量唯一未知路径、method、release tag 等输入，确认 registry 与活跃序列在声明上限内，抓取内容不含敏感字段。
8. 在隔离验证环境停止 Prometheus / Grafana、阻断 scrape、使 sampler 超时或耗尽观测队列，业务成功语义保持完整，观测状态显示 degraded / unknown。
9. 核对所有面板与规则在无流量、低流量、重启 reset、sampler stale 和多个实例下的查询语义。
10. 在约定环境记录开销对照和容量结果；重型 / 集成验证在会话所属 Agent VM 或 CI 中执行，本轮需求讨论不运行这些验证。

## 方案取舍

推荐应用内低基数聚合 + Prometheus 历史 + Grafana 文件化交付，复用现有日志与业务管理页进行必要的逐条排查。它符合当前 Rust / SQLite 服务结构，也能适配已有共享设施；主要成本在于正确覆盖真实生命周期、限制采样查询负载和验证规则口径。

备选一是仅解析 JSON 日志生成图表：初期接入较轻，但无法可靠提供当前排队水位、取消边界和独立于日志过滤的计数，因此不建议作为核心性能指标来源。备选二是在应用内新建性能历史库和图表：可复用现有 UI，但引入重复历史存储、SQLite 观测写压力和额外维护面，因此不建议作为首期路线。

## 架构决定与范围边界

- 已确认：首期核心 metrics + Prometheus / Grafana，tracing / hotpath / profile 后置；项目通用性与自家部署适配必须同时满足。
- 已确认：Grafana + 只读诊断工具；接入既有监控服务 + 独立 Compose 样例；基础告警规则由部署侧启用和通知。
- 已确认：采集默认关闭，独立 listener + scrape token；容器样例显式启用。
- 已确认：机器查询沿用平台只读权限，工具固定查询 OctoRill，不增加服务端跨项目数据隔离层。
- 展示面为 Grafana 与诊断工具；tracing、hotpath、CPU profile、应用内新性能图表和多实例共享 SQLite 能力不进入首期。
- 地址、网络、org / datasource 绑定、凭据和通知接收人属于部署配置；指标语义、低基数、数据质量和故障隔离属于项目合同。

对应架构决定记录在 [ADR 0020](../adr/0020-performance-observability-boundary.md)。长期合同已收敛到 [monitoring-dashboard Spec](../specs/monitoring-dashboard/SPEC.md)；本文保留需求来源与设计边界，不将它解释为线上变更授权。
