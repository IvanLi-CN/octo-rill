# 实现状态

- Summary: 本地验证完成；fast-track / public release pages + REST API + admin registry

## Milestones

- [x] M1: 新增公开 usage schema 与同步聚合入口。
- [x] M2: 新增公开列表/详情 API 与 pending retry 语义。
- [x] M3: 新增公开列表/详情页面与管理后台登记列表。
- [x] M4: 完成自动化验证与视觉证据。
- [x] M5: 完成 review-loop。
- [ ] M6: 完成 PR 收敛。
- [x] M7: 公开页面页脚版本号链接与移动端视觉证据完成。

## Current Notes

- 管理后台删除公开登记记录后，若该仓库不再被其他公开登记、登录用户 release 可见性或历史 brief membership 使用，会清理对应共享 `repo_releases`、release AI 缓存与 release sync state；仍被使用时保留缓存。
- 公开列表/详情首次登记后会优先按 `full_name_lower` 复用 24 小时内刷新过且带真实 `is_private` 字段的本地公开 metadata：`starred_repos.is_private=0`。若该 `repo_id` 已有非草稿 `repo_releases`，公开 usage 会立即回填 `repo_id` 并标记 `ready`；若 metadata 已知但 Release 缓存为空，则回填 `repo_id`、保持 `pending`，并入队 interactive repo release 同步。过旧 metadata 或 owned-only baseline 不用于公开访问决策，会继续走 metadata pending 与后台公开校验路径。
- SQLite 主连接池默认使用 `OCTORILL_SQLITE_POOL_MAX_CONNECTIONS=8`，允许在 `1..32` 内配置；启动日志记录实际连接池大小，并在 repo release / translation worker 并发明显超过 pool budget 时输出 warning。高竞争后台 claim / attach 写路径需要使用 `BEGIN IMMEDIATE` 这类提前声明写意图的事务，避免 WAL 多连接下 `BEGIN` 读快照升级写锁时触发 `SQLITE_BUSY_SNAPSHOT`。
- 公开列表页默认展示原文正文，但列表态会截断超长正文，详情页仍展示完整正文。
- 公开 Release 页脚与全站 footer 保持一致：有效 `loadedVersion` 链接到 OctoRill 自身 public-only Release 详情页，`unknown` 保持纯文本。
- 公开文档站已提供面向接入方的 `公开 Release 接入` 页面，覆盖公开页面 URL、REST API、pending retry、真实 pending reason 枚举、分页参数与部署前检查。

## Verification

- `codex -m gpt-5.5 -c model_reasoning_effort="medium" --sandbox read-only -a never review --base origin/main`
- `cd web && bun run build`
- `cargo test public_release --all-targets`
- `cargo test --all-targets`
- `cd web && bun run storybook:build`
- `cd web && PLAYWRIGHT_WEB_PORT=36830 bun run e2e -- public-release-page.spec.ts`
- `cd web && npm run storybook:build`
- `cd web && npm run e2e -- public-release-page.spec.ts`
