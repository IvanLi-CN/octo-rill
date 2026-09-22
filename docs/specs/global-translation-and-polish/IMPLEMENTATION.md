# 全局翻译与润色工作模型实施

## Current State

全局内容处理调度器使用模型无关的工作与结果身份；`source_hash` 属于身份，`model_profile` 与配置指纹只保留为历史来源信息。尝试开始时按当时有效配置快照有序模型路由和安全配置。admission、手动重试和 worker claim 均在取得 SQLite 写事务后读取持久模型路由，再决定新工作的配置状态或写入 `attempt_started`，使配置写入与调度动作按数据库写锁排序。无效配置时工作进入可恢复的 `blocked_config`；有效配置更新或运行时重载触发事件驱动的恢复，不按计时器重试。前端把该状态显示为 pending，保留可读结果并继续轮询原请求。旧的用户隔离表仍作为历史事实保留。

## Approved Identity Update

后续身份切换必须将工作与结果投影身份统一为不含模型路由或配置指纹的规范资源、处理链路、变体、目标语言、源哈希和协议版本。每次新尝试在开始时快照当时有效的安全配置与有序模型路由；一次运行中的尝试保持该快照，后续尝试重新读取当前配置。敏感密钥只允许以不可逆指纹表示，实际命中的模型仍通过尝试到模型调用的关联记录。

数据库升级须保留既有尝试、模型调用、请求者关联和已发布输出的可追溯性。同一完整结果身份（包含 `source_hash`）下存在多个模型专属有效投影时，最近发布的有效投影成为唯一当前投影，同一发布时间以稳定 ID 决胜，其他记录只作为历史事实。对既有 `blocked_config` 工作按新身份归并；仅当没有源哈希完全匹配的有效当前投影时，自动排入一次恢复。已存在匹配投影的内容不重跑；恢复走正常调度器、并发和 provider 防护。有效配置更新或运行时配置重载会重新验证阻塞项；配置仍无效时不进行定时 provider 重试。界面将 `blocked_config` 显示为 pending 并继续轮询原请求，使用“等待模型配置恢复，恢复后会自动继续”；不增加手动重试入口或改变 API wire shape。源内容未变时不提供强制重生成命令。

兼容结构阶段只添加迁移 `0084` 所需 schema，不执行身份切换或历史回填。当前身份切换实现复用该 schema，不增加数据库迁移，并通过可暂停、幂等的运行时流程回填身份与投影。

## Original Global Cutover Shape

全局切换已经启用。以下步骤记录原有切换与回滚安全边界，不是本次模型无关身份升级的待执行步骤。

1. 在兼容版本中加入全局扩展表、`content_processing_control` 控制记录和启动前模式保护。默认模式为 `legacy`；该版本仍运行旧模型，但已经包含并应用所有切换前需要的迁移。
2. 兼容版本必须在 `global` 或 `rollback_freeze` 模式下阻止旧入口、旧调度器和直接缓存写入，返回可轮询的维护响应。它仍可启动、读取一般业务数据和保留旧内容处理事实，因此是数据库迁移后的最低回滚版本。
3. 在受控窗口暂停内容处理接收，等待运行中的旧批次到达终态；超时的批次只按既有租约恢复规则收口，不迁移为全局尝试。写入 `rollback_freeze`，使所有新内容处理请求得到 `503` 和状态轮询信息。
4. 从旧表和缓存只读取并写入旧事实观察：`ai_translations.entity_type IN ('release_smart', 'announcement_smart')` 与相应 `translation_work_items.kind`／尝试事件必须和翻译记录一并处理。有可显示缓存而无对应工作证据的记为 `legacy_cached`；存在不可一致解释的工作与缓存证据记为 `legacy_conflict`。不复制或修改旧行，不创建全局工作项、结果投影或虚构尝试。
5. 部署切换版本。它不携带新的数据库迁移，只读取已存在的扩展表，并在一个数据库事务中把模式从 `rollback_freeze` 切换为 `global`；提交后由全局调度器读取该控制状态并接管新的覆盖请求和交互入口。
6. 在稳定期只通过全局读模型展示状态，并持续监测未预期的旧写入。发现问题时暂停全局调度器并回退到兼容版本；兼容版本保持全局模式保护，避免恢复旧写入，随后以前向修复恢复服务。

