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
The latest local observation of a repository's OctoRill-managed GitHub Hook, such as registered, archived, conflicted, permission-paused, outside-PAT-scope, or errored. It describes what was observed, not what the user wants.
_Avoid_: Webhook 推送目标状态, 健康开关

**Webhook 对齐操作**:
A user-scoped background operation that moves verified GitHub Hooks toward the persisted Webhook 推送目标状态 and records each repository's disposition independently.
_Avoid_: 注册按钮, 同步请求, 检查记录

**Webhook 仓库处置结果**:
The result of reconciling one repository, such as registered, archived, action-required, or outside-PAT-scope. It does not determine whether the enclosing Webhook 对齐操作 completed.
_Avoid_: 批次失败, Webhook 推送目标状态, 全局错误

**Webhook 对齐需求**:
A durable per-user request created with a newly persisted eligible owned-repository baseline and consumed by a Webhook 对齐操作. It coalesces concurrent discovery without dropping work while an operation is in flight.
_Avoid_: 直接注册, 尽力入队, 新仓库任务

**协作取消**:
A request for a running Webhook 对齐操作 to stop at a safe remote-request boundary. It does not change the user's Webhook 推送目标状态.
_Avoid_: 撤销目标状态, 删除 Hook, 人工重试

**LLM 逻辑调用**:
一次由 OctoRill 调度并最终归属于单个模型的 provider 请求。内部路由重试仍属于同一次逻辑调用；其成功只表示已收到模型响应，不表示响应已通过业务输出契约。
_Avoid_: 内容处理尝试, 业务处理成功, 单次重试

**内容处理尝试**:
翻译或润色 work item 的一次完整处理过程，涵盖模型调用、响应校验、结果写入与恢复安排。它的结果是业务结果，而不是 provider 请求状态。
_Avoid_: LLM 逻辑调用, 批次, 重试记录

**尝试配置快照**:
一次内容处理尝试开始时确定的当前模型路由与运行配置。该快照在本次尝试期间保持不变；后续尝试重新按开始时的当前配置选择。它属于尝试事实，不属于规范资源或全局工作项身份。
_Avoid_: 资源模型绑定, 全局工作配置

**配置阻塞**:
当前全局模型配置不可执行时暂停的全局工作状态，不代表源内容永久失败。配置变更后重新验证；配置可用时排入使用新尝试配置快照的工作。配置无效期间不按计时器反复请求 provider。
_Avoid_: 永久失败, provider 冷却

**输出契约结果**:
模型响应是否满足某个内容处理阶段要求的结构与语义，例如 JSON 可解析、字段完整或 Markdown 结构匹配。它独立于模型调用的 provider 结果。
_Avoid_: 模型调用状态, 翻译状态, 润色状态

**规范内容输出**:
通过内容处理输出契约、以声明的目标字段表达结果的内容输出。它是结果投影的输入，不因 provider 已成功返回而自动成立。
_Avoid_: provider 响应, 原始模型输出, LLM 成功

**输出包装兼容**:
对不改变内容含义、且边界明确的模型响应包装进行有限归一化，再执行规范内容输出校验。无法唯一确定内容字段的包装仍属于输出契约失败。
_Avoid_: 任意格式容错, 静默修复, 内容重写

**截断输出**:
provider 在内容输出尚未完整结束时终止的响应。它与 JSON 包装或字段契约错误不同，需要独立的恢复次数和终态审计。
_Avoid_: 空响应, provider 不可用, 输出包装

**事故观察 cohort**:
由明确事故时间窗和审计事实预先确定的一组全局工作项，用于验证事故影响和恢复结果。它不是新的恢复入口；工作项仍沿通用自动恢复和授权用户重试规则处理。
_Avoid_: 全量回填, 事故专用重试, 手工改库

**调用归因链接**:
一条内容处理尝试与实际参与该尝试的 LLM 逻辑调用之间的阶段化因果关系。批次共同成员不构成调用归因。
_Avoid_: batch 调用列表, 推断关联, 同批调用

