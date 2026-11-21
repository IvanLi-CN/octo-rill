# PostgreSQL 切换与回滚预案

目标：把 octo-rill 生产运行时从 SQLite 切到 PostgreSQL，窗口内可判定成功与回滚，不丢用户可见状态。

## 前提

- 运行时已固定 PostgreSQL（`DATABASE_URL=sqlite:` 会失败关闭），旧镜像仍可回滚。
- 生产 SQLite 仍是唯一写入方；切换前不动它，切换后保留只读与完整备份。
- 数据链路已验证：快照一致性、90 张表跨库等价、11 个 Seed 套件、生产副本读探测 75 通过。
- 定时调度器与后台 executors 已完成迁移：`src/postgres_scheduler.rs` 七个通道（入队/状态写入，durable 单飞），由 `OCTORILL_PG_SCHEDULER_ENABLED`（默认开启）控制。
- 生产副本写探测已通过：14 项可逆写全部还原，期间无任务进入 `running`，外部依赖写接口显式跳过。
- Seed 套件 15 套件全绿（含单仓刷新候选轮换 / 公共 fallback 5 项）。
- 生产仓库刷新依赖匿名 fallback 的量化：84 个 `github_public` 仓库中 11 个没有任何用户 token 可读，切换后只能走未认证请求。

## 时间预算（实测）

| 步骤 | 实测/预计 |
| --- | --- |
| `VACUUM INTO` 快照 | 119 s |
| 传输（局域网 50 MB/s） | 149 s（7.39 GB） |
| `preflight` + `import` | 461 s |
| 全量 `verify`（90 表，单连接） | 1322 s |
| 定向校验（行数 + 抽样表规范化校验） | 1–2 min |

结论：全量校验放在**预演**里做；切换窗口只做导入 + 定向校验，窗口预算约 **12–14 分钟冻结**。

## 步骤

### T-1 预演（不影响生产）

1. 生产只读快照 → 临时目录。
2. 导入到新建 PG 库（`octo_rill_stage`），跑 `import && verify` 全量。
3. 启动 PG 服务（`OCTORILL_PG_WORKER_ENABLED=0`）跑读探测与写探测（`pg_read_acceptance.py`、`pg_write_acceptance.py`）。
4. 预演通过才进入 T0；任何失败回退到代码修复，不进入切换。

### T0 冻结写入

1. 停止旧容器（记录 `docker compose stop` 时间与镜像 tag）。
2. 冻结时刻：`T0 = 容器停止时间`。
3. 保留 `data/octo-rill.db`、`-wal`、`-shm` 原样，只读挂载备份。

### T1 快照

```bash
sqlite3 data/octo-rill.db "VACUUM INTO '/home/ivan/srv/media/shared_data/octo-rill-cutover/snapshot.db'"
sqlite3 /path/snapshot.db "PRAGMA quick_check;"   # 必须 ok
sha256sum /path/snapshot.db
```

### T2 导入新库

```bash
SQLITE_SOURCE="sqlite:///.../snapshot.db?mode=ro" \
POSTGRES_URL="postgres://.../octo_rill_prod" \
MIGRATION_MANIFEST=/path/manifest.json \
  octo-rill-migrate preflight && octo-rill-migrate import
```

定向校验（窗口内）：

- 每表行数与快照一致（`job_task_events` 允许目标 ≥ 源）。
- 抽样表规范化校验：`users`、`repo_releases`、`starred_repos`、`briefs`、`translation_requests`、`job_tasks`。
- `_sqlx_migrations` 不存在于目标（导入器按设计跳过）。

### T3 切换

1. 新库上以 `OCTORILL_PG_BOOTSTRAP_ONLY=1` 启动一次，确认运行时迁移 0010–0015 已应用。
2. 部署新镜像/配置：`DATABASE_URL=postgres://...`、`OCTORILL_PG_WORKER_ENABLED=0`、`OCTORILL_PG_SCHEDULER_ENABLED=1`；生产加密密钥与 OAuth 凭据沿用现有 secret。
3. 启动服务，等待 `/health` 与 `/version`。
4. 冒烟通过后再单独打开 worker（见 T3.5），避免带存量 14,616 条 `repo_release_work_items` 与七个 lane 同时冲 GitHub。

### T3.5 打开 worker

不存在存量任务洪峰：切换时 `repo_release_work_items` 全部为终态（生产快照实测 14,085 succeeded / 531 failed / 0 queued），PG 只在 webhook 到达时单飞去重入队，账户级刷新按 `sync_auto_fetch_interval_minutes` 分桶。worker 打开后按普通冒烟观察：

