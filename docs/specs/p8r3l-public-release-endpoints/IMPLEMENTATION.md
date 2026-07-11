# 实现状态

- Summary: fast-track / typed tag/ID selector、后端推荐窗口、双向虚拟列表、润色默认阅读路径、公开列表页头响应式布局与会话/PAT 条件化表情反应已交付。

## Milestones

- [x] M1: 新增公开 usage schema 与同步聚合入口。
- [x] M2: 新增公开列表/详情 API 与 pending retry 语义。
- [x] M3: 新增公开列表/详情页面与管理后台登记列表。
- [x] M4: 完成自动化验证与视觉证据。
- [x] M5: 完成 review-loop。
- [x] M6: 完成 PR 收敛。
- [x] M7: 公开页面页脚版本号链接与移动端视觉证据完成。
- [x] M8: 完成 typed 高亮 URL、服务端分段/gap 窗口、双向虚拟列表、浮动导航与前后端回归覆盖。
- [x] M9: 公开列表页头字标、owner/avatar 仓库身份与页面级 lane 响应式布局完成；列表卡片去除重复身份和操作，并补齐桌面与移动端 mock-only 视觉证据。
- [x] M10: 公开列表表情反应仅在已登录且 PAT 有效时启用；匿名不请求反应凭据或数据，并补齐反应切换回归与 member mock-only 视觉证据。

## Current Notes

- 管理后台删除公开登记记录后，若该仓库不再被其他公开登记、登录用户 release 可见性或历史 brief membership 使用，会清理对应共享 `repo_releases`、release AI 缓存与 release sync state；仍被使用时保留缓存。
- 公开列表/详情首次登记后会优先按 `full_name_lower` 复用 24 小时内刷新过且带真实 `is_private` 字段的本地公开 metadata：`starred_repos` 与 `owned_repo_star_baselines` 都可作为匿名 public proof，但只信任 fresh 的 privacy metadata，不把 `manual_feed` 或 `user_repo_associations.last_seen_at` 当 freshness 证明。若最新可信 metadata 判定为 public，且该 `repo_id` 已有非草稿 `repo_releases`，公开 usage 会立即回填 `repo_id` 并标记 `ready`；若 metadata 已知但 Release 缓存为空，则回填 `repo_id`、保持 `pending`，并入队 interactive repo release 同步。过旧 metadata、unknown privacy 或最新 fresh private metadata 仍会继续走 metadata pending 与后台公开校验路径。
- 公开 Release usage 已区分 `github_public` 与 `private_owner_published`：GitHub public repo 可由匿名访问自动登记；viewer-owned personal private repo 只能由拥有者登录态通过 `/api/repos/{owner}/{repo}/public-release` 发布，取消发布会删除 private usage 并复用共享缓存清理判断。
- `/public/:owner/:repo/releases` 前端路由只对旧列表路径做 replace 跳转到 `/:owner/:repo/releases`，保留 `/public/:owner/:repo/releases/tag/:tag` public-only 详情语义。
- SQLite 主连接池默认使用 `OCTORILL_SQLITE_POOL_MAX_CONNECTIONS=8`，允许在 `1..32` 内配置；启动日志记录实际连接池大小，并在 repo release / translation worker 并发明显超过 pool budget 时输出 warning。高竞争后台 claim / attach 写路径需要使用 `BEGIN IMMEDIATE` 这类提前声明写意图的事务，避免 WAL 多连接下 `BEGIN` 读快照升级写锁时触发 `SQLITE_BUSY_SNAPSHOT`。
- 公开列表与公开详情默认展示润色；首屏携带原文作为缺失/失败回退，翻译按视口可见记录批量加载。
- 公开 Release 页脚与全站 footer 保持一致：有效 `loadedVersion` 链接到 OctoRill 自身 public-only Release 详情页，`unknown` 保持纯文本。
- 公开文档站已提供面向接入方的 `公开 Release 接入` 页面，覆盖公开页面 URL、REST API、pending retry、真实 pending reason 枚举、分页参数与部署前检查。
- 高亮 URL 使用重复 typed `highlight` 或 typed `highlight_start` / `highlight_end`，并以 `highlight_active` 保存当前导航目标；离散目标上限 20，首屏完整数据预算 30。
- 高亮列表复用 `repo_releases` 唯一 ID 与仓库排序索引，响应增加连续 segments、内部 gaps、精确命中进度和双向 bounded cursor。
- 页面统一使用动态高度 window virtualization，内部 gap 接近视口后自动填充；右下浮动导航按页面时间顺序切换 active 目标并保持详情往返上下文。
- 公开列表页将页面级 lane 状态提升到 owner/avatar 加仓库名的标题带：宽度足够时同行，窄屏时 selector 整块换行；列表卡片不再重复仓库身份、lane 或 GitHub 操作，翻译 hydration 继续由页面级 lane 驱动。
- 公开列表复用现有 `AuthBootstrap` 和 reaction token 查询键：匿名会话不发起 PAT 状态查询；认证会话的 PAT 为 `configured + valid` 后，按最多 100 条一批刷新反应，且只有接口确认可操作的 Release 显示控件。列表窗口在刷新途中变化时仍合并已发出的有效响应，避免控件因 pagination、gap 或高亮加载竞态而永久隐藏；非 PAT 的失败批次独立重试最多三次，成功批次仍立即合并。`pat_required` / `pat_invalid` / `not_found` 会即时撤下相关控件，避免提供无法提交的操作。

## Verification

- `cargo test --all-targets -q` (`663 passed`)
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run build:demo`
- `cd web && bun run storybook:build`
- `cd web && PLAYWRIGHT_WEB_PORT=15300 bunx playwright test e2e/public-release-page.spec.ts --project=chromium` (`10 passed`)
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run build:demo`
- `cd web && bun x playwright test e2e/public-release-page.spec.ts --project=chromium` (`12 passed`)
- `cd web && PLAYWRIGHT_WEB_PORT=47130 bun x playwright test e2e/public-release-page.spec.ts --project=chromium` (`16 passed`，覆盖匿名无 PAT 请求、有效 PAT 的 100 条分批刷新与提交表情反应、瞬态刷新失败重试、无效 PAT 或无 feed 可见性时隐藏)
- Chrome mock-only `ui_demo`: `public-release-highlight-discrete` with `d_persona=member` at `1440x1000` and `390x844`; 有效 PAT 在每张可见 Release 卡片呈现 6 个表情反应，两个视口均无横向溢出。
- Chrome mock-only `ui_demo`: `public-release-highlight-discrete` at `1440x1000` and `390x844`; title band has the sole owner/avatar repository identity and lane selector, while cards have no repo identity, lane, or GitHub controls.
- `codex review --base origin/main`: 五轮审查已逐项修复 legacy redirect 重复参数、居中窗口顺序、详情翻译、范围绝对序号、hydration 字段覆盖与双向 cursor 边界问题。
- Chrome mock-only `ui_demo`: `public-release-highlight-discrete` at `1440x1000`; `public-release-highlight-range` at requested `390x844`.
