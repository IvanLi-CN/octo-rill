# 历史

- 2026-05-04: 新建规格，锁定公开 Release 外链、REST API、pending retry 语义、共享 `repo_releases` 复用与管理后台登记列表范围。
- 2026-05-06: 收敛管理删除语义：无人使用时清理共享 Release、AI 缓存与 release sync state；仍被历史 brief membership 引用时保留共享缓存。
- 2026-05-07: 公开 Release 页面页脚补齐 OctoRill 前端加载版本号；有效版本号链接到 OctoRill 自身 public-only Release 详情页，并补充移动端视觉证据。
- 2026-05-07: 公开文档站补齐 `公开 Release 接入` 页面，作为第三方展示特定 GitHub 仓库 Releases 的稳定接入入口。
- 2026-05-08: 公开 Release 首次登记改为优先复用近期本地公开仓库 metadata 与共享 Release 缓存，避免已有缓存的公开仓库卡在 metadata pending；同时 SQLite 主连接池改为默认 8 且可通过环境变量调整。
- 2026-05-08: 主连接池放大后，高竞争后台写事务必须提前获取 SQLite writer slot；repo release / translation scheduler 的 claim / attach 路径按该约束收敛，避免 `SQLITE_BUSY_SNAPSHOT` 让计划任务连续失败。
- 2026-07-07: 公开 Release 模型扩展为 `github_public` / `private_owner_published` 双 access kind；GitHub public repo 默认公开，viewer-owned private repo 需拥有者显式发布，并补齐 `/public/:owner/:repo/releases` 到 canonical `/:owner/:repo/releases` 的 replace 跳转。
- 2026-07-09: 匿名 public proof 从“只认 recent public starred metadata”扩到“信任 fresh 的 `starred_repos ∪ owned_repo_star_baselines` privacy metadata”；public owned repo 若已有共享 `repo_releases` 缓存，首访可直接 `ready`，但 stale / unknown / fresh private metadata 仍保持 `metadata_pending`。
- 2026-07-10: 公开 Release 列表新增 `release_id` 高亮深链。后端直接解析离散 ID 或倒序闭区间，复用唯一 Release 查询与排序索引返回最多 12 条聚焦窗口、双向 opaque cursor、解析/未解析元数据和项目高亮标记；前端新增自动视口定位、older 追加、newer 前置与滚动锚点补偿，旧 `/public/.../releases` 跳转保留 query。
- 2026-07-10: 高亮深链合同扩展为 typed tag/ID selector、20 个离散目标、30 条分段推荐窗口、内部 gap 自动填充、`highlight_active` 导航与动态高度虚拟列表；公开列表和详情默认切换到润色并携带原文回退。
- 2026-07-11: 公开列表页头将页面级 lane selector 提升到仓库标题带；桌面端保持标题与 selector 同行，窄屏整块换行，顶栏字标调整为桌面 32px、移动端 28px。
- 2026-07-11: 公开列表仓库身份进一步收敛到页面标题带，使用 owner/avatar 加仓库名；列表卡片移除重复身份、card-level lane 与 GitHub 操作。
- 2026-07-11: 公开列表表情反应改为依赖认证会话与有效 PAT；匿名不读取 PAT 或反应数据，无效/失效 PAT 不展示控件。刷新按最多 100 条分批，且只有当前用户可操作的 Release 显示控件，避免公开链接上的无效反应操作。
- 2026-07-11: 修复反应刷新与列表窗口更新的竞态；已发出的有效刷新响应继续按 `release_id` 合并，防止分页、gap 或高亮加载期间的控件被永久隐藏。
- 2026-07-11: 反应批次刷新改为独立收敛：成功批次立即合并，非 PAT 瞬态失败仅重试其所属 Release 至多三次，避免多批次请求互相吞掉结果。