**诊断载荷**:
管理员用于排查近期内容处理问题的 prompt、规范化消息、模型响应与 provider 交付元数据。它是短期受控证据，不属于长期处理尝试审计。
_Avoid_: 尝试审计, 公开错误详情, 永久日志

**采集记录**:
管理端“内容处理”中按 Release、公告或日报呈现的源记录及其处理摘要。它不是任务、处理工作项、尝试事件或 LLM 逻辑调用。
_Avoid_: 任务记录, work item, attempt

**Demo 场景**:
mock-only Web Demo 的页面级路由预设，由 `demo` share state 选择。它定义页面上下文，不定义 Admin Jobs 内的读取 surface 或 fixture 数据。
_Avoid_: Admin Jobs surface, data case, tab

**Admin Jobs surface**:
Admin Jobs 中可独立验收的只读读取面：内容处理（采集记录）或 LLM 调度。它由正式 Admin Jobs 子路由表达；每个 surface 拥有互不重叠的 endpoint family 和 Demo Inspector share state。
_Avoid_: Demo 场景, 内容处理尝试, LLM 逻辑调用

**Surface data case**:
一个 Admin Jobs surface 的 URL-shareable mock 响应选择：`loaded`、`empty`、`many` 或 `loading`。`loading` 保持该 surface 的读取请求 pending，其他值定义该 surface 返回的 fixture。
_Avoid_: network profile, Demo 场景, 全局数据状态

**Surface network profile**:
一个 Admin Jobs surface 的 URL-shareable 传输行为选择：`normal`、`slow` 或 `faulty`。它独立于 Surface data case；`faulty` 产生该 surface 的读取错误，`slow` 延迟非 `loading` fixture。
_Avoid_: Surface data case, 全局 `d_net`, 数据缓存

**采集记录检索窗口**:
管理员读取采集记录时以含 `from`、不含 `before` 定义的时间范围；产品支持的最大跨度为三十一天。
_Avoid_: 读取超时, 分页范围, 数据保留期

**精确匹配总数**:
在一个采集记录检索窗口内满足当前类型、处理状态和尝试次数筛选的全部记录数量。它独立于当前页返回的记录数。
_Avoid_: 预估总数, 当前页数量, 原始记录总量

**处理摘要**:
采集记录针对翻译或润色展示的当前状态、重试次数和关键时间信息。它是管理员的安全读取视图，不包含源文本、prompt、模型原始输出或原始上游错误。
_Avoid_: 任务详情, 原始处理结果, LLM 调用

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

**内容处理活动窗**:
内容处理管理页固定展示的、以采集记录来源时间分桶的最近十二个自然小时；它包含当前尚未结束的小时。
_Avoid_: 记录列表筛选窗, 重试时间窗, LLM 调用窗口

**内容处理活动格**:
内容处理活动窗中代表一条采集记录的单个可交互格子。它不代表某次内容处理尝试或某次 LLM 逻辑调用。
_Avoid_: 尝试格, 调用格, 聚合计数格

**综合处理状态**:
管理员为一条采集记录汇总其适用内容处理链路后得到的只读分类：已完成、处理中、异常或中性。
_Avoid_: work item 状态, LLM 调用状态, 单链路状态

**中性处理状态**:
采集记录尚无当前处理事实，或仅有不能证明当前处理结果的旧事实证据时的综合处理状态。它不等同于已完成、处理中或异常。
_Avoid_: 已完成, 异常, 可重试失败

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

**版本目录**:
公开 Release 阅读器中按时间顺序呈现仓库全部 Release 的导航视图。它与详情时间线使用同一序列，用于定位和定向，不是筛选后的副本。
_Avoid_: 版本过滤器, 独立版本数据源, 缩略详情

**详情时间线**:
公开 Release 阅读器中按与版本目录相同顺序呈现 Release 完整内容的阅读视图。
_Avoid_: 单条详情页, 版本目录, 第二份 Release 列表

**阅读当前版本**:
用户正在详情时间线中阅读的单个 Release。它由明确的版本链接、目录选择或可见阅读位置确定，并驱动版本目录的选中状态。
_Avoid_: URL 指定版本与正在阅读版本长期并存, 多个当前版本

