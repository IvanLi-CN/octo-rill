# 公开仓库 Release 外链与 API（#p8r3l）

## 背景 / 问题陈述

第三方应用需要直接链接到 OctoRill 页面查看公开仓库更新记录，也需要通过 REST API 获取指定公开仓库的 Release 列表、详情、翻译与润色内容。

## 目标 / 非目标

### Goals

- 提供未登录可访问的公开 Release 列表页与详情页。
- 提供未登录 REST API：公开仓库 Release 列表与 tag 详情。
- 公开 Release 页面页脚展示当前 OctoRill 前端加载版本，并链接到 OctoRill 自身 public-only Release 详情页，登录态不得把该链接切到 Dashboard。
- 首次访问先登记仓库 usage；若本地已有近期刷新且明确公开的仓库 metadata 与非草稿 Release 缓存，则直接复用共享缓存返回 ready；若只有近期公开 metadata 但尚无 Release 缓存，则回填 `repo_id` 并返回可重试 pending 响应；若本地无法确认近期公开 metadata，则返回 metadata pending。
- 公开端点与登录用户视图复用同一份仓库级 `repo_releases` 主数据。
- 管理后台展示公开端点登记仓库、访问统计、同步状态、共享缓存数据量，并允许删除登记记录。

### Non-goals

- 不为首次访问做请求内 GitHub 拉取。
- 不开放私有仓库或用户私有 viewer 状态。
- 不复制 Release 主数据到 public-only 表。
- 删除公开登记记录时不清理仍被登录用户视图或其他公开登记使用的共享缓存。
- 不新增除 `zh-CN` 以外的翻译语言。

## 接口契约

- `GET /api/public/repos/{owner}/{repo}/releases`
  - Query: `content=original|translated|polished|all`, `lang=zh-CN`, optional `source=page`
  - `200`: 返回共享缓存列表。
  - `202`: 返回 `status=pending_sync`、`reason`、`retry_after_seconds`，并设置 `Retry-After`。
  - `400 unsupported_language`: 非 `zh-CN` 语言。

- `GET /api/public/repos/{owner}/{repo}/releases/tag/{tag}`
  - Query 同列表接口。
  - `200`: 返回共享缓存详情。
  - `202`: 仓库登记或同步尚未完成。
  - `404 release_not_found_or_not_cached`: 仓库已有同步结果但指定 tag 未命中。

- `GET /api/admin/public-release-repos`
  - 管理员会话 required。
  - 返回登记仓库、访问计数、同步状态、release 数量、翻译/润色 ready/missing 数量。

- `DELETE /api/admin/public-release-repos/{usage_id}`
  - 管理员会话 required。
  - 删除登记记录与统计。
  - 若该 `repo_id` 不再被其他公开登记或登录用户 release 可见性使用，同步清理对应 `repo_releases` 与 release AI 缓存，并在响应中返回 `cache_cleanup`。

## 数据契约

- `public_repo_release_usage` 只保存登记、统计、`repo_id` 映射、同步状态和错误。
- Release 主数据只写入并读取 `repo_releases`。
- 全局 `sync.subscriptions` 将已登记公开仓库纳入现有 repo release queue；没有用户 token 候选时可对公开仓库使用匿名 GitHub REST fallback。

## 验收标准

- Given 未登录用户首次访问公开列表页
  When 仓库尚未缓存
  Then 页面展示等待同步状态并自动重试。

- Given 未登录用户在移动端访问公开 Release 列表或详情页
  When 页脚可见
  Then 页脚展示 `Version <loadedVersion>`，有效版本号链接到 `/public/IvanLi-CN/octo-rill/releases/tag/<loadedVersion>`，且页面无横向溢出。

- Given 已登录用户点击页脚版本号
  When 跳转到 OctoRill 自身 Release 详情
  Then 页面必须使用 public-only URL 与公开 REST API，不进入 Dashboard release detail。

- Given 第三方调用公开 API 且仓库尚未缓存
  When 请求到达服务端
  Then 响应为 `202 Accepted`，包含 `Retry-After` 与 pending JSON。

- Given 第三方首次调用公开 API 且本地已有近期公开仓库 metadata 与共享 Release 缓存
  When 请求到达服务端
  Then 响应为 `200 OK`，并将公开 usage 回填到已知 `repo_id`。

