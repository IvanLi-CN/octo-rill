# PostgreSQL 18 迁移共享契约

本文是 schema、runtime、session、search、import 和 ops 任务的共同接口。权威数据、导入 manifest 和失败报告属于可审计事实；搜索索引与 outbox 消费状态属于可重建投影。

## 数据与类型

| 分类 | 事实 | PostgreSQL 首版约束 |
| --- | --- | --- |
| 权威 | users、tokens、API keys、passkeys、repo associations、releases、notifications、jobs、content work、runtime owners | 原始 ID 保持 `text`；外键、唯一键和 CHECK 与现有语义一致 |
| 权威机密 | PAT、OAuth refresh token、WebAuthn/加密密钥材料 | 原始密文和字节保持 `bytea`/等价二进制；不得明文化、轮换或重加密 |
| 协调状态 | lease owner/expiry、attempt、available/retry 时间、migration control | 事务内更新；过期才能回收；每次回收记录 owner、时间和原因 |
| Session | `tower_sessions.session` 的 id、MessagePack data、expiry | PostgreSQL store 专属 schema；`data bytea`；旧记录逐行 decode 后导入 |
| 派生 | search documents、`tsvector`/trigram index、outbox、backfill cursor | 可删除重建；不能替代权限过滤、导入成功或权威查询 |

Rust 映射以 SQLx 编译期解码为准：`String`→`text`，`Vec<u8>`→`bytea`，`i64`→`bigint`，`bool`→`boolean`，`chrono::DateTime<Utc>`→`timestamptz`，`NaiveDateTime`→`timestamp`。JSON 首版保留文本，使用 `jsonb` 前必须提交字段级解码和 null/排序证据。未知时区的旧文本不得直接 cast；转换失败进入 manifest 异常。

## 迁移历史与导入

- PostgreSQL 只读取 `migrations_pg/` 的独立 SQLx history；SQLite `_sqlx_migrations` 只读保留，不能复制、补行或重算 checksum。
- 迁移分为结构 DDL、当前状态修复、历史 backfill 三类，每类有独立版本、checksum、进度、暂停和失败状态；schema migration 不同步重建全量搜索。
- 导入前生成只读 SQLite 快照和 manifest。每批记录源表、主键范围/哈希、行数、目标 schema、开始/结束时间和异常类型（decode、constraint、foreign-key、duplicate、secret、unknown）。
- 重跑必须使用相同 manifest/request key 或新批次目标；唯一键保证幂等。任何覆盖生产、删除源快照或跳过异常都需要单独保护批准。
- 导入通过后按顺序执行 sequence 修复、外键/唯一检查、Session decode/账户状态检查、权限抽样，再重建派生投影；每阶段输出行数、SHA-256、缺失/额外 ID 和失败清单。

## 连接、事务与租约

前台连接池保留最小容量，后台连接池/信号量单独限额；连接获取、事务等待、查询和后台批次都有超时。后台 worker 必须在容量不足时退避，不得绕过 pool 或直接创建连接。

认领必须是一个短事务：按 `available_at, created_at, id` 稳定排序，`FOR UPDATE SKIP LOCKED` 锁定候选，检查 lease 是否为空或已过期，写入唯一 lease owner/expiry 和 attempt，并 `RETURNING` 认领行。外部调用在事务提交后执行，完成/失败以 owner 条件更新，防止旧 worker 覆盖新 owner。死锁/序列化冲突 SQLSTATE `40P01`/`40001` 重试整个事务（有限次数、指数退避、日志含 attempt 和 request key）；事务失败不得提前清除租约。

任务幂等键由业务请求和版本组成，数据库唯一约束是最终防线；重复认领返回已存在任务或明确冲突。所有时间比较使用 UTC 参数，不使用 SQLite `datetime()`/`julianday()` 字符串函数。

## Session 接口

Session store 暴露与现有 `tower_sessions::SessionStore` 相同的 create/load/update/delete/expired-delete 语义。`update` 先读取当前记录，按基线合并字段和 expiry，再在同一事务中条件写回；冲突重试完整 read-merge-write。旧 SQLite session 必须先 decode 成 `Record`，验证 id、expiry、账户存在/禁用状态和密钥版本，失败记录到导入报告并隔离。清理只删除过期记录，不能删除未过期或无法 decode 的记录。

## 搜索验收门禁

初始实现同时评估显式 text-search configuration + `tsvector` 与 `pg_trgm` 候选查询。固定夹具至少包含：中文标题/正文、英文标题、仓库 `owner/name`、代码标识符与带标点的片段；每条夹具标注 user、repo visibility 和预期可见性。验收记录：每类召回率、误召回、p95 延迟、索引大小、重建耗时、查询计划和跨用户泄漏测试。查询必须先应用用户可见仓库/用户 lane 谓词，再返回候选；排序只保证时间 DESC、权威 ID DESC 的稳定 tie-break，不保证 SQLite FTS5/bm25 等价。

门禁未通过时，search 任务必须提交扩展路线证据（例如 PGroonga 的安装权限、PG18/驱动兼容、备份恢复、索引重建和性能），由单独 ADR 决定；不得静默切换扩展或承诺中文质量。索引和 outbox 可删除重建，权威权限判断不能依赖它们。

## API、切换与回退

API 保持现有字段名、ID 外观和 RFC3339 序列化；数据库时间精度变化必须在契约测试中显式确认。切换前完成一致性快照、manifest 校验、Session/权限/任务/搜索隔离验收和撤销点。开放 PostgreSQL 写入后，旧 SQLite 只保留只读恢复材料；直接切回会丢失 PostgreSQL 新写入，回退必须停写并执行受控反向导出或备份恢复，重新完成校验后才开放服务。

## 文件责任

- `migrations_pg/`、`src/database_migrations.rs`：schema 任务；维护 PostgreSQL migrator 与独立 history。
- `src/runtime.rs`、`src/jobs.rs` 及业务 SQL：runtime 任务；实现连接容量、方言、认领、幂等、死锁重试。
- `src/session_store.rs`：session 任务；实现 PostgreSQL Session store 和旧数据转换。
- `src/search.rs`、`src/search_index.rs`：search 任务；实现双路搜索、outbox、权限过滤和重建。
- `tools/sqlite_to_postgres/`：import 任务；实现快照、manifest、COPY/批量导入、校验和失败重跑。
- `docs/database-migration/operations.md` 等：ops 任务；仅写通用操作模板，不写 101 地址、secrets、Compose 或生产数据。

任务提交前检查本契约与 ADR 0023；若实现需要改变权威/派生分类、类型映射、回退边界或搜索门禁，必须先更新 ADR 和本契约。
