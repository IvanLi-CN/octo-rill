# 演进记录（仓库刷新治理页与预算调度收敛）

## 生命周期

- Lifecycle: active
- Created: 2026-06-29
- Last: 2026-06-29

## 变更记录

- 2026-06-29: 新建 spec，正式把“仓库治理”从 `s8qkn` 的历史非目标中拆出，收口为独立后台一级页与 budgeted system refresh contract。
- 2026-06-29: 明确 `repo_total` 与 scheduler 共享“有效关注池”语义，替代 `n6zd8` 中 `starred ∪ owned baseline` 的旧宽口径说明。
- 2026-06-29: 后端新增 repo governance snapshots / cycles、budgeted system 选仓与 stargazer cache 排序键；前端新增 `/admin/repos` 治理页、共享 runtime budget 编辑入口，并补齐 Storybook 桌面/窄屏证据。
- 2026-06-29: 预算编辑入口进一步收口到任务中心“订阅同步设置”弹窗；`/admin/repos` 改为只读治理视图，移除重复预算编辑卡与定时任务设置中的重复预算项。
- 2026-06-29: 治理页补齐活动图 a11y 等价信息、概览/明细分离状态、深色语义色与预算 CTA 单跳自动展开；订阅同步设置弹窗同步收紧术语并提升触达可用性。
- 2026-06-29: 治理页进一步改为中文优先术语，补活动图 decoder，并把仓库明细收口成更易比较的排序/目标窗口/迫切值结构；订阅同步设置弹窗同步统一“Release 抓取并发”命名。
