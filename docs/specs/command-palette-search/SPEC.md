# 命令面板与个人工作区搜索

> 本文是命令面板与个人工作区搜索的长期行为合同；实现覆盖见 `./IMPLEMENTATION.md`，主题背景见 `./HISTORY.md`。

## Context and Scope

- Context: 已登录用户需要在阅读工作区中快速找到本地缓存内容并执行常用工作区操作，同时在桌面、窄屏和头像菜单回退之间保持同一入口与阅读语义。
- In scope: 本地缓存搜索文档、原文/翻译/润色合并结果、搜索筛选语法、可见性、用户级搜索配额、`GET /api/search`、响应式入口、命令面板、受控 actions 及结果深链。
- Out of scope: GitHub 全网搜索、GitHub 原生通知管理、完整 GitHub 客户端工作流、搜索历史持久化、无限结果分页、PAT 或 Reaction 写入、公开 Release 发布切换、管理员运行时配置以及任务调度或取消。

## Terms and Interfaces

- `搜索文档`: 一个规范可见内容对象及其可搜索字段、类型、仓库、来源时间、阅读目标和内容表达层。
- `有效搜索请求`: 已认证且通过封闭搜索语法校验的请求；有效但无结果的请求仍消耗配额，非法请求不消耗配额。
- `搜索窗口`: 由用户第一次有效搜索请求锚定的五分钟固定配额周期，最多允许 50 次请求；它不是严格滑动窗口。
- `命中 lane`: 搜索实际命中的 `original`、`translated` 或 `smart` 表达层；同一规范文档只返回一次并携带命中层集合或实际命中层。
- `命令动作`: 命令面板提供的权限受控工作区操作；动作展示、筛选和执行不消耗搜索配额。
- Interface: 认证态 `GET /api/search?q=<query>` 返回统一搜索结果、每项 canonical target、命中 lane 以及剩余配额；错误使用稳定错误码。

## Requirements

### REQ-CPS-001

- 系统 MUST 只从 OctoRill 本地缓存搜索 Release、公告/Discussion、日报、Inbox 通知和当前用户可见的仓库。
- 系统 MUST NOT 为满足搜索请求调用 GitHub Search API、写入仓库关联或持久化搜索词与查询历史。
- Inputs: 已认证用户与搜索查询。
- Outputs: 仅包含本地缓存中可授权读取的统一搜索结果。
- covers: 本地搜索边界与资源保护。

### REQ-CPS-002

- 系统 MUST 将同一内容对象的原文、翻译和润色表达合并为一个搜索文档结果，并返回实际命中的 `original`、`translated` 或 `smart` lane。
- 搜索文档 MUST 保留稳定的内容类型、仓库标识、来源时间、标题或可搜索正文、canonical 阅读目标及必要的匹配摘要；同一对象不得因多个 lane 重复出现。
- covers: canonical 结果与 lane 语义。

### REQ-CPS-003

- `GET /api/search` MUST 解析裸词、引号短语和以下过滤器：`owner:`、`repo:`、`type:`、`after:`、`before:`、`is:unread`。
- `repo:` MUST 接受仓库名和 `owner/name`；`type:` 只能选择已支持的搜索文档类型；日期过滤器 MUST 使用可比较的日期值；`is:unread` 只适用于通知语义。
- 未知、重复、格式错误或相互冲突的过滤器 MUST 返回 `invalid_search_query`；空查询或只含不可搜索语法的查询同样无效。非法请求不得消耗搜索配额。
- 合法查询 MUST 最多返回 20 条且不提供分页；至少三字符的查询使用 trigram 全文匹配，两字符查询使用相同授权谓词约束下的 `LIKE` 回退。
- covers: 查询语言、结果上限与短词回退。

### REQ-CPS-004

- Release 与公告结果 MUST 在查询时按当前用户可见仓库范围过滤；日报、通知和仓库结果 MUST 按当前用户身份及其用户仓库关联过滤。
- 系统 MUST 在未授权时完全省略对象、计数、标题、摘要、lane 和目标链接，不得通过结果总数或错误信息泄露存在性。
- covers: 逐类型可见性与越权防护。

### REQ-CPS-005

- 认证态 `GET /api/search?q=<query>` MUST 返回统一的结果数组、每项结果类型、标题/摘要、仓库与来源时间、canonical target、matched lane，以及当前用户的剩余搜索额度。
- 合法但无结果的查询 MUST 返回空结果和更新后的额度信息。
- 请求缺少认证或用户不可用时 MUST 遵循既有认证错误合同，不返回搜索结果或配额信息。
- 超过配额 MUST 返回 HTTP `429`、错误码 `search_rate_limited`、`retry_after_seconds` 和 `reset_at`。
- covers: HTTP 搜索接口与错误响应合同。

