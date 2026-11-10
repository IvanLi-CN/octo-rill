# ADR 0023: PostgreSQL 18 迁移边界与共享契约

## 状态

Accepted

## 背景

当前 HEAD 的持久化层是 SQLx SQLite。`migrations/` 共有 86 张业务表、56 个触发器，并包含 FTS5 虚拟表；应用代码仍使用 SQLite 占位符、`BEGIN IMMEDIATE`、`INSERT OR IGNORE`、`datetime`/`julianday`、`json_extract` 和 SQLite 专属写协调器。`src/session_store.rs` 通过 MessagePack 将 `tower_sessions::Record` 写入 `tower_sessions`，并在并发更新时合并字段。搜索投影由 `search_projection_outbox`、租约和用户 lane 组成，查询先做用户可见仓库过滤再匹配 FTS。

本次迁移目标是独立 PostgreSQL 18 单实例，开发、测试、生产使用同一 PostgreSQL 方言和新 schema 基线。允许短暂停机和扩展；不要求搜索结果保留 SQLite 排名。切换前可以撤销，开放 PostgreSQL 写入后不能直接切回旧 SQLite，因为新增写入无法回流。

## 决策

1. **迁移历史与 schema**：新增独立 `migrations_pg/` 和 PostgreSQL SQLx migrator；不复制或改写 SQLite `_sqlx_migrations`。新库从空 schema 初始化，重复启动必须幂等。SQLite 迁移文件和历史校验逻辑保留到兼容窗口结束。
2. **权威与派生数据**：用户、令牌、密钥材料、仓库关联、发布、通知、任务、租约、Session 和迁移控制记录是权威数据；搜索文档、FTS/trigram 索引、outbox 消费游标和 backfill 队列是可重建投影。租约字段属于协调状态，必须持久化但可在过期后安全回收。派生投影不可成为导入成功或权限判断的依据。
3. **类型映射**：迁移首版保留 `TEXT` ID、密文和加密密钥字节；Rust `String`/`Vec<u8>` 分别映射 PostgreSQL `text`/`bytea`。时间列先按现有 SQLx 解码契约分批迁移：`chrono::DateTime<Utc>` 使用 `timestamptz`，`time::OffsetDateTime` 也使用带时区时间；无法确认时区语义的历史文本暂留 `text` 并增加规范化列和校验。JSON 文本先保留 `text`，只有调用方切换到 SQLx `Json<T>` 并完成 null/排序兼容验证后才改 `jsonb`。整数、布尔和枚举约束按现有 Rust 解码与 CHECK 语义映射，不能以“更现代”为理由全表换型。
4. **Session**：使用 `tower-sessions-sqlx-store` 的 PostgreSQL store，专用 schema/table 由 PostgreSQL migration 创建并归该组件所有；Session `data` 保持 MessagePack `bytea`，原始 ID 和 expiry 语义不变。旧 `tower_sessions` 在只读快照中逐行导入，先以 decode、账户状态、过期时间和计数校验，再允许新写入；解码失败进入隔离报告，禁止静默丢弃。并发合并继续采用记录基线与字段级 merge，数据库冲突按完整事务重试。
5. **事务与容量**：前台 API 和后台 worker 使用同一 PostgreSQL pool，但配置独立的最大连接数、超时、队列等待和后台并发信号量；后台不得耗尽前台保留容量。认领事务采用稳定排序的 `SELECT ... FOR UPDATE SKIP LOCKED` 加条件 `UPDATE`/`RETURNING`，提交后才执行外部工作；租约 owner、expiry 和 attempt 在同一事务中更新。遇到 `40001`（serialization failure）或 `40P01`（deadlock detected）重试完整事务，指数退避并设上限，禁止只重试单条 SQL；唯一键冲突仅在契约明确为竞争时重试。
6. **幂等与回退**：导入、任务创建和 outbox 使用显式 request/idempotency key 及唯一约束；重复请求返回已存在权威记录。失败重跑必须绑定目标 schema、manifest 和批次，不覆盖生产数据。切换前保留 SQLite 只读快照和撤销点；PostgreSQL 开放写入后若需回退，必须走“停写、反向导出/恢复、重新校验”的独立恢复方案，不能承诺直接启动旧 SQLite。
7. **搜索实验门禁**：先实现原生 `tsvector`（明确配置）+ `pg_trgm` 的双路候选查询，结果统一经过用户权限谓词；中文、英文、仓库名和代码片段各有固定样例，记录召回、误召回、延迟、索引大小和跨用户泄漏测试。`pg_trgm` 可由拥有 `CREATE` 的非超级用户安装，且支持相似度和 GIN/GiST 索引；全文配置必须显式指定 parser/dictionary。实验未通过前不得声称满足中文或代码搜索；失败时以实验报告触发 PGroonga 等扩展的单独 ADR，扩展选择必须有可安装性、版本、备份恢复和性能证据。旧排名不作为兼容目标，API 只保证稳定的时间/id tie-break 和权限隔离。
8. **时间与 API**：数据库排序统一使用显式 UTC 语义和稳定 tie-break（时间 DESC、权威 ID DESC）；API 序列化保留现有 RFC3339/字符串外观，迁移期间不把数据库精度变化泄露为协议变化。所有新时间列和旧文本转换都必须有边界样例（null、offset、秒/毫秒精度）。
9. **导入与验收**：从一致性只读 SQLite 快照流式 COPY/分批导入，manifest 记录源表、批次、行数、主键范围、SHA-256 和异常分类。导入阶段只写权威表；序列修复、外键/唯一检查、Session decode、权限抽样和派生索引重建分阶段执行。每阶段可暂停、可重跑、可审计。

## 后果

后续 schema、runtime、session、search、import 任务必须遵守 [共享契约](../database-migration/contracts.md)。迁移初期同时维护 SQLite 与 PostgreSQL 的差异成本较高，但能保留 SQLx 解码、密文和协议兼容性。搜索质量以实测门禁决定，不能用默认英文配置替代中文验收。

## 依据

- [PostgreSQL 18 `pg_trgm`](https://www.postgresql.org/docs/18/pgtrgm.html)
- [PostgreSQL 18 文本搜索配置](https://www.postgresql.org/docs/18/textsearch-configuration.html)
- [PostgreSQL 18 SELECT 与 `SKIP LOCKED`](https://www.postgresql.org/docs/18/sql-select.html)
- [PostgreSQL 18 显式锁与死锁](https://www.postgresql.org/docs/18/explicit-locking.html)
- [PostgreSQL 18 序列化失败处理](https://www.postgresql.org/docs/18/mvcc-serialization-failure-handling.html)
- [SQLx PostgreSQL 类型映射（0.8.6）](https://docs.rs/sqlx/0.8.6/sqlx/postgres/types/index.html)

## 拒绝的方案

- 复制 SQLite `_sqlx_migrations` 到 PostgreSQL：会混淆两个 migrator 的 checksum 和历史所有权。
- 把所有 `TEXT` 时间和 JSON 一次性改为 `timestamptz/jsonb`：会破坏 SQLx 解码、历史 null/格式和 API 兼容性。
- 只靠 PostgreSQL FTS 默认配置承诺中文搜索：缺少中文分词证据，无法通过实验门禁。
- 用应用层锁替代数据库租约：多 worker 和进程崩溃时无法保证唯一认领。
