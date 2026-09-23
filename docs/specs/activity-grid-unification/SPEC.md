# 统一活动图组件与探索交互

## Context and Scope

管理端的采集记录活动图与 LLM 活动图都以状态格表达时间或模型维度的活动，但目前各自实现格子几何、渲染、悬浮信息、键盘交互与滚动容器。采集记录图的格子过大、初次加载没有等尺寸骨架，并且详情路由变化会间接重读活动图和列表。移动端还缺少能在不劫持页面滚动的前提下探索密集格子的交互。

本主题将两类视图统一到一个真实的 `ActivityGrid` UI 组件，并为采集记录活动图提供小尺寸、无面板滚动的探索体验。它保留各业务视图的读模型、状态语义、详情动作和 LLM 的可操作摘要；不改变后端活动接口、状态聚合规则、列表筛选语义或详情内容。

## Requirements

- REQ-ACTIVITY-GRID-001: 必须提供一个真实渲染的 `ActivityGrid` 组件，作为采集记录活动图和 LLM 活动图的唯一网格渲染入口。两类业务视图必须成为数据和动作适配器；它们不得各自保留网格几何、命中测试、选中态、骨架、预览浮层或输入事件的平行实现。
- REQ-ACTIVITY-GRID-002: `ActivityGrid` 必须接受统一的 `ActivityGridModel`，其中每个 cell 至少包含稳定标识、视觉状态、可访问名称、预览摘要和激活动作。模型必须支持 `wrapped-rows`（按小时分行、每行可变数量）与 `matrix`（模型行乘时间桶列）两种布局，以及 `page` 与 `panel` 两种滚动策略。业务适配器拥有数据映射、状态颜色、详情目标和 LLM 调用语义；组件拥有呈现与交互。
- REQ-ACTIVITY-GRID-003: 采集记录的 `wrapped-rows` 布局必须使用 12px x 12px 的实际格子和 2px 间距，格子实际可交互区域也为该尺寸。LLM 的 `matrix` 布局在桌面必须使用同等的 12px 视觉格子和 2px 节奏；在窄平板和手机分别可降至 11px 与 9px，以保留现有容器自适应容量且不产生横向溢出。
- REQ-ACTIVITY-GRID-004: 采集记录活动图必须使用 `page` 滚动策略：完整的最近 12 小时内容参与页面滚动，组件内部不得设置纵向滚动容器或固定可滚动视口。超过高密度阈值时允许使用有界高度的 Canvas 分段绘制，但 Canvas 必须继续参与页面流，不得引入内部滚动容器或面板滚动语义。LLM 活动图必须使用 `panel` 滚动策略，保留其受限高度的内部纵向浏览能力。
- REQ-ACTIVITY-GRID-005: 组件必须提供与最终布局同几何的 loading skeleton。采集记录在首次读取、无缓存读取和首次进入无缓存 tab 时，骨架必须预留统计、标题、图例及 12 小时网格的最终空间，使用相同的行、格子和间距；加载完成前后不得出现可感知的结构性位移。已有数据的后台刷新必须保留真实数据并只显示就地更新反馈，不得退回骨架。
- REQ-ACTIVITY-GRID-006: 桌面端指针悬浮某个可用格子时，必须显示其简要信息浮层；点击格子必须触发该视图的激活动作。键盘焦点必须显示同一预览，方向键在逻辑相邻格子之间移动焦点，Enter 触发激活动作。DOM 与 Canvas 渲染路径必须提供等价的选择、键盘和辅助技术语义。
- REQ-ACTIVITY-GRID-007: 采集记录图在触摸设备上必须以 150ms 长按进入探索模式。按下后在激活前移动超过 8px 时，手势必须继续作为原生页面滚动；进入探索后，手指移动必须更新当前格子。预览浮层必须跟随触点上方、不可接收指针事件，并通过偏移、翻转和边界偏移始终完整留在视口内。松手必须打开当前采集记录详情；快速拖动不得误开详情。
- REQ-ACTIVITY-GRID-008: 采集记录预览必须展示标题、仓库、来源时间、综合处理状态、翻译状态和润色状态。预览位置必须随视口滚动与尺寸变化重新计算。LLM 适配器必须继续提供现有的可固定、可操作调用摘要，但由共享组件的预览与定位能力承载。
- REQ-ACTIVITY-GRID-009: 打开、关闭或在详情内切换采集记录的尝试与 LLM 调用信息时，只允许读取所需的详情数据。不得重新读取活动图或下方列表，不得重置已加载的活动数据、列表数据、筛选、分页、页面滚动位置或当前网格选择。
- REQ-ACTIVITY-GRID-010: 采集记录活动图必须继续覆盖 Release、公告、通知和日报四个 tab；LLM 活动图必须继续保留模型行、时间桶、状态图例、调用选中态与既有可操作性。组件复用不得改变既有活动读模型的状态含义、时间边界或统计口径。
- REQ-ACTIVITY-GRID-011: Admin Jobs Web Demo 的 Inspector 必须提供可分享的 Data case 控制，至少支持 `有数据（代表性示例）`、`无数据`、`多行活动（同一小时 128 格）` 和 `加载中`。代表性示例必须覆盖中性、已完成、处理中和异常状态；多行活动必须让至少一个小时桶的格子在当前桌面宽度下换成两行或以上；切换后必须让活动图与列表读取对应数据；加载中必须保持最终几何的活动图骨架和列表加载态，且 Data case 与 Network 状态不得复用其他状态的缓存响应。