### REQ-CPS-006

- 系统 MUST 对每个用户使用“懒启动、以首个请求为锚点的固定窗口计数器”（`anchored fixed-window counter` / fixed-window counter with TTL）：第一次有效请求创建五分钟窗口，窗口内最多 50 次，第 50 次成功，第 51 次被拒绝；窗口结束后的下一次有效请求重新锚定窗口。
- 额度计数 MUST 在 SQLite writer 的 `BEGIN IMMEDIATE` 事务中与窗口状态原子更新，多标签页和服务重启不得绕过计数；拒绝请求不得增加计数。
- actions、本地 UI 打开/输入、非法查询和被拒绝的未认证请求 MUST 不计入搜索额度。
- covers: 用户级限流、并发一致性与计费边界。

### REQ-CPS-007

- 命令面板入口 MUST 只出现在已认证阅读工作区壳层；设置、暂停账户、匿名公开 Release 和管理壳层不得显示阅读工作区搜索输入。
- 桌面有足够空间时页头 MUST 显示可直接输入的搜索栏；中等窄屏 MUST 收缩为可操作的搜索按钮；空间不足时 MUST 将入口放入头像展开面板，且三种入口打开同一个命令面板。
- 全局 `Cmd/Ctrl+K` MUST 在阅读工作区打开命令面板，并在重复触发时保持单一对话框实例。
- covers: 页头响应式入口与壳层隔离。

### REQ-CPS-008

- 命令面板 MUST 使用既有 Dialog 语义，输入变更以 250ms 防抖触发搜索，取消过期请求，并在 IME composition 期间避免提交中间文本。
- 用户 MUST 能使用方向键选择结果、Enter 执行当前项、Esc 关闭面板并恢复打开前焦点；加载、空结果、错误、限流和结果状态均需可理解且不覆盖内容。
- 空输入 MUST 展示快速跳转和允许的 actions；以 `>` 开头 MUST 仅展示 actions，普通文本 MUST 展示搜索结果。
- covers: 命令面板交互、键盘可用性与请求生命周期。

### REQ-CPS-009

- 普通用户 actions MUST 至少包括阅读页/工作区 tab、Focus、设置、全量同步、同步 Inbox 和生成最近一个已结束本地自然日的日报。
- 日报生成 MUST 在提交任务前二次确认；全量同步和 Inbox 同步 MUST 复用现有进度反馈与去重语义。
- 关注/取消关注 MUST 只作为仓库搜索结果上的二级确认动作；不得把它作为无上下文的全局 action。管理员在阅读壳层最多只获得进入管理台的导航 action，不获得管理操作 action。
- covers: 受控 actions 与写入边界。

### REQ-CPS-010

- 搜索结果 MUST 跳转到原有阅读上下文：Release 与公告的详情 target 必须能携带并恢复 `original`、`translated` 或 `smart` lane；日报使用既有 brief deep link；通知继续使用其已有 GitHub target link；仓库使用现有仓库阅读/关注上下文。
- lane 路由状态只对登录态 Release/公告详情生效；无对应投影时必须沿用既有原文回退语义，不得制造虚假 lane。
- 返回工作区时 MUST 保留原 tab/scope 等既有上下文参数。
- covers: canonical deep link 与阅读语义恢复。

### REQ-CPS-011

- 搜索读路径 MUST 不复用会写仓库关联或使用记录的 feed scope；搜索结果的读取不得改变关注状态、同步状态、通知已读状态或内容处理状态。
- 系统 MUST 以统一结果顺序稳定合并多个 lane 和多个内容类型，并对相同目标去重；排序规则至少保持来源时间与稳定标识的确定性。
- covers: 只读保证、稳定排序与去重。

### REQ-CPS-012

- 桌面、图标收缩、头像菜单回退和 393x852 窄屏视图 MUST 保持入口可达、焦点可见、文字不裁剪、结果可滚动和动作无误触；浅色与暗色表面均须满足对比度与 Dialog 交互要求。
- 必须提供 mock-only Web Demo 与 Storybook 状态，覆盖空输入、搜索结果、actions、加载、错误、限流、桌面输入、中等图标和头像回退状态。
- covers: 响应式可用性与可复现视觉状态。

### REQ-CPS-013

