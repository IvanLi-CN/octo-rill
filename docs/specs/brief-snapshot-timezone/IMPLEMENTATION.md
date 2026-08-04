# 实现状态（日报快照与时区去敏改造）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-13
- Last: 2026-07-15
- Summary: 已交付；brief 内容窗口统一改成“按用户时区回顾昨天自然日”，自动出报时间改为管理员统一维护的全局运行时设置，Dashboard/历史折叠/任务诊断与历史重算全部对齐自然日快照语义。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现概述

- 新增 `src/briefs.rs` 统一承载用户偏好解析、IANA 时区校验、DST 决策与自然日窗口计算；服务端任何 brief 日期与窗口计算都必须复用该模块。
- 通过 migration 扩展 `users`、重建 `briefs`、新增 `brief_release_memberships`，并引入 `(user_id, window_start_utc, window_end_utc)` 唯一性。
- `src/ai.rs` 中的 brief 生成链路统一写入 snapshot 与 memberships；`src/jobs.rs` 的定时槽调度按用户偏好决定每天几点触发，而内容窗口固定回顾该用户上一个已完整结束的本地自然日。
- Dashboard `FeedGroupedList` 结合 memberships 构建历史 brief 组，raw day grouping 仅对未命中 snapshot 的项目兜底。
- `GET /api/briefs` 继续保持 snapshot-friendly 的瘦列表合同（窗口、时区、memberships、preview），前端完整正文统一走 detail lazy-load；历史卡与 `/briefs` 主卡共享同一份 detail 缓存与 in-flight 去重。
- Web 端新增普通用户“日报时区设置”与管理员全局出报时间配置，所有历史/详情展示都读取落库 snapshot 字段。

## 测试与验证

- `cargo check`
- `cargo test`
- `cd web && npm run test:day-groups`
- `cd web && npm run build`
- `cd web && npm run storybook:build`
- 重点视觉场景：
  - 普通用户日报设置表单（默认 / 错误 / DST）
  - 管理员用户详情中的日报设置编辑
  - Dashboard 历史 brief 折叠与 raw fallback
  - Admin task details 的 brief 生成 / daily slot / history recompute 诊断