## Non-goals

- 不改动采集记录活动接口、LLM 活动接口或其后端聚合规则。
- 不改变详情页展示的字段、尝试或调用历史的业务含义。
- 不把所有管理端数据图表重构为 `ActivityGrid`。
- 不以扩大单格命中区域的方式改变采集记录格子的 12px x 12px 交互尺寸。

## Verification

- VER-ACTIVITY-GRID-UNIFIED-RENDER: covers: REQ-ACTIVITY-GRID-001, REQ-ACTIVITY-GRID-002, REQ-ACTIVITY-GRID-010。以 `wrapped-rows` 和 `matrix` 两类夹具验证两个业务适配器均经由同一组件渲染，且各自状态、摘要与激活动作保持正确。
- VER-ACTIVITY-GRID-DENSITY-AND-SCROLL: covers: REQ-ACTIVITY-GRID-003, REQ-ACTIVITY-GRID-004。验证采集记录格子及间距为 12px/2px、12 小时内容无内部滚动；验证 LLM 在桌面、窄平板和手机的 12px、11px、9px 密度以及保留受限面板滚动。
- VER-ACTIVITY-GRID-LOADING: covers: REQ-ACTIVITY-GRID-005。比较初始 loading、无缓存 tab 与已缓存刷新状态的布局边界：前两者使用等尺寸骨架且完成后无结构性位移，后者保留真实内容并显示就地更新状态。
- VER-ACTIVITY-GRID-DESKTOP-INTERACTION: covers: REQ-ACTIVITY-GRID-006, REQ-ACTIVITY-GRID-008。验证桌面悬浮、点击、焦点、方向键和 Enter 在 DOM 与 Canvas 路径下的同等行为，浮层在滚动和调整视口后仍完整可见；验证 LLM 的固定调用摘要继续可操作。
- VER-ACTIVITY-GRID-TOUCH-INTERACTION: covers: REQ-ACTIVITY-GRID-007。以触摸序列验证 150ms/8px 判定、页面滚动让渡、连续格子探索、视口内跟手预览、松手打开详情和快速拖动不误开。
- VER-ACTIVITY-GRID-DATA-ISOLATION: covers: REQ-ACTIVITY-GRID-009。拦截活动、列表和详情读取，验证打开、关闭及详情内导航只产生详情读取，并完整保留活动图、列表、筛选、分页、页面滚动与当前选择。
- VER-ACTIVITY-GRID-DEMO-DATA-CASES: covers: REQ-ACTIVITY-GRID-011。通过 Demo Inspector 切换四种 Data case，验证 URL share state、列表行数、活动图统计、空状态和同几何骨架；验证 Network 状态变化不会复用正常响应缓存。

## Interfaces & Contracts

| Interface | Kind | Scope | Change | Contract | Owner | Consumers |
| --- | --- | --- | --- | --- | --- | --- |
| `ActivityGrid` | React component | web internal | Add | 两种布局、两种滚动策略、统一 cell/preview/action model | web UI | 采集记录活动图、LLM 活动图 |
| `ActivityGridModel` | TypeScript model | web internal | Add | stable cell identity、visual state、preview、activation 与布局数据 | web UI | 业务适配器、`ActivityGrid` |
| 采集记录活动读取 | HTTP API | external | Retain | 继续提供四类采集记录的 12 小时只读活动数据 | backend | 采集记录适配器 |
| LLM 活动读取 | HTTP API | external | Retain | 继续提供模型与时间桶活动数据 | backend | LLM 适配器 |
| 采集记录详情路由 | browser route | web internal | Modify | 详情导航不得使活动或列表查询重新生效 | web UI | 采集记录页面、详情页 |

## Related ADRs

None

## Visual Evidence

- `assets/admin-jobs-many-dense-ego.png`: Ego Lite Web Demo 在桌面视口中展示 `多行活动（同一小时 128 格）`；`09时` 桶实际换成两行方块。
