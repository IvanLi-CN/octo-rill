# 实现状态（运行时结构化日志合同与容器日志收口）

## 当前状态

- Lifecycle: active
- Implementation: 已实现
- Created: 2026-06-23
- Last: 2026-06-23
- Summary: 运行时 JSON stdout、request-id、slow/error access log、SQLite / upstream 热点字段、脱敏测试与项目文档同步已完成
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`：新增规格索引并维护实现状态摘要。
- `docs-site/docs/config.md`：新增日志与慢阈值配置入口。
- `docs/architecture.md`：新增容器日志与排障入口边界。
- `.env.example`：补齐日志相关 env 示例。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: `tracing-subscriber/json` 与 `tower-http/request-id` feature 启用。
- [x] M2: JSON stdout subscriber 与 request-id propagation middleware 接通。
- [x] M3: slow/error-only HTTP access log contract 落地。
- [x] M4: SQLite / session 写热点字段统一为 `event + operation + elapsed_ms + attempt + error_chain`。
- [x] M5: AI / sync 上游热点日志补统一字段并去除 login 等敏感披露。
- [x] M6: 自动化测试补齐 request-id / slow+error-only / 敏感字段不泄露断言。
- [x] M7: 文档、spec sync 与 project doc promotion 收口完成。
- 运行时背压补充：translation 结果聚合在直接复用 pending 快照时输出 `event=translation.backpressure` + 明确原因；feed reaction refresh 在跳过 counts persist 时输出 `event=feed.reactions.persist` + `downgrade_reason`。