## Existing Database Migration

当前实现使用迁移 `0078_content_processing_global_model.sql` 和追加迁移 `0079_admin_collection_read_budget_indexes.sql` 创建全局表、索引和控制记录；内容准入审计由追加迁移 `0086_content_work_admission_events.sql` 提供：

- `content_processing_control`：单行模式栅栏，取值为 `legacy`、`rollback_freeze` 或 `global`；记录切换代号和更新时间。它是旧新写入者共同读取的唯一切换事实。
- `content_work_items`：包含全局身份、不可变来源快照、工作级配置指纹、优先级、调度状态、租约关联、恢复元数据和取消／替代关系。当前唯一索引仍包含 `model_profile`，这与新批准的身份合同不一致。
- `content_batches` 与 `content_batch_items`：持久化调度批次、工作成员、分区、令牌估算、触发原因和批次结果，唯一 worker kind 为 `general`。
- `content_result_projections`：当前按规范资源、处理链路、变体、语言、协议和模型档案保存已验证投影、已发布来源哈希、当前工作项和活动工作项。新合同要求去除模型档案分组并只解析一个当前投影。
- `content_request_links`：保存请求者或系统生产者、授权快照、请求来源、交付模式、关联工作项和响应事实；它是重试竞争时仍要写入的关联记录。
- `content_attempt_events` 和 `content_attempt_llm_calls`：追加式的全局尝试和精确模型调用归因，仅保存安全元数据。
- `content_legacy_observations`：引用旧表的原始主键和只读分类，保存 `legacy_cached` 或 `legacy_conflict` 的判定依据；不复制旧内容、不反向修改旧表。
- `idx_notifications_thread_id`：为按全局通知线程读取 canonical source 提供 `thread_id` 前导索引；授权仍使用用户行单独校验。
- `idx_notifications_admin_canonical_source`：按通知线程、更新时间和稳定行 ID 支持管理读取的规范来源选择。
- `idx_translation_work_items_admin_entity_kind_attempt`：按实体、处理种类和尝试次数支持管理候选集筛选。
- `0085_admin_collection_activity_indexes.sql`：只添加活动时间窗与日报最新调用查询所需的表达式索引，不改写历史行。

这些原始切换迁移不执行 `DROP TABLE`、表改名、数据重建、旧行 `UPDATE`、旧行 `DELETE` 或把旧数据插入全局工作／结果／尝试表。`translation_work_items`、`translation_requests`、旧尝试事件和 `ai_translations` 继续存在；应用仅把它们当作旧事实读取。

## Identity Upgrade Migration

迁移 `0084_content_processing_model_independent_identity.sql` 是身份升级的兼容结构阶段。它作用于已包含当前全局 schema（迁移至 0083）的 SQLite 数据库：

- 只添加 `content_work_identities`、`content_work_identity_members`、`content_current_result_projections`、`content_identity_upgrade_control` 和尝试级安全配置/路由快照列；控制记录以 pending 开始。
- 不复制结果、不关联既有工作、不重写工作/投影，也不切换当前 model-specific 运行行为。
- 后续身份切换版本不得增加新迁移；它按可暂停、幂等且有阶段进度的流程归并工作与投影，然后处理符合条件的 `blocked_config` 恢复。
- 数据回填按完整身份执行。多个模型专属有效投影按 `published_at` 最新者胜出，同一时间以稳定 ID 决胜；匹配当前源哈希的有效结果不重跑。

迁移记录与运行边界：

- Durable state：应用运行时 SQLite 数据库及全局工作、请求、尝试、调用和投影事实。
- Compatibility range：迁移前的现有 schema 可升级到迁移 0084；部署后只有包含 0084 的兼容版本及其后续版本支持打开数据库。
- DDL：新增模型无关身份注册、旧工作成员映射、当前结果投影、升级进度控制，以及可空尝试快照列。兼容版本只运行现有 model-specific 行为。
- DML/backfill：迁移 0084 不转换历史行；后续版本分阶段、可暂停、幂等地回填，并分别报告工作映射、投影选择和阻塞恢复的进度。
- Recovery：失败后可回退到迁移 0084 兼容版本；不 down-migrate，缺少该迁移的旧二进制必须拒绝部署。
- Validation：以当前 schema 数据库夹具验证工作、投影、请求、尝试和调用关联不变，新结构为空且控制状态为 pending；重复启动兼容版本成功，迁移前二进制因未知迁移被拒绝。