**显式版本链接**:
直接定位某一 Release 的公开链接。它与无 tag 的公开列表浏览入口不同：前者表达可复现的版本位置，后者表达从最新 Release 开始的浏览。
_Avoid_: 普通列表入口, 仅视觉高亮

**规范通知来源**:
同一通知线程的当前规范来源：选择更新时间最新的来源行；更新时间相同时，使用稳定来源标识确定唯一结果。它决定该线程的来源时间和显示内容，而不归属于任一用户。
_Avoid_: 字典序最大标题, 任意用户副本, 首次发现记录

**采集记录读取预算违约**:
管理员采集记录读取未能在服务响应预算内返回的结果。它与数据库查询错误不同；CDN 的超时状态只是该违约可能产生的边缘表现。
_Avoid_: CDN 错误码, SQL schema 错误, 缓存未命中

**源快照**:
一个内容处理工作项接收时冻结的规范输入与哈希。来源随后变化会产生新快照，不会原地改写旧快照。
_Avoid_: 实时来源, 可变输入

**源版本顺序**:
用于判断同一规范资源的源快照先后关系的权威顺序；它能够区分同时产生、乱序到达或重复观察的快照，不以同步请求到达顺序代替来源版本顺序。
_Avoid_: 同步到达顺序, 最后写入顺序, 随机并列排序

**已替代工作项**:
其源快照已被同一规范资源、内容处理链路和输出协议下的更新源快照替代的全局工作项。它是终态历史事实，不再进入队列、运行或产生新的规范内容输出。
_Avoid_: 过期工作, 旧任务, 重试窗口到期

**重试窗口到期**:
全局工作项的自动恢复时间边界已经结束的事实。它不表示源快照已被替代；符合授权边界的手动重试可以按现行策略重新打开恢复流程。
_Avoid_: 已替代工作项, 过期工作, 永久失败

**provider 调用准入**:
一次内容处理尝试确认其源快照仍可处理，并获得发起 provider 请求资格的业务边界。准入之后源内容才发生变化时，该调用仍属于已准入尝试，但其结果可能随后成为已替代事实。
_Avoid_: provider 成功, 内容处理成功, 调用开始时间

**工作准入事件**:
记录生产者提交、接受、无变化或因源版本已替代而拒绝全局工作项的事实。它独立于内容处理尝试；没有产生 provider 调用的准入结果也必须能够被审计。
_Avoid_: 尝试排队事件, LLM 逻辑调用, 重试记录

**Markdown 结构契约**:
内容输出必须保持 Markdown 区块的类型、顺序、数量及边界；同一区块内部的自然换行可以变化，但不能借此新增、删除或错位内容区块。
_Avoid_: 逐行字符串相等, 任意格式容错, 只校验 JSON

**内容处理链路**:
应用于规范资源并具有特定变体和输出契约的“翻译”或“润色”能力。
_Avoid_: 智能摘要, 替代功能名称

**全局工作项**:
由调度器拥有、对应规范资源的内容处理链路、源版本和输出协议的执行单元。模型配置不属于工作项身份；它由各次内容处理尝试分别记录。它没有用户所有者；请求和读取时才检查授权。
_Avoid_: 用户任务, 用户作业

**结果投影**:
同一规范资源、内容处理链路、源版本和输出协议下，最近一次通过完整校验并发布的输出。模型配置不属于结果身份；实际使用的模型由生成它的内容处理尝试记录。刷新期间旧投影仍可继续读取。
_Avoid_: 工作状态, 状态缓存

**请求者关联**:
把调用者或系统生产者与全局工作项相连的授权和审计事实。它不使输出归该调用者私有。
_Avoid_: 结果所有者, 工作所有者

**旧事实证据**:
从原用户隔离表或缓存保留的不可变事实。它可以解释历史展示，但不能证明当前全局工作已经执行。
_Avoid_: 重建工作项, 合成尝试

