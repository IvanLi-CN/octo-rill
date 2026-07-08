# 任意 Public Repo Feed 与关注仓库体系 Implementation

## Status

- Lifecycle: active
- Delivery mode: fast-track
- Current state: 主实现与 UI 收口已落地；视觉证据已刷新，等待 PR merge gate 最终收口

## Scope coverage

- [x] 新增 `user_repo_associations` schema、索引与历史回填
- [x] star / owned / manual_feed 三类来源 upsert helper 与 follow 状态机
- [x] `/api/feed` repo-scope public repo 关联登记与 release-first warmup
- [x] `HEAD /api/feed` 预热语义
- [x] `/api/feed` 与 `/api/dashboard/updates` 的 `scope=following`
- [x] `GET /api/repos/following`
- [x] `PUT /api/repos/{owner}/{repo}/following`
- [x] `DELETE /api/repos/{owner}/{repo}/following`
- [x] 后台 following release 池纳入 refresh 口径
- [x] `/focus/following` 路由与账号菜单入口
- [x] repo 聚焦页与 following 页的 follow/unfollow UI
- [x] Storybook 场景与前端构建验证
- [ ] Rust 自动化测试在当前仓库基线下恢复为绿

## Validation target

- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`

## Notes

- 本主题复用 `#84nup` 的 Bearer API Key 业务接口鉴权边界，不新增匿名读取面。
- 本主题复用 `#p8r3l` 的 public release usage 与异步同步链路，但不回写其匿名 public page 主题边界。
- 本主题扩展 `#u1f6v` 的 scoped focus feed 契约，新增 `following` scope 与关注仓库阅读面。
- 当前验证结果：
  - `cargo check` 通过
  - `cd web && bun run lint` 通过
  - `cd web && bun run build` 通过
  - `cd web && bun run storybook:build` 通过
  - `cargo test` 未收敛：仓库当前存在大量既有失败用例，失败面横跨 `ai` / `api` / `admin_runtime` / `jobs` 旧测试，不是本主题单点新失败即可直接归因的状态
  - 视觉证据已刷新：following 页面默认态 / 关联态 / 眼睛 icon follow 切换已落盘到 spec `visual-evidence/`
