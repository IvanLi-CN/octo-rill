# OctoRill

OctoRill is a personal GitHub activity workspace. This glossary fixes the product terms that shape reading, admin, and sync surfaces so future changes do not drift across similar-but-different repository concepts.

## Language

**项目处理仓库总数**:
The deduplicated count of repositories OctoRill currently processes for one user across watched repositories and owned-repository baselines. One repository counts once even if it appears in multiple sources.
_Avoid_: 关注 + 私有仓库, 仓库总数（未说明口径）, private repo total

**关注仓库**:
A repository stored from the user's GitHub starred-repository snapshot. This is the canonical social/release source for explicit user attention.
_Avoid_: watched repo, processed repo, owned repo

**自有仓库基线**:
A repository baseline discovered from the current GitHub viewer's owner repository snapshot and stored for release/social processing. It is not equivalent to a watched repository or a private-repository flag.
_Avoid_: 私有仓库, star baseline, watched repo

**我的发布纳入状态**:
The user preference that controls whether owned-repository baselines participate in release visibility. It describes release inclusion, not repository ownership or sync freshness.
_Avoid_: 私有仓库开关, owner repo enabled

**LLM 逻辑调用**:
一次由 OctoRill 调度并最终归属于单个模型的 LLM 请求。内部重试仍属于同一次逻辑调用，最终只产生一个成功或失败结果。
_Avoid_: 执行, attempt, 单次重试

**原始动态分页**:
按单条 GitHub 活动排序的完整浏览序列，用于精确筛选和列表阅读；它不等同于日报阅读序列。
_Avoid_: 日报分页, 读模型分页

**可读区块分页**:
Dashboard 根页 `全部` tab 的阅读序列；每一页至少交付一个此前未呈现的完整日报面板或原始活动组。历史日报是完整的可读区块，其覆盖的原始记录数不应中断连续阅读。
_Avoid_: 可见进展分页, 手动续载, raw cursor

**完整日报区块**:
一个包含日报完整正文的历史可读区块。它不是摘要或正文占位符；进入主阅读流即应可阅读，原始发布记录与其他活动明细仍独立按需加载。
_Avoid_: 日报摘要, 预览日报, 空日报面板

**区块明细**:
一个可读区块所代表的完整原始 GitHub 动态集合，只在用户选择“列表”阅读时按需呈现；它不决定主阅读序列的推进，也不等同于日报视图中的补充动态。
_Avoid_: 主分页内容, 预加载活动, 补充动态

**补充动态**:
日报窗口内没有被该日报覆盖的原始活动。在日报视图中与完整日报一起呈现；切换到“列表”后，由完整区块明细取代。
_Avoid_: 日报摘要, 完整区块明细, 已覆盖活动

**无日报历史区块**:
一个历史自然日尚未生成日报时的可读区块。它默认以原始动态列表呈现，并在进入视图时按需加载首批明细；生成日报只能由用户明确发起。
_Avoid_: 自动生成日报, 空白历史区块

**采集记录**:
管理员可见的一条持久化来源实体，包括 Release、公告或日报；无论是否已创建处理任务或尝试记录，它都必须可见。
_Avoid_: 重试记录, LLM 调用记录, 已处理记录

**采集记录来源时间**:
用于筛选采集记录的业务时间：Release 使用发布时间，公告使用发生时间，日报使用生成时间。它与发现时间不同。
_Avoid_: 发现时间, 同步时间

**发现时间**:
OctoRill 首次观察到来源实体的时刻。它是可选的审计来源信息；无法从历史数据可靠恢复时，界面显示“未知”。
_Avoid_: 发布时间, 生成时间, 筛选时间

**处理总尝试次数**:
一条采集记录在全部适用处理链路中的最大总执行次数，包含首次执行与重试。零表示所有适用链路均未开始执行；它不同于重试次数。
_Avoid_: 重试次数, LLM 逻辑调用数

**可见进展分页**:
Dashboard `全部` tab 的分页规则：自动续载只在新页扩展用户可见的日组、日报面板或原始活动时继续；若新页完全被既有历史日报折叠，则转为用户点击的显式续载。
_Avoid_: 无限自动补拉, 哨兵重试, 隐藏页循环
