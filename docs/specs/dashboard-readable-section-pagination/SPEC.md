# Dashboard 可读区块分页

> 本文定义 Dashboard 根页 `全部` tab 的长期阅读与分页合同；实现覆盖见 `./IMPLEMENTATION.md`，主题演进见 `./HISTORY.md`。

## Context and Scope

- Context: 原始动态 cursor 的记录级排序与日报优先的区块级阅读不是同一个序列。客户端折叠已加载记录后，不能可靠地判断继续请求是否会产生新的可读内容。
- In scope: 非 scoped Dashboard 根页 `全部` tab 的服务端可读区块顺序、区块明细按需加载、自动分页、日报与补充动态的呈现边界。
- Out of scope: `/api/feed` 的原始 cursor 合同、`发布`/`加星`/`关注` tab、scoped focus、日报生成算法、日报 tab 和侧栏的独立分页策略。

## Terms and Interfaces

- `可读区块`: 根页 `全部` tab 中按时间连续呈现的日报区块或原始活动区块。
- `完整日报区块`: 带完整 `content_markdown` 的历史日报，以及该日报没有覆盖的补充动态。
- `补充动态`: 位于同一日报窗口、但没有被该日报覆盖的原始活动；它不是日报摘要，也不是完整原始列表。
- `区块明细`: 某区块的完整原始活动集合，只在用户切换到“列表”时按需获取。
- `区块 cursor`: 服务端不透明 continuation token，表示已交付完整区块的排他时间边界。
- Interface: [`./contracts/http-apis.md`](./contracts/http-apis.md)。

## Requirements

### REQ-READABLE-SECTIONS-001

- 系统 MUST 为非 scoped 的 Dashboard `全部` tab 提供按可读区块分页的服务端接口。
- Inputs: 已认证用户与可选的不透明区块 cursor。
- Outputs: 时间连续、互不重复的零至三个区块和下一页 cursor；只要还有区块，每个成功页至少包含一个此前未交付的区块。
- covers: Dashboard 根页历史阅读与自动分页。

### REQ-READABLE-SECTIONS-002

- 系统 MUST 在历史日报区块中直接返回并渲染完整日报正文，而不是 `preview_markdown`、摘要或正文占位。
- 系统 MUST 同时返回该日报未覆盖的补充动态；已被日报覆盖的活动不得在日报视图重复显示。
- covers: 日报优先阅读与信息完整性。

### REQ-READABLE-SECTIONS-003

- 系统 MUST 只在用户选择某日报区块的“列表”后获取其完整原始活动集合。
- 区块明细 MUST 使用独立、不透明且只对该区块有效的 cursor，每页最多返回三十个原始活动；继续加载明细不得阻塞主区块流。
- covers: 原始活动按需加载与区块隔离。

### REQ-READABLE-SECTIONS-004

- 系统 MUST 将没有日报的历史日期作为原始活动区块呈现，并在该区块进入视图时按需取得首批明细。
- 系统 MUST 保留用户显式生成日报的入口，且不得因为滚动或进入视图自动调用日报生成。
- covers: 无日报历史的可读性与 LLM 调用边界。

### REQ-READABLE-SECTIONS-005

- 客户端 MUST 在主区块流触底时自动请求下一页，并对同一 cursor 保持单请求去重。
- 请求中 MUST 显示水平居中的无文字三点波浪加载胶囊；失败时 MUST 在同一位置显示可访问的重试控件，并以延迟 tooltip 提供说明。
- 正常主流不得显示 `继续加载历史动态`，且到达末页时保持既有居中末页状态。
- covers: 连续阅读、加载反馈和失败恢复。

### REQ-READABLE-SECTIONS-006

- 普通同步、日报生成和日报内容刷新 MUST 不使已取得的区块 cursor 被服务端拒绝；服务端按 cursor 的排他边界继续读取当前投影。
- 日界设置变化、tab/scope 变化或用户明确刷新时，客户端 MUST 取消过期请求并从首屏重新建立正确的阅读序列。
- covers: 续页稳定性与显式重分组。

### REQ-READABLE-SECTIONS-007

- 根页 `全部` tab 的首屏与后续区块请求 MUST 不以 `/api/feed` 或全量 `/api/briefs` 作为日报分组的前置数据。
- 服务端 MUST 使用可按用户与区块时间边界检索的读模型；实现必须用查询计划验证分页查找不会依赖无界原始活动扫描。
- `/api/feed`、`/api/briefs` 与其现有消费者 MUST 保持兼容。
- covers: 性能边界与兼容性。

## Verification

### VER-READABLE-SECTIONS-001

- Method: Rust API fixture test。
- covers: `REQ-READABLE-SECTIONS-001`, `REQ-READABLE-SECTIONS-006`
- Pass condition: 夹有大量同一日报已覆盖活动的时间窗仍返回后续未交付区块；普通同步或日报生成后，旧 cursor 继续返回严格更早且不重复的区块。

### VER-READABLE-SECTIONS-002

- Method: Dashboard Playwright mock cursor 序列。
- covers: `REQ-READABLE-SECTIONS-001`, `REQ-READABLE-SECTIONS-002`, `REQ-READABLE-SECTIONS-005`
- Pass condition: 滚动后自动取得新完整日报区块，不出现显式续载命令；日报正文与未覆盖补充动态可见，且请求期间显示加载胶囊。

### VER-READABLE-SECTIONS-003

- Method: Dashboard Playwright mock 与组件 interaction test。
- covers: `REQ-READABLE-SECTIONS-003`, `REQ-READABLE-SECTIONS-004`
- Pass condition: 初始日报流不请求区块完整明细；点击“列表”后只请求该区块首批三十条，区块内续页不阻塞主流；无日报区块不自动生成日报。

### VER-READABLE-SECTIONS-004

- Method: SQL `EXPLAIN QUERY PLAN` fixture test。
- covers: `REQ-READABLE-SECTIONS-007`
- Pass condition: 主区块查询使用用户/区块边界索引，且不需要扫描未限制的原始活动集合来找下一页。

### VER-READABLE-SECTIONS-005

- Method: Storybook canvas 与窄屏截图。
- covers: `REQ-READABLE-SECTIONS-002`, `REQ-READABLE-SECTIONS-005`
- Pass condition: 完整日报、补充动态、列表按需加载、三点波浪加载、错误重试和居中末页在桌面与窄屏均无重叠或溢出。

## Related ADRs

- [ADR 0002: Dashboard 可读区块分页](../../adr/0002-dashboard-readable-section-pagination.md)

## Visual Evidence

- None until the implementation satisfies the verification contract.

## References

- `./contracts/http-apis.md`
- `./IMPLEMENTATION.md`
- `./HISTORY.md`
- `../dashboard-day-grouping/SPEC.md`
- `../dashboard-brief-social-folding/SPEC.md`
- `../brief-snapshot-timezone/contracts/http-apis.md`