## Migration-Free Identity Cutover Runtime

身份切换版本只使用已部署的迁移 0084，不增加 DDL。全局调度 worker 在身份控制记录完成前不 claim 工作；新 admission 可以继续进入队列，但必须在同一 writer 事务内注册模型无关身份、关联该身份下所有模型专属工作，并把可用的匹配结果写入当前投影。

admission 与手动重试在 writer 事务中读取持久化模型路由后再确定是否排队；worker claim 使用同一事务内读取的路由写入 `attempt_started`。因此路由更新与工作创建、重试或尝试开始之间都有单一 SQLite 写入顺序，配置更新后的恢复扫描不会漏掉排在更新之后提交的 `blocked_config` 工作。每轮阻塞恢复批次也在 writer 事务内重新读取持久路由并验证配置；若配置已无效，则保留阻塞状态且不追加排队事件。恢复扫描排除已有匹配投影或 queued、running、deferred_provider 工作的 identity；无可处理项或批次未产生进展时会终止，不重复占用 SQLite writer。

升级 worker 以小事务分阶段执行：

1. `work_identity_backfill` 为尚未映射的历史工作建立规范身份与成员关系。每批重新选择未映射工作，因此暂停后或回填期间到达且排序早于上次处理 ID 的行不会被游标跳过。
2. `projection_backfill` 为每个身份选择最近发布的有效模型专属投影；来源时间相同按投影 ID 稳定决胜。存在有效投影时，保留一个 ready 工作，其余同身份的活动或可恢复工作标记为 `superseded`，不改写既有尝试与调用记录。
3. `blocked_config_recovery` 只为没有匹配当前投影、且没有其他 queued、running 或 deferred 工作的身份排入自动恢复。批次查询排除已由其他活动工作覆盖的身份，因此整批都不可恢复时仍会终止而不重复扫描。配置无效时不调用 provider；有效配置更新或启动时运行时重载会重新验证并触发恢复扫描。

控制记录保存阶段、处理量、阶段总量、最近 ID、恢复数、错误码和完成时间。管理员可读取状态并在批次边界暂停、恢复；失败后可从已提交游标前向继续。`GET /api/admin/jobs/content-processing/identity-upgrade` 返回进度，`POST` 接收 `{"action":"pause"}` 或 `{"action":"resume"}`。升级完成还会请求既有搜索索引 content-projection phase 重算，以统一当前投影读取。

每次新尝试在 `attempt_started` 中写安全配置快照、按顺序排列的模型路由快照及配置指纹。worker 在领取工作项的同一 SQLite 写事务中读取持久化模型路由并写入开始事件；这让跨实例配置更新与 claim 按数据库写锁线性排序，避免 runtime 尚未 heartbeat 的实例用旧路由开始新尝试。快照不保存 URL 原文、用户凭据或密钥；只保留 URL origin、完整 URL 的 SHA-256、API key 的 SHA-256 和模型路由。调用期间使用该次记录的路由顺序，后续尝试重新读取当时的有效配置。

Feed 翻译与润色 hook 将 `blocked_config` 保持为原请求的 pending 状态，状态轮询间隔为 30 秒，网络错误退避最高 5 分钟，不受普通 pending 的最大等待年龄限制。已发布的匹配结果不会因模型变化重跑；卡片保留服务端返回的可读结果并显示既定等待文案，不提供手动重试入口。

运行时配置更新只有在模型路由实际变化时唤醒阻塞项；启动时会先完成运行时设置和路由恢复，再执行一次恢复扫描。尝试配置不再读取工作项创建时的模型档案或配置指纹。

## Admin Collection Read Budget

Release、公告、通知和日报的管理列表都先在 SQLite 中构造规范来源、旧事实／全局处理状态和筛选候选集，再精确计算总数并只读取当前页 ID。通知按 `updated_at DESC, id DESC` 选取每个 `thread_id` 的唯一来源；不存在可靠的首次发现时间时仍返回 `NULL`。

