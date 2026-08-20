# 演进记录（多 GitHub 绑定与 LinuxDO 首登补绑改造）

## 生命周期

- Lifecycle: active
- Created: 2026-04-21
- Last: 2026-04-21

## 历史摘要

- 2026-04-21: 建立该主题规格并冻结基础范围。
- 2026-04-21: 已交付；PR #117; fast-track / multi-github + linuxdo onboarding / 去主账号语义并收口运行时迁移；legacy schema cleanup deferred
- Landing 的 GitHub、LinuxDO 与 Passkey 入口统一认证忙碌状态；OAuth 首击显示跳转反馈并拦截重复认证动作，同时保留原始链接与修饰键语义。
- Demo Inspector 为 Landing 提供可分享的登录交互 Case 选择器，覆盖 OAuth 跳转、Passkey 处理中与降级故障状态，且不触发真实认证导航。
- Demo Inspector 将 Case 预设与场景专属控制分层；Landing 可组合认证动作、Passkey 能力和认证启动状态，切换其他 Scene 会加载目标身份并保留跨场景网络与发布上下文。
