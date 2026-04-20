# GitHub 登录态持久化与稳定 session cookie（#gr8kr）

## 状态

- Status: 部分完成（3/4）
- Created: 2026-04-21
- Last: 2026-04-21

## 背景 / 问题陈述

当前 GitHub 登录态依赖服务端 session + 浏览器 cookie，但 cookie 仍是默认 session-only。结果是：

- 浏览器重开、整页 reload、版本更新提示触发的刷新后，用户容易表现成“像被更新踢下线”。
- 生产 cookie 名还会跟公开入口 host + port 推导绑定，未来若入口发生调整，整站已有 cookie 会失配。
- 前端 warm boot cache 会在本地保留“最近登录过”的启动种子；如果它的窗口和真实服务端 session 不一致，就会制造“看起来还登录着”的假象。

## 目标 / 非目标

### Goals

- 后端 session 改成 **30 天不活跃滑动过期**，并在活跃请求期间通过节流 touch 续期。
- 新增可选 `OCTORILL_SESSION_COOKIE_NAME`，允许生产固定 cookie 名。
- 保持现有 REST 响应结构、OAuth 流程和 SQLite session store 不变。
- 同步文档与部署口径，明确一次性重新登录预期。

### Non-goals

- 不改成 JWT / refresh token / 外部 session 基础设施。
- 不为历史旧 cookie 名或整数主键 session 做额外迁移兼容。
- 不调整 GitHub OAuth scope、用户表结构或 `/api/me` 响应字段。

## 范围（Scope）

### In scope

- `src/server.rs` session layer 与 cookie 命名策略
- `src/config.rs` 新环境变量解析
- `web/src/auth/startupCache.ts` 的口径与注释
- `.env.example`、`README.md`、`docs-site/docs/config.md`
- Rust / Playwright 回归验证

### Out of scope

- 部署域名迁移
- 历史 session 兼容读回填
- GitHub 登录页面视觉改造

## 功能与行为规格

- 登录成功后返回的 `Set-Cookie` 必须带 `Max-Age=2592000`，不再是 session-only cookie。
- 有效已登录请求会通过节流 touch 刷新 cookie 与服务端 session 的不活跃过期时间，避免对 SQLite 造成每请求写放大。
- 若设置 `OCTORILL_SESSION_COOKIE_NAME`，服务必须优先使用该固定名称；未设置时继续回退到现有“按公开入口推导”行为。
- 前端 startup cache 的 30 天窗口只是启动优化提示，不得被当成比 `/api/me` 更高优先级的登录真相。

## 验收标准（Acceptance Criteria）

- Given 用户完成 GitHub 登录  
  When 后端返回 session cookie  
  Then `Set-Cookie` 包含 `Max-Age=2592000`，并保留 `HttpOnly`、`SameSite=Lax` 与原有 `Secure` 策略。

- Given 用户已登录且 session 仍有效  
  When 用户刷新页面或浏览器重开后再次访问  
  Then `/api/me` 仍返回 `200`，登录态保持。

- Given 用户已经设置固定 `OCTORILL_SESSION_COOKIE_NAME`  
  When 服务升级但 `OCTORILL_PUBLIC_BASE_URL` 的推导逻辑发生变化  
  Then cookie 名仍保持固定，不因为版本更新批量失效。

- Given 本地存在过期或失效的启动缓存  
  When `/api/me` 返回 `401`  
  Then 前端必须清空 warm cache 并收敛到匿名态。

## 非功能性验收 / 质量门槛

### Testing

- Rust tests 覆盖：
  - 固定 cookie 名配置优先级
  - `Set-Cookie` 持久化 `Max-Age`
  - 有效请求的滑动续期
  - 无效 sid 的清 cookie 行为
- Playwright / 集成验证覆盖：
  - 401 收敛时清空 warm cache
  - 启动阶段不把 stale cache 当最终登录真相

### Quality checks

- `cargo test`
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cd web && bun run lint`
- `cd web && bun run e2e -- app-auth-boot.spec.ts`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 创建并冻结持久 session 规格与文档入口。
- [x] M2: 后端 session layer 支持 30 天不活跃滑动过期与固定 cookie 名。
- [x] M3: 前端 startup cache 语义与公开配置文档同步收口。
- [ ] M4: 验证、review-loop、PR 合并与 cleanup 完成。

## 风险 / 假设

- 风险：固定 cookie 名上线后，旧 cookie 名不会自动平滑迁移；允许一次性重新登录。
- 风险：如果部署仍把 SQLite 放在非持久层，即使 cookie 持久化也无法保住服务端 session。
- 假设：生产会同步固定 `OCTORILL_SESSION_COOKIE_NAME`，并继续保持稳定的数据库与加密密钥。
