# 实现状态（Passkey 登录与 GitHub 绑定 Onboarding）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Last: 2026-04-23
- Summary: 已交付；fast-track / PR-ready / passkey login + GitHub bind onboarding + settings management + landing passkey icon + 统一 Landing 认证动作互斥与 OAuth 跳转反馈 + review fixes for first-paint support + today_live cutoff + pending-passkey oauth/linuxdo consumption
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新

- `docs/specs/README.md`
- `docs/product.md`
- `docs-site/docs/product.md`
- `docs-site/docs/config.md`
- `docs-site/docs/quick-start.md`
- `web/src/pages/Landing.tsx`、`web/src/stories/AppLanding.stories.tsx` 与 `web/e2e/landing-login.spec.ts` 覆盖统一认证动作状态、OAuth 首击反馈、重复点击拦截及互斥禁用。