**搜索文档**:
工作区搜索中的一个规范可见内容对象，包含内容类型、仓库归属、来源时间、阅读目标及可用的原文、翻译或润色表达；同一对象的不同表达不构成多个文档。
_Avoid_: 搜索副本, 搜索命中行, GitHub 全网结果

**有效搜索请求**:
一个已登录用户提交的、符合工作区搜索语法且不存在未知、重复或冲突过滤条件的搜索请求；即使没有结果，它仍是一次有效请求。
_Avoid_: 搜索词记录, 有结果搜索, 命令动作

**搜索窗口**:
从用户第一次有效搜索请求开始计算的连续五分钟配额周期；周期内允许的搜索次数固定，不随每次请求向前滑动。
_Avoid_: 滑动窗口, 搜索历史, 全局窗口

**命中 lane**:
搜索文档实际匹配的内容表达层：原文、翻译或润色；同一搜索文档无论命中几层都只呈现一个结果，并保留实际命中层。
_Avoid_: 结果副本, 默认显示层, 内容版本

**命令动作**:
命令面板允许用户明确触发的、具有固定权限边界和可预期目标的工作区操作；它不是搜索结果，也不因展示或执行而计入搜索额度。
_Avoid_: 任意命令, 后台任务, 搜索动作

**确认进度**:
由全量同步任务的 `task.progress` SSE 事件确认的阶段进度。它只表示后端已经报告完成的阶段，不包含客户端为了连续展示而计算的视觉预测值。
_Avoid_: 预测进度, 视觉百分比, 同步耗时

**预测进度**:
Dashboard 同步按钮在两个确认进度锚点之间展示的受限客户端视觉进度。它只能单调向下一个阶段锚点逼近，不能倒退或越过下一个锚点；它不是后端事实，也不用于详情数字、业务判断或 ETA 承诺。
_Avoid_: 真实进度, 完成比例, 预计完成时间

## Repository workflow

**提交时快速检查**:
绑定普通 `git commit` 的低延迟检查，只负责在提交边界发现格式、前端静态检查和提交消息问题；它不等价于完整测试，也不应隐式启动全量测试。
_Avoid_: 提交全量测试, commit-time full suite

**显式本地验证**:
由开发者主动选择并执行的针对性或完整本地检查。它提供开发反馈，但不能替代 PR、合并或交付阶段的远端门禁证据。
_Avoid_: 隐式本地验证, 提交钩子全量验证

**完整 CI 门禁**:
由 GitHub Actions 在 PR、merge group 或 `main` push 上针对当前提交执行的适用 required checks 集合。它是合并和发布前的完整质量证据，而不是单个测试命令的别名。
_Avoid_: 单元测试门禁, 任意 CI 通过

**交付验证**:
绑定已合并到 `main` 的目标提交、并在发布自动化继续之前确认其 `main` push CI 成功的验证阶段。
_Avoid_: 发布前本地测试, 标签即验证

**重型验证**:
需要浏览器、Docker、跨平台 runner 或受控性能环境的验证，例如完整 Playwright、发布镜像 smoke、worktree bootstrap 和性能验收；它不属于普通提交或本机 pre-push 的隐式职责。
_Avoid_: 本地提交检查, 快速检查

**Rust 源码质量契约**:
Rust 源码在格式、语义 lint、源码结构和 host/features 覆盖四个层面上的可重复质量边界。它描述源码是否可维护和可审阅，不等同于服务业务测试或发布成功。
_Avoid_: cargo check 通过, 单个 Clippy 命令, 发布门禁

**服务运行时契约**:
长期运行的 Rust service 对监听地址、认证与会话、SQLite 持久化、HTTP/SSE 端点、静态资产、后台 worker、健康探针和关闭行为承担的稳定边界。
_Avoid_: 后端 API 列表, 前端运行时, Docker build 成功

**遗留 suppression 基线**:
当前源码中已存在、经过明确记录并允许暂时保留的 lint 豁免集合。它不是新增豁免的默认授权；新增或扩大豁免必须以窄范围和可审查理由更新基线。
_Avoid_: 全局关闭 lint, zero-debt 已完成, 静默 allow
