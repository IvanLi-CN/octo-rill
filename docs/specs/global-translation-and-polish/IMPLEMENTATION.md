# 全局翻译与润色工作模型实施

## Current State

当前运行时仍以用户隔离的 `translation_work_items` 和 `ai_translations` 为事实来源。Release 明细翻译存在直接结果缓存路径，管理页却只读取工作项，因此会把“已有缓存、没有工作项”显示为“未开始”。Release 润色也仍按 `scope_user_id` 分组；它没有同一条缺工作项的直接写入路径，但同样不符合全局共享的目标。

## Delivery Shape

1. 在兼容版本中加入全局扩展表、`content_processing_control` 控制记录和启动前模式保护。默认模式为 `legacy`；该版本仍运行旧模型，但已经包含并应用所有切换前需要的迁移。
2. 兼容版本必须在 `global` 或 `rollback_freeze` 模式下阻止旧入口、旧调度器和直接缓存写入，返回可轮询的维护响应。它仍可启动、读取一般业务数据和保留旧内容处理事实，因此是数据库迁移后的最低回滚版本。
3. 在受控窗口暂停内容处理接收，等待运行中的旧批次到达终态；超时的批次只按既有租约恢复规则收口，不迁移为全局尝试。写入 `rollback_freeze`，使所有新内容处理请求得到 `503` 和状态轮询信息。
4. 从旧表和缓存只读取并写入旧事实观察：`ai_translations.entity_type IN ('release_smart', 'announcement_smart')` 与相应 `translation_work_items.kind`／尝试事件必须和翻译记录一并处理。有可显示缓存而无对应工作证据的记为 `legacy_cached`；存在不可一致解释的工作与缓存证据记为 `legacy_conflict`。不复制或修改旧行，不创建全局工作项、结果投影或虚构尝试。
5. 部署切换版本。它不携带新的数据库迁移，只读取已存在的扩展表，并在一个数据库事务中把模式从 `rollback_freeze` 切换为 `global`；提交后由全局调度器读取该控制状态并接管新的覆盖请求和交互入口。
6. 在稳定期只通过全局读模型展示状态，并持续监测未预期的旧写入。发现问题时暂停全局调度器并回退到兼容版本；兼容版本保持全局模式保护，避免恢复旧写入，随后以前向修复恢复服务。

## Database Migration Plan

当前分支最高迁移号为 `0077`。下一条迁移在实际落地时必须使用当时的下一个单调编号，并且只创建下列新表、索引和控制记录：

- `content_processing_control`：单行模式栅栏，取值为 `legacy`、`rollback_freeze` 或 `global`；记录切换代号和更新时间。它是旧新写入者共同读取的唯一切换事实。
- `content_work_items`：包含全局身份、不可变来源快照、冻结配置指纹、优先级、调度状态、租约关联、恢复元数据和取消／替代关系。唯一索引覆盖 `REQ-GTP-IDENTITY` 的全部字段。
- `content_batches` 与 `content_batch_items`：持久化调度批次、工作成员、分区、令牌估算、触发原因和批次结果，唯一 worker kind 为 `general`。
- `content_result_projections`：按规范资源、处理链路、变体、语言、协议和模型档案保存最新已验证投影、已发布来源哈希、当前工作项和活动工作项。更新使用同一事务替换投影；刷新中的工作绝不清空旧投影。
- `content_request_links`：保存请求者或系统生产者、授权快照、请求来源、交付模式、关联工作项和响应事实；它是重试竞争时仍要写入的关联记录。
- `content_attempt_events` 和 `content_attempt_llm_calls`：追加式的全局尝试和精确模型调用归因，仅保存安全元数据。
- `content_legacy_observations`：引用旧表的原始主键和只读分类，保存 `legacy_cached` 或 `legacy_conflict` 的判定依据；不复制旧内容、不反向修改旧表。

迁移不执行 `DROP TABLE`、表改名、数据重建、旧行 `UPDATE`、旧行 `DELETE` 或把旧数据插入全局工作／结果／尝试表。`translation_work_items`、`translation_requests`、旧尝试事件和 `ai_translations` 继续存在；应用仅把它们当作旧事实读取。

## Rollback and Data Safety

SQLx 默认会校验数据库中每一个已应用迁移是否存在于当前二进制。因而，一旦扩展迁移已应用，迁移前的旧应用会因未知迁移版本而无法启动；这不是数据库损坏，而是运行时拒绝在未知模式下打开数据库。部署系统必须显式禁止此类回滚。

兼容版本包含扩展迁移，并在看到 `global` 或 `rollback_freeze` 时禁止旧内容处理写入。因此从全局切换版本回退到兼容版本时：数据库可打开；旧表、全局表和所有历史行仍在；内容处理保持受控暂停，不会把旧用户隔离模型重新写活；服务以兼容版本的一般功能运行，直到以前向修复恢复全局处理。不得把兼容版本之后产生的全局数据解释为旧模型的当前状态。

## Implementation Boundaries

- Scheduler runtime owns claiming, batching, provider invocation, attempt events, result publication and automatic recovery.
- API adapters own authorization, requester association and response shaping; they never directly create terminal output.
- Source ingestion owns coverage submission; admin list and detail GETs only read.
- Admin read model joins the global work, result and legacy observation independently, rather than inferring any one from another.
- Web clients treat active-retry `409` and transition `503` as status synchronization outcomes, then poll the supplied link. They do not optimistically invent a local attempt.

## Completion Evidence

Implementation is complete only after the verification scenarios in [SPEC.md](./SPEC.md) pass against a migration-bearing compatibility build and a migration-free global cutover build. The release checklist must prove the database can start under the compatibility build after cutover, that migration-preceding binaries are rejected before deployment, and that no legacy table changed during old-fact observation.