列表请求把缺省或单边时间条件归一化为不超过 31 天的 UTC 窗口，先物化窗口内来源候选，再限制处理状态聚合到候选 ID；当前页通过窗口总数取得精确匹配总数，越界空页才回退到计数查询。相同规范化查询键的在途读取由 keyed singleflight 合并，不缓存完成结果；panic 转换为完整内部错误并释放查询键，允许后续同键请求重新执行。五秒监督超时返回 `admin_collection_records_timeout`、HTTP 503 和 `Retry-After: 1`，不返回部分数据。

生产形状合成 fixture 的 `24h`、`7d`、`30d` 列表预算验证覆盖 Release、公告、通知和日报，并以 global processing mode 执行。testbox 上各类窗口的最慢 p95/p99 分别为 Release `157/157ms`、公告 `102/102ms`、通知 `182/182ms`、日报 `178/178ms`，均低于 1s/2s 门槛。fixture 包含每类 100,000 条源行，大多数位于窗口之外；EXPLAIN 断言来源时间/canonical 索引，并确认 global work 状态从有界来源 ID 经复合索引点查。

管理端客户端在切换种类、筛选或页码时取消失效请求，不自动重试；上述 503 显示既有页面内的人工刷新提示。线上形状副本验证了两项索引被选用，四类 31 天读取的三十次预热后测量均满足 p95 1 秒、p99 2 秒和单次 5 秒预算。

列表 keyed singleflight 使用规范化查询条件的稳定语义键：默认滚动窗口保留 `(from=None, before=None)` 标记，显式起点到当前和固定窗口分别保留可复现的边界；实际读取仍使用本次请求计算出的 31 天窗口。完成、超时或 panic 后先释放键再通知等待者，不缓存已完成结果。

## Admin Collection Activity

Release、公告、通知和日报活动读取在服务端按固定 UTC 十二小时半开窗先构造规范来源候选，再将 global、legacy、coverage 或 brief LLM 状态限制到这些候选。公告按 discussion 选择全历史最新 `occurred_at` 行，同时间以 rowid 稳定去重；窗口候选通过 canonical 索引验证后才水合源字段。通知使用 `updated_at DESC, id DESC`；摘要计数由响应中的完整 cells 计算。活动 GET 使用独立的 keyed singleflight 和五秒监督器，与列表读取互不阻塞。

迁移 `0085` 在 Release、公告、通知、日报来源时间及日报最新 LLM call 上新增索引。100,000 条/类的无内容合成数据库副本上，EXPLAIN 确认了四类来源时间索引、公告 canonical 索引、通知 canonical 索引和日报最新 call 索引；global activity/list CTE 从窗口内来源 ID 经 `idx_content_work_items_resource` 点查；每类预热后测 30 次。活动窗口返回数分别为 5,000、2,500、2,500、5,000；p95 为 `203/116/119/131ms`，p99 为 `205/116/121/143ms`，最大值为 `205/116/121/143ms`。列表 `24h/7d/30d` 各覆盖四种记录；各类窗口最慢 p95/p99 分别为 Release `157/157ms`、公告 `102/102ms`、通知 `182/182ms`、日报 `178/178ms`。两组预算均低于 p95 1s、p99 2s 和单次 5s 门槛；完整执行逻辑由忽略的 `admin_collection_activity_production_shape_budget` 测试承载。

管理端只请求当前 tab 的活动接口；tab 切换等待当前列表读取结束，随后按 kind 使用五秒内存缓存。筛选和翻页只更新列表；手动刷新或活动读取失败后的重试才会重新读取图表。DOM 与 Canvas 的活动格均为 24 CSS px；Canvas 仅命中格子边界内的指针输入，上下方向键按每小时独立视觉行移动并在小时边界保持列位置。超过 8,000 cells 时切换到固定视口 Canvas、滚动虚拟绘制和可访问 active gridcell；数据本身不截断。

## Rollback and Data Safety

SQLx 默认会校验数据库中每一个已应用迁移是否存在于当前二进制。因而，一旦扩展迁移已应用，迁移前的旧应用会因未知迁移版本而无法启动；这不是数据库损坏，而是运行时拒绝在未知模式下打开数据库。部署系统必须显式禁止此类回滚。

