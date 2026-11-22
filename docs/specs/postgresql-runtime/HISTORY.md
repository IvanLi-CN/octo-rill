# PostgreSQL 运行时持久化 主题历史

> 这里记录主题局部生命周期、替换、兼容性与必要背景；完整 ADR 取舍保留在 `docs/adr/`。

## Lifecycle / Compatibility

- 主题建立于运行时持久化从 SQLite 切换到 PostgreSQL 18 的交付期。SQLite schema 与 PostgreSQL schema 之间没有自动转换路径：唯一受支持的迁移是将快照导入新建 PostgreSQL 库。
- `REQ-PGR-003` 的兼容边界是导入库与空库收敛到同一可启动 schema；已发布迁移不可修改，类型差异通过 forward migration 与读路径归一解决。

## Replacements / Background

- 取代 `docs/specs/persistent-state-migrations` 所描述的 SQLite 在线迁移运行面：PostgreSQL 运行时不读取 SQLite `_sqlx_migrations`，也不执行 SQLite 在线 DDL/DML 回填。
- ADR 0023 记录“PostgreSQL 运行时权威”的决策与取舍；ADR 0019 仍是 PostgreSQL 运行时 schema 迁移的启动顺序约束。

## Related Changes

- 账号级与单仓 Release 刷新恢复自动翻译：只为本次插入的 Release 排队 `translation.generate`，按工作项去重并在有在途任务时跳过，单次最多 25 个 Release（`REQ-PGR-009`）。旧 `content_processing` 流水线不随本变更恢复。
- issue #384：未认证 Release 读取纳入进程级预算（`REQ-PGR-008`）；预算耗尽记为延后，不再混入失败分类，验证入口为 `scripts/pg_seed_repo_release_anon_budget_test.py`。
- 表情计数持久化不再写 `repo_releases.updated_at`：该列是 Release 源修订号，内容处理用它判断在途 work item 是否过期；被推进后 GitHub 真实修订与 LLM provider admission 判定互相冲突。修复入口为 `scripts/pg_seed_reactions_test.py`，存量污染行用 `octo-rill-migrate repair-release-revisions` 修复。
- 交付记录：`仓库外运维记录,completed,verification,cutover-plan}.md` 与 `migration-record.json`。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
