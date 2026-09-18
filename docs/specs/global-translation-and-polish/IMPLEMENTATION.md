# 全局翻译与润色工作模型实施

## Current State

当前运行时仍以用户隔离的 `translation_work_items` 和 `ai_translations` 为事实来源。Release 明细翻译存在直接结果缓存路径，管理页却只读取工作项，因此会把“已有缓存、没有工作项”显示为“未开始”。Release 润色也仍按 `scope_user_id` 分组；它没有同一条缺工作项的直接写入路径，但同样不符合全局共享的目标。

## Delivery Shape

1. 在监听前由 SQLx 完成空库初始化，并对已有库只读校验已应用 history 的版本、checksum 和 dirty 状态。
2. 服务注册 `runtime_owner` 后，在线 operator 通过命名持久 lease bootstrap 创建 run/operation 控制表；DDL、DML、历史回填按独立 operation 顺序推进。
3. 有效内容提交始终走全局 admission。提交事务先把 `legacy` 或 `rollback_freeze` 前向修复为 `global`，再创建或关联唯一工作项；迁移不会返回专属 `503`，也不需要停机窗口。
4. 从旧表和缓存只读取并写入旧事实观察：`ai_translations.entity_type IN ('release_smart', 'announcement_smart')` 与相应 `translation_work_items.kind`／尝试事件必须和翻译记录一并处理。有可显示缓存而无对应工作证据的记为 `legacy_cached`；存在不可一致解释的工作与缓存证据记为 `legacy_conflict`。不复制或修改旧行，不创建全局工作项、结果投影或虚构尝试。
5. 历史回填按最多 100 行的 cursor 批次执行，在提交后释放 SQLite permit；前台写入等待时 migration priority 让行。pause/resume、owner heartbeat 和脱敏错误都持久化。
6. 故障只通过识别现状的 forward repair 收敛；不做 down-migration、蓝绿切换或拓扑改造。

## Database Migration Plan

当前实现使用迁移 `0078_content_processing_global_model.sql` 和追加迁移 `0079_admin_collection_read_budget_indexes.sql`，只创建下列新表、索引和控制记录：

- `content_processing_control`：单行历史控制记录，取值为 `legacy`、`rollback_freeze` 或 `global`；有效 admission 会在同一事务中将前两者修复为 `global`。
- `content_work_items`：包含全局身份、不可变来源快照、冻结配置指纹、优先级、调度状态、租约关联、恢复元数据和取消／替代关系。唯一索引覆盖 `REQ-GTP-IDENTITY` 的全部字段。
- `content_batches` 与 `content_batch_items`：持久化调度批次、工作成员、分区、令牌估算、触发原因和批次结果，唯一 worker kind 为 `general`。
- `content_result_projections`：按规范资源、处理链路、变体、语言、协议和模型档案保存最新已验证投影、已发布来源哈希、当前工作项和活动工作项。更新使用同一事务替换投影；刷新中的工作绝不清空旧投影。
- `content_request_links`：保存请求者或系统生产者、授权快照、请求来源、交付模式、关联工作项和响应事实；它是重试竞争时仍要写入的关联记录。
- `content_attempt_events` 和 `content_attempt_llm_calls`：追加式的全局尝试和精确模型调用归因，仅保存安全元数据。
- `content_legacy_observations`：引用旧表的原始主键和只读分类，保存 `legacy_cached` 或 `legacy_conflict` 的判定依据；不复制旧内容、不反向修改旧表。
- `idx_notifications_thread_id`：为按全局通知线程读取 canonical source 提供 `thread_id` 前导索引；授权仍使用用户行单独校验。
- `idx_notifications_admin_canonical_source`：按通知线程、更新时间和稳定行 ID 支持管理读取的规范来源选择。
- `idx_translation_work_items_admin_entity_kind_attempt`：按实体、处理种类和尝试次数支持管理候选集筛选。
- `online_migration_leases`、`online_migration_runs`、`online_migration_operations`：持久化命名 lease、不可变定义 checksum、DDL/DML/backfill 顺序、cursor、pause、owner heartbeat 和脱敏失败。

迁移不执行 `DROP TABLE`、表改名、数据重建、旧行 `UPDATE`、旧行 `DELETE` 或把旧数据插入全局工作／结果／尝试表。`translation_work_items`、`translation_requests`、旧尝试事件和 `ai_translations` 继续存在；应用仅把它们当作旧事实读取。

## Admin Collection Read Budget

Release、公告、通知和日报的管理列表都先在 SQLite 中构造规范来源、旧事实／全局处理状态和筛选候选集，再精确计算总数并只读取当前页 ID。通知按 `updated_at DESC, id DESC` 选取每个 `thread_id` 的唯一来源；不存在可靠的首次发现时间时仍返回 `NULL`。

列表请求把缺省或单边时间条件归一化为不超过 31 天的 UTC 窗口，完整读取（模式读取、候选查询、总数、当前页装载和摘要投影）共享一个容量为一的进程内闸门和五秒预算。闸门繁忙或读取超时分别返回 `admin_collection_records_busy`／`admin_collection_records_timeout`、HTTP 503 和 `Retry-After: 1`；超时会先取消并等待 SQL 任务清理，再释放许可。

管理端客户端在切换种类、筛选或页码时取消失效请求，不自动重试；上述 503 显示既有页面内的人工刷新提示。线上形状副本验证了两项索引被选用，四类 31 天读取的三十次预热后测量均满足 p95 1 秒、p99 2 秒和单次 5 秒预算。

## Rollback and Data Safety

SQLx 默认会校验数据库中每一个已应用迁移是否存在于当前二进制。因而，一旦扩展迁移已应用，迁移前的旧应用会因未知迁移版本而无法启动；这不是数据库损坏，而是运行时拒绝在未知模式下打开数据库。部署系统必须显式禁止此类回滚。

已部署状态不支持 down-migration。停止或发现缺陷的版本由后续 forward repair 识别 run/operation 现状并继续；旧表、全局表和历史行仍在，内容提交保持原有 `202`/`409` 合同。

## Implementation Boundaries

- Scheduler runtime owns admission/retry transactions, claiming, batching, provider invocation, attempt events, result publication and automatic recovery; provider/attempt/projection writes stay behind this boundary.
- API adapters own authorization, requester association and response shaping; they delegate admission/retry to the scheduler boundary and never directly create terminal output.
- Source ingestion owns coverage submission; admin list and detail GETs only read.
- Admin read model joins the global work, result and legacy observation independently, rather than inferring any one from another.
- Web clients treat active-retry `409` as the status synchronization outcome and poll the supplied link. There is no transition `503` contract.

## Completion Evidence

Implementation is complete only after the verification scenarios in [SPEC.md](./SPEC.md) pass against a fresh database and an existing database with validated SQLx history. The release checklist must prove pause/resume re-entry, forward repair, unchanged legacy rows and no migration-specific admission error.
