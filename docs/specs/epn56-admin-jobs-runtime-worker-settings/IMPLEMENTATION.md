# 实现状态（管理员任务中心运行时 worker 数量设置）

## 当前状态

- Lifecycle: superseded(#y2yf8)
- Implementation: 重新设计（#y2yf8）
- Created: 2026-03-27
- Last: 2026-04-13
- Summary: 已由 #y2yf8 接替；historical worker-only contract superseded by #y2yf8 translation runtime settings
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)
- Superseded by: [../y2yf8-release-translation-input-budget-runtime/SPEC.md](../y2yf8-release-translation-input-budget-runtime/SPEC.md)

## 当前实现说明

- 后端已支持 LLM permit 热更新，并通过单例 `admin_runtime_settings` 记录 `llm_max_concurrency`、`translation_general_worker_concurrency`、`translation_dedicated_worker_concurrency`。
- 所有 live runtime 会在 `runtime_owner` heartbeat 与管理员状态读取时回拉 `admin_runtime_settings`，因此无需显式广播也能把并发设置收敛到最新持久化值。
- translation scheduler 已从固定 `3 + 1` runtime 改为动态 worker registry，worker 以 `general` 在前、`user_dedicated` 在后生成连续槽位与稳定 `worker_id`。
- `GET /api/admin/jobs/translations/status` 同时返回 live worker 统计与 `target_*` 目标配置，页面文案与设置回填读取 `target_*`，排空中的工作者板卡片继续读取 live `workers`。
- 管理页已增加两个设置按钮和对应对话框，并在保存成功后立即刷新当前卡片/工作者板状态。
- Storybook 与 Playwright 已覆盖打开弹窗、非法值校验、保存成功与工作者板数量变化路径。

## 文档更新（Docs to Update）

- `README.md`
- `docs-site/docs/config.md`
- `docs-site/docs/quick-start.md`
- `docs/specs/README.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 单例 runtime settings 表、启动种子逻辑与接口契约冻结。
- [x] M2: LLM permit gate 与 translation worker runtime registry 支持热更新与排空缩容。
- [x] M3: 管理端设置按钮、弹窗、Storybook 与 Playwright 覆盖完成。
- [x] M4: 视觉证据、PR 与 review-loop 收敛到 merge-ready。