- 数据库迁移 MUST 将历史 `0081_command_palette_search.sql` 视为兼容证据，不得在新部署中重新执行其全量回填；新的 `0082_command_palette_search_recovery.sql` 只创建搜索 schema、触发器和持久化索引状态。
- 服务 MUST 在 TCP listener 绑定后通过可中断的后台 worker 分阶段建立本地投影。每个 `Background` SQLite 事务最多处理 100 个源 rowid，并提交阶段游标，使重启后可以继续且重复执行保持幂等。
- worker MUST 在写入前检查数据库目录的 `statvfs` 可用空间；低于 `OCTORILL_SEARCH_INDEX_MIN_FREE_BYTES`（默认 20 GiB）时暂停并将搜索状态标记为 `paused_low_disk`，不得继续写入 FTS/WAL。索引状态 MUST 以 `building`、`ready` 或 `paused_low_disk` 出现在搜索响应中。
- 迁移运行器 MUST 只对白名单历史版本 `81` 接受精确 SHA-384 checksum；dirty、缺失、未知或 checksum 不匹配的迁移历史 MUST 终止启动。
- covers: 启动可用性、可恢复索引、资源保护与迁移兼容性。

## Verification

### VER-CPS-001

- Method: migration and projection fixture with existing Release, announcement, brief, notification and repository caches plus translation/smart projections.
- covers: `REQ-CPS-001`, `REQ-CPS-002`, `REQ-CPS-011`
- Pass condition: all supported local types are searchable, translated and smart expressions collapse to one canonical result, lane attribution is accurate, no query writes unrelated user associations or search history, and ordering/deduplication is deterministic.

### VER-CPS-002

- Method: parser and HTTP contract tests using bare terms, quoted phrases, every allowed filter, unknown/duplicate/conflicting filters, two-character and three-character queries, and over-cap result fixtures.
- covers: `REQ-CPS-003`, `REQ-CPS-005`
- Pass condition: valid queries return at most 20 canonical results with target and lane fields; invalid queries return `invalid_search_query` without quota changes; response and error fields match the interface contract.

### VER-CPS-003

- Method: per-content-type authorization fixture with cross-user repositories, releases, announcements, briefs, notifications and repository associations.
- covers: `REQ-CPS-004`
- Pass condition: unauthorized objects, counts, summaries, lanes and targets never appear for the requesting user.

### VER-CPS-004

- Method: concurrent SQLite writer fixture exercising the 50/300-second boundary, restart, multiple tabs, invalid requests, empty valid results and an expired window.
- covers: `REQ-CPS-006`
- Pass condition: the 50th valid request succeeds, the 51st returns `429 search_rate_limited` with reset metadata, rejected requests do not increment the counter, and the next request after expiry starts a new window atomically.

### VER-CPS-005

- Method: web component interaction tests and mock transport scenarios for keyboard, focus, IME, debounce, cancellation, actions and lane deep links.
- covers: `REQ-CPS-007`, `REQ-CPS-008`, `REQ-CPS-009`, `REQ-CPS-010`
- Pass condition: each responsive entry opens one Dialog, keyboard and focus behavior is correct, `>` scopes to actions, confirmation precedes brief generation, existing sync progress is reused, and lane/deep-link restoration returns to the original reading context.

### VER-CPS-006

- Method: mock-only Web Demo and Storybook canvas at desktop, compact icon, avatar fallback and 393x852 viewports in light and dark surfaces.
- covers: `REQ-CPS-012`
- Pass condition: all required states are reachable and visually stable without overlap, clipping, or loss of focus visibility.

### VER-CPS-007

- Method: migration compatibility, schema-only recovery migration, bounded worker and startup fixtures, plus Demo/Storybook status scenes.
- covers: `REQ-CPS-013`
- Pass condition: historical `0081` is accepted only with its exact checksum, fresh databases apply schema-only `0082`, listener startup does not wait for full indexing, worker resumes at persisted cursors in batches of at most 100 rows, low disk pauses without FTS writes, and `index_status` is visible in `building`/`paused_low_disk` demo states.

## Related ADRs

None

## Visual Evidence

- Desktop command palette with search results and quota status:
  `./assets/command-palette-desktop.png`
- Responsive 393x852 command palette state:
  `./assets/command-palette-mobile393.png`
- Both assets are mock-only Web Demo captures covering the search and command-panel surface; the owner confirmed the rendered states in Chrome.

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
- `../dashboard-header-brand-layout/SPEC.md`
- `../dashboard-tab-path-release-deep-link/SPEC.md`
- `../dashboard-readable-section-pagination/SPEC.md`
- `../announcement-discussion-reading/SPEC.md`
- `../brief-snapshot-timezone/SPEC.md`
