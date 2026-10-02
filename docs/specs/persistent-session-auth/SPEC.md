# GitHub 登录态持久化与稳定 session cookie

## 背景 / 问题陈述

当前 GitHub 登录态依赖服务端 session + 浏览器 cookie，但 cookie 仍是默认 session-only。结果是：

- 浏览器重开、整页 reload、版本更新提示触发的刷新后，用户容易表现成“像被更新踢下线”。
- 生产 cookie 名还会跟公开入口 host + port 推导绑定，未来若入口发生调整，整站已有 cookie 会失配。
- 前端 warm boot cache 会在本地保留“最近登录过”的启动种子；如果它的窗口和真实服务端 session 不一致，就会制造“看起来还登录着”的假象。

## 目标 / 非目标

### Goals

- 后端 session 改成 **30 天不活跃滑动过期**，并在活跃请求期间通过节流 touch 续期。
- session cookie 名不再开放运行时配置：根路径公网部署固定为 `octo_rill_sid`，本地多实例或非根路径部署自动派生隔离后缀。
- 保持现有 REST 响应结构、OAuth 流程、session 序列化与表结构契约不变；session 写入实现必须继续受应用内 SQLite writer coordinator 约束。
- 同步文档与部署口径，明确一次性重新登录预期。

### Non-goals

- 不改成 JWT / refresh token / 外部 session 基础设施。
- 不为历史旧 cookie 名或整数主键 session 做额外迁移兼容。
- 不调整 GitHub OAuth scope、用户表结构或 `/api/me` 响应字段。

## Context and Scope

- Context: OctoRill 使用服务端 SQLite session 保存 GitHub 登录态；cookie 持久化和滑动过期必须与 SQLite writer pressure 下的用户请求语义保持一致。
- In scope: session cookie contract、30 天不活跃滑动过期、节流 touch、reader/writer pool 路由、critical 与 activity-only save failure 语义，以及对应的 Rust/HTTP 验证。
- Out of scope: JWT、refresh token、外部 session 基础设施、历史 cookie 迁移和 GitHub OAuth scope 变更。

## 范围（Scope）

### In scope

- `src/server.rs` session layer 与 cookie 命名策略
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
- session load 继续使用 reader pool；create/save/delete 与过期清理必须通过共享 writer coordinator，生产文件型 SQLite 使用独立单连接 writer pool。
- session create/save/delete 的前台写入从首次进入 writer 队列开始共享一个 900 ms 单调 deadline；writer acquisition、`BEGIN IMMEDIATE`、SQL、busy retry 与 COMMIT 不得重新开始预算，且 COMMIT 已派发后必须等待 SQLite 的真实结果。
- 关键 session 写入遇到 writer deadline、busy 或 locked 时必须返回 `503`、`Retry-After: 1` 与 `sqlite_write_retryable`；解码、约束和其他真实后端错误保留各自错误类别。
- 如果请求只改变 `activity_touched_at`，刷新写入在 writer 压力下可以跳过，必须保留原业务响应且不得发送虚假的 refreshed `Set-Cookie`；同一 session 同时含有关键字段变更时不得跳过。业务响应已经成功但 activity refresh 失败时仍保留原成功响应。
- 服务对根路径公网部署统一使用 `octo_rill_sid`；本地多实例、非默认端口或非根路径部署自动派生隔离 cookie 名，避免跨实例互踢。
- 前端 startup cache 的 30 天窗口只是启动优化提示，不得被当成比 `/api/me` 更高优先级的登录真相。

## Requirements

### REQ-PERSISTENT-SESSION-001

- session cookie MUST use the documented 30-day inactivity contract, including `Max-Age=2592000`, `HttpOnly`, `SameSite=Lax`, and the existing `Secure` policy.

### REQ-PERSISTENT-SESSION-002

- Valid authenticated requests MUST refresh inactivity state only according to the existing throttle interval; the warm cache MUST NOT override `/api/me` as the source of truth.

### REQ-PERSISTENT-SESSION-003

- Session load MUST remain on the reader pool. Session create/save/delete MUST use the shared SQLite writer coordinator and its dedicated single-connection writer pool without changing the session table or MessagePack contract.

### REQ-PERSISTENT-SESSION-004

- Activity-only refresh MAY be skipped under writer pressure, but MUST preserve the original successful response and omit a refreshed cookie. A save containing any critical field change MUST NOT be skipped.

### REQ-PERSISTENT-SESSION-005

- Critical retryable session write failures MUST return `503`, `Retry-After: 1`, and `sqlite_write_retryable`; decode, constraint, and other backend failures MUST retain their real error category.

