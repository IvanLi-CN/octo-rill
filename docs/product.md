# OctoRill 产品说明

OctoRill 是一个 GitHub 信息聚合与阅读界面：提供一个 **Releases 信息流**（类似 GitHub dashboard 的阅读体验），并用 AI 自动翻译成用户语言（当前默认中文）；同时提供 **Release 日报** 与 **Inbox（GitHub Notifications）** 的快捷入口。日报中的 release 主入口先在站内打开详情卡，其余外链统一跳转 GitHub。

## 核心体验

- **信息流（Feed）**：仅展示 Releases，按时间倒序排列，支持无限滚动加载更多。
- **AI 自动翻译**：Release 条目在进入视口时触发翻译并缓存；AI 未配置或翻译失败时回退显示原文。
- **Release 日报**：根据固定时间边界生成“昨日更新”日报，按项目分组覆盖完整 release 内容，并输出可点击链接。
- **Inbox 快捷入口**：把 GitHub Notifications 抄一份到侧栏，提供快速跳转入口。

## 页面结构（Web）

### Tabs：全部 / Releases / 日报 / Inbox

页面整体采用**固定左右两栏**布局：

- **左侧主列**：根据 Tab 展示对应内容（Releases Feed / 日报 / Inbox 列表）。
- **右侧侧栏**：展示“其他内容”，在看 Feed（全部 / Releases）时包含：
  - **Inbox 快捷入口**
  在「日报」Tab 下：右侧包含「日报列表 + Inbox 快捷入口」；在「Inbox」Tab 下：右侧保留 **Inbox 快捷入口** 作为常驻入口。

Tab 语义：

- **全部（聚合）**：左侧展示 Releases 信息流；右侧展示「Inbox 快捷入口」。
- **Releases**：左侧仅展示 Release 信息流（Feed）；右侧展示「Inbox 快捷入口」。
- **日报**：左侧展示日报内容查看（并支持手动生成）；右侧展示「日报列表 + Inbox 快捷入口」。
- **Inbox**：左侧展示通知列表（用于查看与跳转），不进入 Feed；右侧仍展示 Inbox 快捷入口。

### 主列：Releases 信息流（无限滚动）

信息流条目仅包含一种：

1) **Release 条目**
- 展示：仓库名、发布版本（tag 或 release name）、发布时间、（可选）中文翻译标题与发布说明译文（摘录）
- 反馈：展示 GitHub 同款反馈表情（👍 😄 ❤️ 🎉 🚀 👀），支持查看计数；站内点按切换需要用户提供 PAT
- 操作：点击“在 GitHub 打开”跳转到对应 Release 页面（新标签页）

### 日报（独立 Tab）

- 时间窗口：以 `AI_DAILY_AT_LOCAL` 设定的本地时间（例如 `08:00`）为边界，
  窗口为「昨日 08:00（本地）→ 今日 08:00（本地）」。
- 默认边界：若未设置 `AI_DAILY_AT_LOCAL`，默认按本地 `08:00` 计算窗口。
- 内容结构固定为：
  - `## 概览`：时间窗口、项目数、release 数、预发布数
  - `## 项目更新`：按项目分组逐条列出 release 与变更要点
- 链接策略：
  - release 主链接：站内链接 `/?tab=briefs&release=<release_id>`，点击后在 briefs 页展示详情卡
  - 其他链接：跳转 GitHub（仓库页、release 原文页、相关 GitHub 链接）
- 链接完整性保障：日报润色后会按 `release` 查询参数做精确 `release_id` 校验，缺失时自动补链（避免 `12/123` 前缀误判）。
- 用途：让用户快速了解这段时间发生了什么，并能一跳查看完整上下文。

### Release 详情卡（briefs 内联）

- 入口：日报中的 release 主链接（站内链接）触发打开。
- 内容：
  - 默认显示完整中文翻译（支持切换原文）
  - 保留 Markdown 结构（标题、列表、代码块、表格、链接）
  - 提供 “GitHub” 外链按钮查看原始 release 页面

### 右侧侧栏：Inbox 快捷入口

- 展示最近的通知条目（优先未读）。
- 用途：作为快速入口方便用户查看；不试图替代 GitHub 的完整通知工作流。

## AI 翻译策略

- 默认语言：中文（`zh-CN`）。
- 触发方式：**懒加载自动**（条目进入视口时触发），并写入本地缓存。
- 回退策略：
  - 未配置 AI（缺少 `AI_API_KEY`）：直接显示原文，不显示“翻译中”假状态。
  - AI 调用失败：显示原文，并允许用户重试。
- 输出要求：保留版本号/仓库名/代码片段/专有名词，不做过度“意译”。

## 数据来源与同步

- 登录方式：GitHub OAuth。
- OAuth 职责：用于登录、读取和同步（Feed / Notifications / Starred / Releases）。
- OAuth scope：采用最小授权策略（不包含用于站内反馈写操作的额外 scope）。
- Release 反馈写操作：使用用户提供的 GitHub PAT（Personal Access Token），与 OAuth 通道分离。
  - Fine-grained PAT：按 GitHub Reactions 接口可不额外申请 repository permissions，但 token 必须覆盖目标仓库。
  - Classic PAT：公共仓库建议 `public_repo`，私有仓库需 `repo`。
- 本地存储：SQLite（默认 `./.data/octo-rill.db`）。
- Release 数据语义：按“共享事实”处理，release 以稳定 `release_id` 引用；用户 Star 仅用于决定个人列表与同步范围。
- 同步数据：
  - Starred repos（用于确定关注范围）
  - Releases（共享事实数据；用于信息流、日报与详情）
  - Notifications（用于 Inbox 列表与侧栏快捷入口）
- 日报回填：
  - 服务启动时会为每个用户先同步一次 releases，再补齐最近 7 天日报（已存在日期会跳过）
- 可见性规则：
  - 取消 Star 后，该仓库可从当前用户的列表中消失；
  - 但历史日报中的 release 详情链接（`/?tab=briefs&release=<release_id>`）仍应可访问。
- 语义边界：
  - 列表与日报候选按 Star 过滤（个性化可见范围）。
  - 详情读取与详情翻译按 `release_id` 读取（共享事实读取），不依赖当前 Star 状态。

## Release 反馈授权策略

- 默认可用：所有 Release 卡片都展示反馈计数。
- 站内点按前置条件：当前用户已配置 PAT，且 PAT 对目标仓库可访问。
- 未配置或无权限时：反馈保持只读并显示引导信息；不影响 Feed 阅读与同步流程。
- 交互约束（当前任务补充）：
  - 点击反馈按钮且未配置 PAT：必须弹出 PAT 配置对话框（不能只在卡片底部报错）。
  - 对话框必须给出配置路径与最小权限说明（classic PAT：公共仓库 `public_repo`、私有仓库 `repo`）。
  - 输入 PAT 后自动可用性检查，防抖窗口为 `800ms`，且仅最后一次输入结果生效。
  - 检查状态必须可见（`idle/checking/valid/invalid`）；仅 `valid` 状态允许保存并继续。

## 非目标（本项目不做什么）

- 不做完整 GitHub 客户端：不在站内处理评论、合并、标记已读等操作。
- 不做多语言设置页：当前固定中文。
- 不做持续后台全量自动同步：仍以用户手动触发 Sync 为主（仅在服务启动回填日报时补做一次 releases 同步）。