- Given 公开仓库同步完成
  When 未登录页面/API 与登录用户视图读取同一 Release
  Then 内容来自同一条 `repo_releases.release_id` 记录。

- Given 管理员删除公开登记记录
  When 下一轮全局同步运行
  Then 该仓库不再因公开 usage 被纳入同步；若无其他公开登记或登录用户视图使用，则共享 Release 与 AI 缓存被清理，否则缓存保留。

## Visual Evidence

本功能的视觉证据只覆盖用户和管理员能看见的界面状态：首次访问等待、公开列表桌面端、公开列表移动端、公开详情移动端极端文本、管理后台登记与删除确认。REST API 的状态码、`Retry-After`、缓存复用和清理策略由自动化测试覆盖，不纳入截图证据。

- source_type: `storybook_canvas`
  story_id_or_title: `public-publicreleasepage--pending-sync`
  state: `public-page-pending-sync-mobile`
  evidence_note: 验证 390px 移动端首次访问公开 Release 页面且仓库尚未缓存时，页面使用面向用户的中文等待文案，不暴露 API message 或 reason code；状态胶囊展示“同步排队中 · 约 60s 后重试”，并提供手动重试入口；页头 GitHub 入口使用外链图标并指向当前仓库 Releases 页面，页脚 GitHub 入口使用 GitHub 图标并指向当前仓库根路径，页脚贴在短内容页面底部。
  PR: include
  image:
  ![公开 Release 等待同步状态](./assets/public-release-evidence-pending-mobile-v8.png)

- source_type: `storybook_canvas`
  story_id_or_title: `public-publicreleasepage--release-list`
  state: `public-release-list-desktop`
  evidence_note: 验证桌面端公开列表页展示仓库名、页面级原文/翻译/润色切换器和复用普通 Release 卡片的列表内容；卡片保留 repo identity、标题、发布时间/tag、卡片内部内容 lane 与 GitHub Release 链接，不显示不可靠 release 总数或解释性噪音。
  PR: include
  image:
  ![公开 Release 列表页桌面端](./assets/public-release-evidence-list-desktop-v8.png)

- source_type: `storybook_canvas`
  story_id_or_title: `public-publicreleasepage--release-list`
  state: `public-release-list-mobile`
  evidence_note: 验证 390px 移动端公开列表页中页面级 lane 切换器、普通 Release 卡片、移动端 GitHub icon 链接、正文展示和长内容继续滚动能在窄屏共存，且无横向溢出。
  image:
  ![公开 Release 列表页移动端](./assets/public-release-evidence-list-mobile-v8.png)

- source_type: `storybook_canvas`
  story_id_or_title: `public-publicreleasepage--long-repo-and-tag-detail`
  state: `public-release-detail-mobile-edge`
  evidence_note: 验证 390px 移动端公开详情页的最终布局：页头 LOGO 高度为 24px，右上角 GitHub 按钮使用外链图标打开仓库 Releases 页面；正文区把头像、项目名、时间/tag 作为仓库信息组，超长 repo full name 与超长 tag 单行省略，仓库名与日期行无额外垂直空隙；右侧小尺寸原文/翻译/润色选择器固定尺寸且右端对齐，短内容时页脚贴底并保留指向仓库根路径的 GitHub 入口，全页无横向溢出。
  image:
  ![公开 Release 详情页移动端极端文本](./assets/public-release-evidence-detail-mobile-edge-v8.png)

- source_type: `storybook_canvas`
  story_id_or_title: `public-publicreleasepage--long-repo-and-tag-detail`
  state: `public-release-footer-version-link-mobile`
  evidence_note: 验证 390px 移动端公开详情页的页脚在超长 repo/tag 场景下仍展示 GitHub 入口与 `Version v2.29.0`，版本号链接到 OctoRill 自身 public-only Release 详情页，且全页无横向溢出。
  image:
  ![公开 Release 页脚版本详情链接](./assets/public-release-footer-version-link-mobile.png)

- source_type: `storybook_canvas`
  story_id_or_title: `admin-publicreleaserepomanagement--default`
  state: `admin-public-release-repos-delete-confirmation`
  evidence_note: 验证管理后台展示 ready/pending 登记仓库、API/页面访问统计、Release/翻译/润色数据量，并在删除前说明若无其他使用方会清理共享 Release 与 AI 缓存。
  PR: include
  image:
  ![管理后台公开端点仓库删除确认](./assets/public-release-evidence-admin-v1.png)