- `rate_limited` / `github_http_error` 维持平时水平；
- `public_repo_release_usage.last_sync_status='inaccessible'` 只出现在无 token 的 11 个仓库；
- `job_tasks` 不出现同一 (task_type, payload) 的重复 queued 行（单飞失效的直接信号）；
- `sync.releases` 单任务耗时不超过一个刷新周期（并发上限取自 `admin_runtime_settings.repo_release_worker_concurrency`，生产为 15）。

### T4 冒烟（5 分钟内）

- 匿名：`/api/feed`、`/api/dashboard/feed`、`/api/releases/{id}/detail` → 401。
- 管理员会话：`/api/me`、`/api/feed`、`/api/dashboard/feed`、`/api/admin/jobs/overview`、`/api/admin/jobs/llm/status`、`/api/search?q=...`。
- 写入：运行 `scripts/pg_write_acceptance.py`（13 项可逆变更，自动还原）。
- 调度：确认 `job_tasks` 出现新入队的 `sync.*` 任务且状态流转 `queued → running → succeeded`；日志中七个 lane 无 `scheduler.dispatch_failed`。

### T5 判定与回滚

成功判据（全部满足）：冒烟全过、`job_tasks` 无异常堆积、日志无 `database_error`、错误率与切换前基线相当。

回滚（任一不满足）：

1. 停止 PG 运行时服务。
2. `docker compose` 恢复到旧 SQLite 镜像与配置，重新挂载原 `data/`（含 WAL）。
3. 启动后执行同一套冒烟；SQLite 库在冻结后未被写入，因此回滚不丢 T0 之前的数据；T3–回滚之间的 PG 写入会丢失，需在公告中说明。

## 切换前置条件（未全部满足则不得进入 T0）

| # | 条件 | 当前状态 |
| --- | --- | --- |
| P1 | 真实凭据冒烟：GitHub 登录回调、加密密钥解密 `github_connections`、一次真实 LLM 调用、session cookie 跨库可用 | **未做**（生产副本缺少密钥，OAuth 7 个端点未运行） |
| P2 | 切换后 GitHub 请求速率：账户级刷新按 `repo_release_worker_concurrency` 并发，避免 9,040 仓账户串行超过 10 分钟周期 | 已实现，Seed 套件断言并发上界 |
| P3 | 回滚产物可用：冻结时刻对只读副本 `REINDEX`（issue #381）并复验校验和 | **未做**（需授权） |
| P4 | 回滚契约确认：T3 之后的 PG 写入在回滚时丢弃，公告已说明 | 待主人确认 |
| P5 | 冻结窗口与公告（12–14 分钟） | 待主人排期 |

## 已知限制

- **增量导入不支持删除/更新重放**：导入器只做补齐与校验，不传播源侧删除或更新（现均失败关闭并给出明确提示）。因此切换必须导入**新库**，不能对已导入库做增量重放；中断恢复仍可用同一目标库重跑。
- **会话**：`tower_sessions` 会导入，但切换后 cookie 兼容性取决于同一 session key；冒烟必须包含一次真实登录。
- **任务日志文件**：`job_tasks.log_file_path` 指向旧主机路径，管理端日志读取在切换后需重新生成。
- **外部任务**：切换后调度器会重新入队同步/日报；GitHub rate limit 与 AI 配额需按平时水平预估。
- **全局内容处理**：生产为 `mode='global'`，PG 读路径已迁移（投影身份与写入身份统一，`pg_content_parity.py` 30/30 `ready`），切换无需改模式；异步翻译使用持久化的 `source_snapshot_json` 块，不依赖旧库行。
- **索引一致性**：生产 SQLite 存在 2 行索引条目缺失（见 101 维护记录），切换不修复该问题；PG 侧以规范化校验为准。

## 演练结果（VM，2026-11-21）

演练只用快照副本与临时库，未触碰生产：

1. **全量重新导入可携带增量**：在快照副本上写入 1 个新用户、1 条新 release，修改该用户 `login`，删除 1 条 `starred_repos`；导入新库后：增量行存在、更新值生效、被删行不再出现（`starred_repos` 15430 vs 原 15431），导入 496 秒。
2. **对已导入库做增量重放不可行（现已失败关闭）**：
   - 源侧内容变化但行数相同 → 导入器报 `existing target content differs from source for <table>`，退出码 1。
   - 源侧删除导致行数不同 → 旧版会走逐行补齐路径并静默保留被删行；本轮修复后报 `target keeps N rows but source now has M; the source deleted rows, so import into a fresh database instead of reusing this one`。
3. **崩溃后重入仍可用**：目标侧缺行（模拟上次导入中断）时重跑 `import` 会补齐并成功（实测 1 → 2 行，退出码 0），且逐行路径的类型强制按目标列类型进行（修复了 Text 单元写入 bigint 列时报 `cannot convert ... int8` 的缺陷）。

结论：切换必须**导入新库**；中断恢复可以继续用同一个目标库重跑。
