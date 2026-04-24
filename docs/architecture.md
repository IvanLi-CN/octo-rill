# OctoRill 系统概览

这份文档从维护视角描述 OctoRill 当前的系统组成、运行边界与主要代码落点。它不复述完整产品语义；产品层回答“为什么要这样做”时，以 [`product.md`](./product.md) 为准。

## 系统由哪些部分组成

OctoRill 当前由 4 个长期维护面组成：

- **Rust 后端（`src/`）**：Axum 服务，负责认证、API、同步、翻译、日报、管理员运行面与静态资源托管
- **Web 前端（`web/`）**：React + Vite 单页应用，也是登录后 Dashboard / Admin 的真实交付面
- **Storybook（`web/` 内）**：前端稳定验证面，用来固定页面状态、视觉边界与交互回归
- **公开文档站（`docs-site/`）**：Rspress 站点，承载面向外部的启动、配置与产品说明

## 运行时边界

### 浏览器侧

浏览器只承接前端壳层、页面状态与交互，不直接接触 GitHub、LinuxDO 或 AI 提供方。所有业务数据都通过后端 API 进入页面。

### 后端侧

后端统一负责：

- 会话与登录状态
- GitHub / LinuxDO OAuth 回调
- Passkey challenge 与验证
- GitHub 数据同步与派生视图
- 翻译请求、批次执行与日报生成
- 管理员运行时配置与后台任务观测

### 数据与外部依赖

- **GitHub**：用户连接、release、notification、social activity 的事实来源
- **LinuxDO**：补充身份绑定来源，不替代 GitHub 作为主账号约束
- **AI provider**：只负责翻译 / 润色等 LLM 能力
- **SQLite**：本地缓存、派生数据与会话存储；不是最终事实源

## 启动与应用装配

### 进程入口

- `src/main.rs` 负责读取 `.env.local` / `.env`、初始化 tracing，然后把 `AppConfig` 交给 `server::serve()`
- `src/server.rs` 负责完成真正的运行时装配：数据库、migrations、session store、HTTP client、OAuth client、WebAuthn、scheduler、router 与静态文件服务

### 共享应用状态

`src/state.rs` 中的 `AppState` 是运行期主状态容器，当前集中持有：

- `AppConfig`
- `SqlitePool`
- `reqwest::Client`
- GitHub / LinuxDO OAuth client
- WebAuthn 配置
- 加密密钥
- LLM scheduler / translation scheduler controller
- 当前运行实例的本地 owner id

如果一个能力既需要配置、数据库又需要跨 handler 共享，通常会进入 `AppState` 或由它派生。

## 后端模块分工

### 入口与基础设施

- `src/config.rs`：环境变量解析与运行配置
- `src/server.rs`：路由、session、CORS、静态资源、数据库初始化
- `src/state.rs`：共享状态、OAuth / WebAuthn 装配
- `src/runtime.rs`：运行时常量与共享运行约束
- `src/error.rs`：统一错误语义
- `src/version.rs`：版本与缓存相关元信息

### 认证与账号边界

- `src/auth.rs`：GitHub 登录、会话与账号相关逻辑
- `src/passkeys.rs`：Passkey 注册、登录与管理
- `src/linuxdo.rs`：LinuxDO 绑定与 onboarding 相关逻辑
- `src/crypto.rs`：token 等敏感字段的本地加密

账号模型的关键约束是：**GitHub connection 仍然是正式账号可用性的基础，Passkey 只是补充登录方式，LinuxDO 是补充绑定来源。**

### 内容同步与派生内容

- `src/github.rs`：GitHub API 访问与上游数据转换
- `src/sync.rs`：同步流程、可见范围与相关派生
- `src/briefs.rs`：日报窗口、brief 内容组织与落库
- `src/translations.rs`：翻译请求、批次、worker runtime 与状态推进
- `src/ai.rs`：LLM 调度与相关运行控制

这些模块共同支撑 Dashboard 的 release、social、brief 和翻译阅读能力。

### API 与管理员运行面

- `src/api.rs`：主要 HTTP handler；产品接口和管理员接口都在这里落地
- `src/jobs.rs`：后台任务与运行事件相关能力
- `src/admin_runtime.rs`：管理员可调运行配置与启动时加载逻辑

如果问题表现为“页面拿到了什么数据”，通常先从 `src/api.rs` 找入口；如果问题表现为“后台为什么这样调度 / 为什么卡住”，再继续查 `jobs.rs`、`translations.rs`、`ai.rs` 或 `admin_runtime.rs`。

## 前端职责边界

### Web 应用

`web/` 负责所有登录后页面与公开登录页的真实交付，包括：

- Landing / bind / settings 等账号入口
- Dashboard 的 `全部 / 发布 / 加星 / 关注 / 日报 / 收件箱`
- Admin Panel / Admin Jobs 等管理员界面

前端当前是 React + Vite 工程，并使用 TanStack Router 组织页面路由。

### Storybook

Storybook 在这个仓库里不是展示橱窗，而是**稳定验证面**。改动可见界面时，优先保证：

- 现有页面状态有稳定 story 可复现
- 新边界状态能在 Storybook 中独立验证
- 视觉或交互回归优先通过 story / e2e 入口收敛，而不是只靠一次性手工页面检查

对应约束与常用命令见 [`../web/README.md`](../web/README.md)。

## 文档边界

- `README.md`：仓库入口、最短启动路径与常用入口
- `docs-site/docs/*.md`：公开站点文档
- `docs/*.md`：内部项目当前真相
- `docs/specs/**`：topic-level 规格、实现状态与历史原因

当一个结论已经稳定到“后续维护者不读 spec 也应该知道”，就应优先提升到 `docs/*.md` 或仓库入口文档，而不是只留在 `docs/specs/**`。

## 排查入口

### 登录 / 绑定 / 会话问题

先看：

- [`product.md`](./product.md) 中的权限模型
- `src/auth.rs`
- `src/passkeys.rs`
- `src/linuxdo.rs`
- `src/server.rs` 中 session / cookie / callback 装配

### Release / feed / social 可见性问题

先看：

- [`product.md`](./product.md) 中的对象与信号语义
- `src/github.rs`
- `src/sync.rs`
- `src/api.rs`

### 日报 / 翻译 / LLM 调度问题

先看：

- [`product.md`](./product.md) 中的日报与翻译约束
- `src/briefs.rs`
- `src/translations.rs`
- `src/ai.rs`
- `src/jobs.rs`
- `src/admin_runtime.rs`

### 管理员页面与运行时观测问题

先看：

- `src/api.rs` 中 `/admin/*` 相关 handler
- `src/admin_runtime.rs`
- `src/jobs.rs`
- `web/src/admin/*`

### UI 结构与视觉回归问题

先看：

- [`../web/README.md`](../web/README.md)
- `web/src/stories/*`
- `web/e2e/*`
- 如需追根到需求语义，再回对应 `docs/specs/**`
