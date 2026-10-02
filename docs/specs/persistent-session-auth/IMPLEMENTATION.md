# 实现状态（GitHub 登录态持久化与稳定 session cookie）

## 当前状态

- Lifecycle: active
- Implementation: PR3.9.2 session writer isolation and pressure semantics implemented; candidate `674a5e69` passed fresh WAL HTTP acceptance, statement-interruption recovery, quality gates and persistence checks; fresh review, CI and merge remain
- Created: 2026-04-21
- Last: 2026-10-02
- Summary: 30d sliding session + stable cookie-name config retained; coordinated reader/writer session persistence and pressure-aware middleware added in the PR3.9.2 candidate
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 创建并冻结持久 session 规格与文档入口。
- [x] M2: 后端 session layer 支持 30 天不活跃滑动过期与固定 cookie 名。
- [x] M3: 前端 startup cache 语义与公开配置文档同步收口。
- [ ] M4: 验证、review-loop、PR 合并与 cleanup 完成。

## Current implementation coverage

- Session loads use the reader pool; create/save/delete use direct SQL on the coordinator's dedicated writer pool and preserve the existing `tower_sessions` MessagePack/table contract.
- Foreground session writes share the coordinator's monotonic deadline, bounded busy retry, and commit-result contract. Expired-session cleanup remains best-effort.
- Activity-only refresh failures preserve the original successful HTTP response and omit a refreshed cookie; critical or mixed session changes retain retryable `503` behavior with `Retry-After: 1`.
- Unit coverage includes cookie persistence, activity-only classification, critical failure mapping, stale concurrent session-field merging, and a file-backed WAL reader/writer contention path. Candidate `674a5e69` passed the fresh acceptance matrix with no unexpected failures: baseline and competition remained `200`, external lock downgraded foreground writes to retryable `503`, recovery and sustained traffic remained successful, and the real statement-interruption probe returned `503` before a successful `/api/me` recovery.
