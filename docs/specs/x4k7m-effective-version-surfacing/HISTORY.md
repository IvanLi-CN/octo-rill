# 演进记录（发布有效版本显示修复（API + Footer + Release 注入闭环））

## 生命周期

- Lifecycle: active
- Created: 2026-03-03
- Last: 2026-04-15

## 变更记录

- 2026-03-03: 建立规格并冻结修复范围与接口决策。
- 2026-03-03: 完成后端 `APP_EFFECTIVE_VERSION` 优先解析、`/api/version` 接口、footer 回退逻辑与 Docker/Release 注入闭环校验。
- 2026-03-03: 关联 PR #20，CI Pipeline 全绿。
- 2026-04-15: 修复 `web-builder` 对仓库根 `Cargo.toml` 的强依赖；前端构建改为 env 优先、Cargo 可选兜底、缺省回退 `"unknown"`，Docker 全仓库构建仍保留 Cargo fallback，并把 Docker smoke 并入既有 `Build (Release)` CI 门禁。
- 2026-04-15: 补齐历史 release backfill：当补跑旧 `release_head_sha` 时，`docker-release` 叠加当前 workflow revision 的 Docker/web 构建基础设施修复，避免旧源码再次触发已修复的 `Cargo.toml` 构建断链。
