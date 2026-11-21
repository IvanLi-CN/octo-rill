# PostgreSQL 迁移设计与验证发现

## 数据边界

生产 SQLite 快照通过 SQLite Online Backup API 生成，单次复制耗时 43.67 秒，`quick_check=ok`，大小 10985185280 bytes；传入本次 Agent VM 后 SHA256 完全一致，服务器临时文件已删除。源版本为 SQLx migration 105。生产服务配置与镜像未更改。

## 表结构策略

迁移优先保留既有 ID、外键、NULL、唯一性与业务状态值。现有 importer 将 INTEGER 映射 BIGINT、BLOB 映射 BYTEA、REAL 映射 DOUBLE PRECISION，并保留多数时间字段为 TEXT；原生 PG 任务、会话与租约表使用 TIMESTAMPTZ。必须逐列检查二者交汇处，不能统一改变类型后继续使用旧解码器。SQLite 弱类型列尤其需要真实数据验证。

发现已发布 baseline 0006–0009 使用未限定 schema 的表名，继承 0001 的 search_path；新库因此将业务表建入 octo_rill，而应用查询 public。新增 0010 向前修复，仅在 public 表缺失时移动既有 octo_rill 表，保留已有生产导入表。

当前 importer 仅枚举 table，不导入 view 或 trigger。必须显式迁移 user_following_repos、user_release_visible_repos、search_release_metadata 视图及搜索 outbox 触发语义；不能快照化后宣告支持持续写入。

## 导入性能与正确性

优先 COPY FROM STDIN，避免逐行网络往返。PostgreSQL 官方建议新表先 COPY 再建索引和外键，随后 ANALYZE；当前实现提前建立部分索引，后续可在保持完整性门禁的前提下分离建表、COPY、约束验证和索引创建。[官方批量装载说明](https://www.postgresql.org/docs/18/populate.html)、[COPY 合同](https://www.postgresql.org/docs/18/sql-copy.html)。

当前 verify 的源哈希与目标哈希分别对照各自 manifest，不能证明跨库数据等价；目标按 ctid 排序也不稳定。同数量目标表跳过复制且哈希留空，重入校验不足。导入锁在 baseline 后提前释放，无法覆盖整个迁移。现已补充规范化跨库行哈希集合校验、同数量目标重入比对，并将导入锁延长至整轮结束；Seed PG 已验证重复导入成功以及同数量篡改在 import/verify 均被拒绝。仍需在生产规模验证时间与内存上界，不能把小数据成功视为最终验收。

## 自动化证据

基线 importer 单元测试：9 通过。基线应用测试：1255 通过、1 失败、3 ignored；失败为 sync::tests::social_activity_snapshot_preserves_committed_events_when_later_chunk_fails 的 SQLite 锁竞争，需单独复测。

新增 scripts/pg_seed_worker_test.py 在独立 PG18 数据库初始化、Seed 用户/连接/queued 任务，创建实际 MessagePack 会话并启动服务；断言 health、已登录 me、匿名拒绝与禁用 worker 后任务/事件不变。schema 修复后该测试通过。每次运行自动删除其独立测试库。

scripts/pg_read_acceptance.py 是读接口验收起点，当前不能代表完整契约覆盖；占位接口、缺少 fixture、非 JSON 响应均不能被当作成功。生产副本验收仍未完成。

导入器现保留三个业务视图并将 instr 转成 strpos；合成 schema + Seed 用户/Star 的 91 表导入测试证明视图随底表删除实时变化。默认 daily_brief_hour_slots 需先清除后按源导入，否则空源被 24 个默认行替代；该场景已覆盖。

## 生产副本探测进展

基线 COPY 导入 91 张表约 25 分钟。规范化校验首次发现 daily_brief_hour_slots 同数量但配置内容不同，已仅在 VM 副本按源恢复，并重新执行校验。当前新版全量校验仍运行中。

生产副本管理员读探测：43 pass、3 fail、8 not_run；尚未覆盖全部原应用路径、非管理员隔离及细粒度响应语义。已修复 profile 错误配置源、搜索括号与个人仓库相关子查询超时。翻译管理请求/批次列表复用原有响应模型，Seed PG 含实际行验证通过。

运行时会话实际位于 octo_rill.tower_sessions，原 importer 只写 public.tower_sessions，导致登录会话未被使用。已补充离线导入到运行时 session 表，合成 Seed 验证通过。

VM 第一次 lease 到期停止导致全量校验中断；不可将该轮扫描记为成功，需恢复原 session 后重新验证。

验收工具已修正旧 server 的 /api 嵌套路由映射；扩大后的管理员探测为 50 pass、13 fail、20 not_run。此前 49/0 只对应较小覆盖范围，不构成全量通过。大量后台与写接口仍未迁移。
