# 演进记录（Passkey 登录与 GitHub 绑定 Onboarding）

## 生命周期

- Lifecycle: active
- Last: 2026-04-23

## 历史摘要

- 2026-04-23: 已交付；fast-track / PR-ready / passkey login + GitHub bind onboarding + settings management + landing passkey icon + review fixes for first-paint support + today_live cutoff + pending-passkey oauth/linuxdo consumption
- Landing 的 GitHub、LinuxDO 与 Passkey 入口统一为互斥认证动作；OAuth 首击显示跳转反馈并保留原始链接语义，Passkey 加载与错误清理行为不变。
