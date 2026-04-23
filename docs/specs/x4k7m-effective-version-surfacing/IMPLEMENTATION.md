# 实现状态（发布有效版本显示修复（API + Footer + Release 注入闭环））

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-03
- Last: 2026-04-15
- Summary: 已交付；PR #20；新增 /api/version，health/version 同源，footer 回退 health；release web-builder fallback + build gate docker smoke + historical backfill overlay
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 后端版本解析模块与 `/api/version` 完成。
- [x] M2: 前端 footer 双端点回退逻辑完成。
- [x] M3: Docker/release 注入链路校验与自动化测试完成。
- [x] M4: 前端 `vite.config.ts` fallback 容错与 CI Docker smoke 完成。