## 验收标准（Acceptance Criteria）

- Given 用户完成 GitHub 登录  
  When 后端返回 session cookie  
  Then `Set-Cookie` 包含 `Max-Age=2592000`，并保留 `HttpOnly`、`SameSite=Lax` 与原有 `Secure` 策略。

- Given 用户已登录且 session 仍有效  
  When 用户刷新页面或浏览器重开后再次访问  
  Then `/api/me` 仍返回 `200`，登录态保持。

- Given 服务运行在根路径公网部署  
  When 浏览器继续携带既有 session cookie  
  Then cookie 名保持 `octo_rill_sid`，不因为版本更新批量失效。

- Given 同一 host 上运行多个本地实例，或部署在非根路径/非默认端口  
  When 浏览器同时访问这些实例  
  Then 每个实例会得到不同的派生 cookie 名，互不覆盖登录态。

- Given 本地存在过期或失效的启动缓存
  When `/api/me` 返回 `401`
  Then 前端必须清空 warm cache 并收敛到匿名态。

- Given reader pool 连接被只读请求占用且 writer pool 仍可用
  When 已登录请求需要读取或保存 session
  Then session load 不等待 writer permit，session save 从独立 writer pool 进入 coordinator，并继续保持 30 天 cookie contract。

- Given 外部 SQLite writer lock 使 activity-only session refresh 在前台 deadline 内无法完成
  When 原业务请求已经返回成功
  Then HTTP status 与 payload 保持原值，响应不带新的 `Set-Cookie`，日志能识别 `sqlite session activity refresh failed`。

- Given 外部 SQLite writer lock 使 session critical change 在前台 deadline 内无法完成
  When session middleware 完成保存
  Then 返回 `503`、`Retry-After: 1` 与 `sqlite_write_retryable`，且不得把 retryable failure 伪装成普通 `500`。

- Given 请求已经加载了一个已有 session，而过期清理或并发操作在保存前删除了该行
  When 该请求继续保存原 session ID
  Then 返回 retryable session conflict，不能用 stale record 重新创建已删除的 session；真正的新 session 或 `cycle_id` 仍通过 `create` 使用新 ID。

## Verification

### VER-PERSISTENT-SESSION-001

- Method: Rust session-layer tests and HTTP middleware assertions for cookie attributes, expiry refresh, activity-only classification, critical failure mapping, and stale save after concurrent deletion.
- covers: REQ-PERSISTENT-SESSION-001, REQ-PERSISTENT-SESSION-002, REQ-PERSISTENT-SESSION-004, REQ-PERSISTENT-SESSION-005
- Pass condition: cookie and sliding-expiry contracts remain intact; activity-only pressure preserves the original response without a refreshed cookie; mixed/critical pressure returns the documented retryable response.

### VER-PERSISTENT-SESSION-002

- Method: file-backed SQLite WAL HTTP acceptance with independent reader/writer pools, seeded sessions, external writer lock, recovery traffic, and integrity checks.
- covers: REQ-PERSISTENT-SESSION-003, REQ-PERSISTENT-SESSION-004, REQ-PERSISTENT-SESSION-005
- Pass condition: reader-backed requests remain usable while writer pressure is active, session writes follow the coordinator contract, recovery returns to successful traffic, and evidence is tied to the current candidate SHA.

## 非功能性验收 / 质量门槛

### Testing

- Rust tests 覆盖：
  - 根路径公网部署保持固定 cookie 名
  - 本地多实例 / 非根路径部署使用隔离 cookie 名
  - `Set-Cookie` 持久化 `Max-Age`
  - 有效请求的滑动续期
  - 无效 sid 的清 cookie 行为
  - activity-only refresh pressure 下保留业务响应且不发虚假 cookie
  - mixed critical session change pressure 下返回 retryable 503
- Playwright / 集成验证覆盖：
  - 401 收敛时清空 warm cache
  - 启动阶段不把 stale cache 当最终登录真相
  - file-backed SQLite WAL 下真实 HTTP session save、dashboard read、task enqueue 的 lock/recovery 验收

### Quality checks

- `cargo test`
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cd web && bun run lint`
- `cd web && bun run e2e -- app-auth-boot.spec.ts`

## 风险 / 假设

- 风险：固定 cookie 名上线后，旧 cookie 名不会自动平滑迁移；允许一次性重新登录。
- 风险：如果部署仍把 SQLite 放在非持久层，即使 cookie 持久化也无法保住服务端 session。
- 假设：部署继续保持稳定的数据库与加密密钥。

## Related ADRs

None
