# 历史

- 2026-05-04: 新建规格，锁定公开 Release 外链、REST API、pending retry 语义、共享 `repo_releases` 复用与管理后台登记列表范围。
- 2026-05-06: 收敛管理删除语义：无人使用时清理共享 Release、AI 缓存与 release sync state；仍被历史 brief membership 引用时保留共享缓存。
- 2026-05-07: 公开 Release 页面页脚补齐 OctoRill 前端加载版本号；有效版本号链接到 OctoRill 自身 public-only Release 详情页，并补充移动端视觉证据。
- 2026-05-07: 公开文档站补齐 `公开 Release 接入` 页面，作为第三方展示特定 GitHub 仓库 Releases 的稳定接入入口。
- 2026-05-08: 公开 Release 首次登记改为优先复用近期本地公开仓库 metadata 与共享 Release 缓存，避免已有缓存的公开仓库卡在 metadata pending；同时 SQLite 主连接池改为默认 8 且可通过环境变量调整。
