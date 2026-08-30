# 实现状态（管理员 LLM 多模型路由与故障切换）

## 当前状态

- Lifecycle: active
- Implementation: 已实现
- Created: 2026-06-28
- Last: 2026-08-16
- Summary: 多模型路由与固定 50 小时活动接口、按容器宽度动态取最近窗口的活动视图、活动图/卡片切换、无滚动紧凑网格、portal 聚合浮窗、可分享的调用筛选和排障跳转均已实现。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/admin-llm-multi-model-routing/SPEC.md`
- `docs-site/docs/config.md`
- `docs-site/docs/quick-start.md`
- `.env.example`

## 计划资产（Plan assets）

- Directory: `docs/specs/admin-llm-multi-model-routing/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: `admin_runtime_settings.llm_models_json` migration、启动 seed/backfill、runtime snapshot 与 live scheduler 同步。
- [x] M2: `ai.rs` 多模型选模、连续最终失败计数、10 分钟冷却、预算按实际候选模型计算、状态接口扩展。
- [x] M3: 管理台多模型编辑、排序/增删、逐模型状态展示、Storybook 夹具与 E2E 更新。
- [x] M4: 完成 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`web` lint/build/storybook/e2e 与视觉证据落盘。
- [x] M5: 固定 50 小时管理员活动接口、override 对账、历史模型排序与 Rust API 覆盖。
- [x] M6: 活动网格、视图切换、列聚合交互、独立刷新状态及 mock / Storybook / Playwright 覆盖。
- [x] M7: 完成最终质量门禁与桌面/移动视觉证据。
- [x] M8: 聚合浮窗改为固定 portal 定位与安全区检测；完成按容器宽度动态展示（最多 50 桶）的无滚动网格、移动图例、组件与页面视觉证据。
- [x] M9: 时间刻度改为水平边界内对齐；末桶替代过近的常规刻度，并以页面回归覆盖首尾完整性与相邻标签不重叠。
- [x] M10: 浮窗与活动网格增加矮视口高度上限和纵向滚动，保证极端模型数量下仍不产生页面横向溢出或不可见的浮窗内容。
- [x] M11: 调用列表支持精确模型和终态 `[from, before)` 筛选；活动图/模型卡通过可访问上下文菜单跳转，URL 可恢复筛选，结果区域会滚动并获得焦点。
- [x] M12: 逻辑调用失败分类、路由恢复、持久化健康窗口、翻译恢复梯度、真实 `partial` 结果与默认关闭灰度开关。
