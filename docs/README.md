# OctoRill 内部文档入口

这组文档面向仓库维护者与协作者，回答“项目现在是什么、怎么运行、从哪里继续维护”。公开说明仍以 `docs-site/` 为准；`docs/specs/` 保留 topic-level 规格、实现状态与历史脉络，不承担日常入口职责。

## 从哪里开始

- **先理解项目现在做什么**：看 [`product.md`](./product.md)
- **先理解系统怎么拼起来**：看 [`architecture.md`](./architecture.md)
- **先在本地跑起来**：看 [`../docs-site/docs/quick-start.md`](../docs-site/docs/quick-start.md)
- **先核对运行配置**：看 [`../docs-site/docs/config.md`](../docs-site/docs/config.md)
- **先做前端 / Storybook 改动**：看 [`../web/README.md`](../web/README.md)
- **要追某个主题的规格、实现与演进原因**：看 [`specs/README.md`](./specs/README.md)

## 文档分层

### 仓库入口

- [`../README.md`](../README.md)：仓库级概览、最短启动路径、最常用入口
- `docs/README.md`：内部文档导航与维护入口

### 公开文档

- `docs-site/docs/*.md`：面向公开站点的启动、配置与产品说明
- 适合回答“外部用户或新同事应该先看什么”

### 内部项目文档

- [`product.md`](./product.md)：产品语义、界面职责、权限与数据边界
- [`architecture.md`](./architecture.md)：系统组成、运行边界、模块职责与排查入口
- 适合回答“项目现在怎么工作、从哪里改、谁是事实源”

### Agent memory / 长期工程记忆

- `docs/specs/<id>-<topic>/SPEC.md`：topic-level 规格与验收语义
- `docs/specs/<id>-<topic>/IMPLEMENTATION.md`：当前实现状态、验证与落地范围
- `docs/specs/<id>-<topic>/HISTORY.md`：关键演进原因与后继关系
- `docs/solutions/**`：可复用的工程经验与排障结论（存在时）

## 按问题找真相

- **产品到底承诺了什么行为**：先看 [`product.md`](./product.md)，再回对应 spec
- **接口 / 会话 / 静态资源由谁接住**：先看 [`architecture.md`](./architecture.md)，再看 `src/server.rs` / `src/api.rs`
- **OAuth / Passkey / LinuxDO 登录链路在哪里**：看 [`architecture.md`](./architecture.md) 的认证边界，再看 `src/auth.rs`、`src/passkeys.rs`、`src/linuxdo.rs`
- **日报 / 翻译 / 后台任务为什么这样运作**：先看 [`product.md`](./product.md) 与 [`architecture.md`](./architecture.md)，再看相关 spec 与 `src/briefs.rs`、`src/translations.rs`、`src/ai.rs`、`src/jobs.rs`
- **UI 改动应该从哪里验证**：先看 [`../web/README.md`](../web/README.md)，优先用 Storybook 和现有 e2e / story 入口

## 维护约定

- 公开口径改动，优先同步 `docs-site/` 与仓库入口 `README.md`
- 稳定的项目当前真相，优先回写 `docs/*.md`
- 主题级约束、实现轨迹与历史原因，继续保留在 `docs/specs/**`
- 不要把 task log、PR 对话或临时排查过程直接堆进项目文档
