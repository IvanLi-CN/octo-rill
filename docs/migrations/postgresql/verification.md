# PostgreSQL 迁移验证方法

本文件记录可复现的验证手段与判定规则；具体结果见 `completed.md`。

## 环境

- 租约 Agent VM（session 由宿主持有），Ubuntu 24.04，PostgreSQL 18.7（PGDG），8 vCPU。
- 服务与数据库都只在该 VM 内运行；不访问生产外部服务，不复制生产凭据。

## 1. 生产快照

```bash
# 生产机（只读连接，不动生产服务）
sqlite3 data/octo-rill.db "VACUUM INTO '/path/snapshot.db'"
sqlite3 /path/snapshot.db "PRAGMA quick_check;"   # 期望 ok
sha256sum /path/snapshot.db
```

判定：`quick_check=ok` 且两端 SHA256 一致；服务器临时文件随后删除。
`integrity_check` 的索引级发现单独记录在 101 的维护记录里，不影响表数据等价。

## 2. 导入与逐表等价

```bash
SQLITE_SOURCE="sqlite:///workspace/data/snapshot.db?mode=ro" \
POSTGRES_URL=postgres://.../octo_rill \
MIGRATION_MANIFEST=/workspace/data/manifest.json \
  octo-rill-migrate preflight && ... import && ... verify
```

判定要点：

- `preflight` 报告权威表数量与目标剩余空间。
- `import` 逐表输出行数与耗时；`_sqlx_migrations`、FTS5 投影表按设计跳过。
- `verify` 以规范化值在两侧独立重算校验和（时间按微秒舍入、布尔按 `0/1` 与 `t/f` 归一），不做物理页比较。
- 运行期会被服务写入的表（`runtime_owners`、`public_repo_release_usage` 的请求计数）单独判定：全列比对必须只出现计数/时间列差异。

## 3. 批内 Seed 套件

```bash
PG_ADMIN_URL=postgres://.../postgres \
PG_SEED_SCHEMA_SOURCE=/workspace/data/snapshot.db \
BINARY_DIR=target/debug PORT_BASE=3100 \
  bash scripts/pg_seed_suite.sh
```

每个套件自建一次性数据库、以 `OCTORILL_PG_BOOTSTRAP_ONLY=1` 建表、用 `OCTORILL_PG_WORKER_ENABLED=0` 启动服务，断言 HTTP 状态与响应，最后 `DROP DATABASE ... WITH (FORCE)`。`scripts/pg_seed_common.py` 统一 `psql` 调用并在失败时回显 SQL 错误，避免把 fixture 的 schema 偏差误报成产品缺陷。

## 4. 生产副本读探测

```bash
python3 scripts/pg_read_fixtures.py --postgres-url "$DATABASE_URL" \
  --user-id "$ADMIN" --base-url http://127.0.0.1:3500 --cookie-file admin.cookie \
  --output read-params.json
python3 scripts/pg_read_acceptance.py --base-url http://127.0.0.1:3500 \
  --cookie-file admin.cookie --parameters read-params.json --output read-acceptance.json
```

- 路由清单直接来自 `src/postgres_server.rs` 的 GET 注册，新增路由自动纳入。
- fixture 值来自真实数据（按会话用户可见范围选择），dashboard 分段 id 这类用户级游标通过 `/api/dashboard/feed` 获取。
- 分类规则：5xx 与带鉴权被拒 = 失败；401/403 且无会话 = 通过（匿名拒绝）；缺 fixture 或需交互登录 = 未运行。
- 已知环境限制在失败项中显式说明，不以“通过”掩盖。

## 4b. 全局内容处理一致性（mode='global'）

生产 `content_processing_control.mode='global'`，译文可能只存在于 `content_result_projections`，因此必须单独验证“投影里的译文能被接口读到”：

```bash
AI_BASE_URL=... AI_API_KEY=... AI_MODEL=... <service>   # legacy 在 AI 未配置时隐藏译文
python3 scripts/pg_content_parity.py --base-url ... --cookie-file ... \
  --postgres-url ... --sample 30 --output content-parity.json
```

判定：抽样 release 必须返回 `translated.status in {ready, stale}`；若全部为 `disabled` 说明服务未配置 AI，探测无效。

基线（未修复时）：`mode=global`、30/30 失败、`translated` 字段整体缺失 —— 对应约 5,239 个只有投影、没有 `ai_translations` 的 release。

修复后：30/30 全部 `ready`（含 tag 内含 `/` 的 URL 六段场景，写入身份为 `unknown/<repo_id>`）。

## 4c. 新库与导入库的列类型差异审计

两套 schema 来源不同（0001–0016 基线 vs SQLite 翻译），必须定期比对，避免再出现 `boolean`/`bigint` 这类只在一侧失败的解码错误：

```bash
# 新库：空库 + OCTORILL_PG_BOOTSTRAP_ONLY=1
# 导入库：生产副本
Q="select table_name||'.'||column_name||'|'||data_type from information_schema.columns where table_schema='public' order by 1"
join -t'|' <(psql <imported> -XAtc "$Q" | sort) <(psql <fresh> -XAtc "$Q" | sort) \
  | awk -F'|' '$2!=$3 {print}'
```

当前结论（2026-11-21）：差异只有两类等价类型（`text` vs `timestamptz`、`bigint` vs `integer`），读路径均已按 TEXT/整数语义归一（`normalize_timestamp`、`::text::boolean`、`::bigint`）；`users.daily_brief_schedule_local_time` 仅在基线存在但运行时不读（调度配置在 `admin_runtime_settings`），导入库多出的 `translation_*`/`content_work_items.evidence_*` 列运行时不写。

## 5. 运行时后端闸门

```bash
DATABASE_URL=sqlite:///tmp/any.db <service>
# 期望：application runtime requires a PostgreSQL DATABASE_URL
```

## 6. 单元/回归测试

```bash
cargo test --locked --bin octo-rill      # 1262 passed, 0 failed, 4 ignored
cargo fmt --check
cargo check --locked
```
