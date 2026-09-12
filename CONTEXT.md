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

**Webhook 推送目标状态**:
The user's intended lifecycle for release notifications: enabled, paused, or deleted. It is an instruction that remains stable while the remote GitHub Hook is being reconciled.
_Avoid_: 当前 Hook 状态, 注册结果, 接收状态

**Webhook Hook 观察状态**:
The latest local observation of a repository's OctoRill-managed GitHub Hook, such as registered, missing, conflicted, permission-paused, or errored. It describes what was observed, not what the user wants.
_Avoid_: Webhook 推送目标状态, 健康开关

**Webhook 对齐操作**:
A user-scoped background operation that moves verified GitHub Hooks toward the persisted Webhook 推送目标状态 and records progress, retries, and terminal errors.
_Avoid_: 注册按钮, 同步请求, 检查记录

**协作取消**:
A request for a running Webhook 对齐操作 to stop at a safe remote-request boundary. It does not change the user's Webhook 推送目标状态.
_Avoid_: 撤销目标状态, 删除 Hook, 人工重试

**LLM 逻辑调用**:
一次由 OctoRill 调度并最终归属于单个模型的 provider 请求。内部路由重试仍属于同一次逻辑调用；其成功只表示已收到模型响应，不表示响应已通过业务输出契约。
_Avoid_: 内容处理尝试, 业务处理成功, 单次重试

**内容处理尝试**:
翻译或润色 work item 的一次完整处理过程，涵盖模型调用、响应校验、结果写入与恢复安排。它的结果是业务结果，而不是 provider 请求状态。
_Avoid_: LLM 逻辑调用, 批次, 重试记录

**输出契约结果**:
模型响应是否满足某个内容处理阶段要求的结构与语义，例如 JSON 可解析、字段完整或 Markdown 结构匹配。它独立于模型调用的 provider 结果。
_Avoid_: 模型调用状态, 翻译状态, 润色状态

**调用归因链接**:
一条内容处理尝试与实际参与该尝试的 LLM 逻辑调用之间的阶段化因果关系。批次共同成员不构成调用归因。
_Avoid_: batch 调用列表, 推断关联, 同批调用

**诊断载荷**:
管理员用于排查近期内容处理问题的 prompt、规范化消息、模型响应与 provider 交付元数据。它是短期受控证据，不属于长期处理尝试审计。
_Avoid_: 尝试审计, 公开错误详情, 永久日志

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
用于筛选采集记录的业务时间：Release 使用发布时间，公告使用发生时间，通知使用规范通知来源的更新时间，日报使用生成时间。它与发现时间不同。
_Avoid_: 发现时间, 同步时间

**采集记录查询窗**:
一次管理员采集记录读取允许覆盖的、以采集记录来源时间界定的连续范围。它独立于页面大小和用户翻页位置。
_Avoid_: 页码范围, 发现时间窗口, 缓存周期

**发现时间**:
OctoRill 首次观察到来源实体的时刻。它是可选的审计来源信息；无法从历史数据可靠恢复时，界面显示“未知”。
_Avoid_: 发布时间, 生成时间, 筛选时间

**处理总尝试次数**:
一条采集记录在全部适用处理链路中的最大总执行次数，包含首次执行与重试。零表示所有适用链路均未开始执行；它不同于重试次数。
_Avoid_: 重试次数, LLM 逻辑调用数

**可见进展分页**:
Dashboard `全部` tab 的分页规则：自动续载只在新页扩展用户可见的日组、日报面板或原始活动时继续；若新页完全被既有历史日报折叠，则转为用户点击的显式续载。
_Avoid_: 无限自动补拉, 哨兵重试, 隐藏页循环

**规范资源**:
独立于任一用户、可按访问控制授权读取的 Release、公告或通知。它是共享内容处理工作与结果的唯一资源身份。
_Avoid_: 用户记录, 用户副本

**规范通知来源**:
同一通知线程的当前规范来源：选择更新时间最新的来源行；更新时间相同时，使用稳定来源标识确定唯一结果。它决定该线程的来源时间和显示内容，而不归属于任一用户。
_Avoid_: 字典序最大标题, 任意用户副本, 首次发现记录

**采集记录读取预算违约**:
管理员采集记录读取未能在服务响应预算内返回的结果。它与数据库查询错误不同；CDN 的超时状态只是该违约可能产生的边缘表现。
_Avoid_: CDN 错误码, SQL schema 错误, 缓存未命中

**源快照**:
一个内容处理工作项接收时冻结的规范输入与哈希。来源随后变化会产生新快照，不会原地改写旧快照。
_Avoid_: 实时来源, 可变输入

**内容处理链路**:
应用于规范资源并具有特定变体和输出契约的“翻译”或“润色”能力。
_Avoid_: 智能摘要, 替代功能名称

**全局工作项**:
由调度器拥有、对应一个全局工作身份的执行单元。它没有用户所有者；请求和读取时才检查授权。
_Avoid_: 用户任务, 用户作业

**结果投影**:
一个全局工作身份最新的、已经通过完整校验的输出。在处理更新的源快照时，旧投影仍可以继续读取。
_Avoid_: 工作状态, 状态缓存

**请求者关联**:
把调用者或系统生产者与全局工作项相连的授权和审计事实。它不使输出归该调用者私有。
_Avoid_: 结果所有者, 工作所有者

**旧事实证据**:
从原用户隔离表或缓存保留的不可变事实。它可以解释历史展示，但不能证明当前全局工作已经执行。
_Avoid_: 重建工作项, 合成尝试