兼容版本包含扩展迁移，并在看到 `global` 或 `rollback_freeze` 时禁止旧内容处理写入。因此从全局切换版本回退到兼容版本时：数据库可打开；旧表、全局表和所有历史行仍在；内容处理保持受控暂停，不会把旧用户隔离模型重新写活；服务以兼容版本的一般功能运行，直到以前向修复恢复全局处理。不得把兼容版本之后产生的全局数据解释为旧模型的当前状态。

模型无关身份升级有自己的回滚下界：应用迁移 `0084` 后，第一阶段兼容版本是支持的最低二进制；缺少 `0084` 的更早版本不支持启动。后续切换版本不增加新迁移，但仍保留对已应用迁移 `0084` 的识别；失败时可回退到兼容版本而不反向修改数据库。

## Implementation Boundaries

- Scheduler runtime owns admission/retry transactions, claiming, batching, provider invocation, attempt events, result publication and automatic recovery; provider/attempt/projection writes stay behind this boundary.
- API adapters own authorization, requester association and response shaping; they delegate admission/retry to the scheduler boundary and never directly create terminal output.
- Source ingestion owns coverage submission; admin list and detail GETs only read.
- Admin read model joins the global work, result and legacy observation independently, rather than inferring any one from another.
- Web clients treat active-retry `409` and transition `503` as status synchronization outcomes, then poll the supplied link. They do not optimistically invent a local attempt.

## Failure Finalization

Provider and output-validation failures finalize the failed call link, work item, batch item, batch, and `attempt_completed` event in the same SQLite transaction. The regression test `content_processing::tests::execute_persists_failure_finalization_atomically` drives this path with a local mock provider and verifies that the terminal state and audit records persist together while the worker lease is cleared.

## Content Work Admission and Supersession

Migration `0086_content_work_admission_events.sql` adds append-only work
admission facts and per-attempt provider-admission facts without rewriting
existing work, attempt, call or result history. Global source adapters place an
authoritative source revision timestamp and tie-break value in the frozen
source snapshot; revision metadata is excluded from `source_hash`.

The scheduler serializes source admission, claim and provider admission through
the SQLite writer. Repeated same-source submission is an idempotent no-op;
older source revisions are retained as `superseded` and their requests point to
the current work. A superseded work item cannot be reopened by admission,
automatic recovery, claim or manual retry. Before every primary or bounded
length-recovery provider request, the worker validates the live lease, source
existence and source currentness, then records a provider-admission fact. A
source replacement after that fact may finish the provider call, but the call
audit remains linked while the attempt is finalized as `superseded` without
publishing output or scheduling retry.

Startup and recovery reconciliation close queued, failed, deferred-provider,
blocked-config and ready stale work without provider calls; running work with a
live lease is left to the worker guard. Admission events are retained for seven
days by the existing LLM diagnostic cleanup path, while attempt/provider-call
audit retention remains unchanged.

## Global Output Contract

The content-processing worker prompts for declared target fields at the JSON top level. It accepts only one optional Markdown JSON fence or one `output` object envelope before applying the existing field and Markdown validation; unknown, nested, ambiguous, duplicate-key or incomplete wrappers remain `output_contract_invalid`. A first `finish_reason=length` response receives one same-attempt recovery call with the same route and configuration snapshot and a 6,000-token limit. Before each provider call, the worker verifies and extends the current attempt lease in a SQLite writer transaction, so the primary and recovery calls each have a full execution window; an expired or superseded claim does not issue another model request. A second truncation records `output_truncated` without a third call. Every primary and recovery call is linked to the attempt, including provider-success call links for business-contract failures. The focused tests cover prompt shape, normalization, wrapped-output publication, lease renewal, bounded recovery, truncation classification and call-link persistence.

## Completion Evidence

Identity-upgrade implementation is complete only after the verification scenarios in [SPEC.md](./SPEC.md) pass against the migration-bearing compatibility build and the later migration-free identity cutover build. The release checklist must prove the database can start under the compatibility build after cutover, that migration-preceding binaries are rejected, and that compatibility migration 0084 performs no historical-data backfill.
