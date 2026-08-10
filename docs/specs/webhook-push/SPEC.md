# Webhook 推送

## 背景 / 问题陈述

“我的发布”当前通过访问刷新和定时拉取发现用户个人仓库的新 Release。两次拉取之间的新发布无法及时进入共享 Release 缓存，且新增仓库没有主动建立上游通知通道。

本能力允许用户显式授权 OctoRill 使用其已保存的 classic PAT，为该 PAT 所属 GitHub 账号的个人 owner 仓库注册 Release webhook。Webhook 是快速发现信号，现有 Release 同步仍是数据写入路径。

## Goals

- 在“我的发布”中提供默认关闭的“Webhook 推送”子开关。
- 提供“全量注册 Webhook”“全量检查 Webhook”“批量删除 Webhook”以及逐仓注册/检查入口。
- 开启后立即注册，并由管理员配置的后台定时巡查查漏补缺；默认周期 7 天。
- 权限错误按仓库暂停自动巡查，直到人工注册成功。
- 验证 GitHub HMAC 签名并幂等接收新发布 Release 事件，再复用共享 Release 同步队列。

## Non-goals

- 不支持 fine-grained PAT、组织仓库或非 PAT 所属账号的仓库。
- 不接收非 Release 事件，不处理 Release 编辑、撤回或删除。
- 不新增任何名为“巡查”或“立即巡查”的按钮；巡查仅指后台定时任务。
- 不删除或修改非 OctoRill 管理的 GitHub webhook。

## Interfaces & Contracts

- HTTP API：[`contracts/http-apis.md`](./contracts/http-apis.md)
- DB：[`contracts/db.md`](./contracts/db.md)

## Functional Contract

### 开关与前置条件

- `users.webhook_push_enabled` 默认 `0`。
- “Webhook 推送”依赖 `include_own_releases=1`。关闭“我的发布”时必须同时关闭“Webhook 推送”，但不得隐式删除 GitHub hooks。
- 开启前必须确认：
  - 已保存 PAT 且最近校验有效；
  - PAT owner 是当前用户已绑定的 GitHub 账号；
  - classic PAT scope 包含 `public_repo` 或 `repo`；
  - `OCTORILL_PUBLIC_BASE_URL` 是 GitHub 可访问的 HTTPS 地址。
- 开启必须经过二次确认。确认内容必须说明权限用途、仅监听新发布 Release、secret 加密保存、关闭后 hooks 仍保留。
- 开启成功后立即排队一次全量注册。单仓失败不回滚开关。

### 仓库范围

- 目标仓库来自 PAT owner 对应 GitHub connection 刷新的 `owned_repo_star_baselines`。
- 仅处理 `owner_login` 与 PAT owner login 一致的个人 owner 仓库。
- `public_repo` PAT 对私有仓库的权限失败按仓库暂停；`repo` PAT 可覆盖公开与私有仓库。

### 注册、检查和删除

- 注册先列出仓库 hooks，按 callback URL 与 `release` event 识别 OctoRill hook。
- 没有匹配项时创建；恰好一个匹配项时确保 active、JSON content type、Release event 与当前 secret；多个匹配项标记冲突，不自动删除。
- 全量注册包含 `permission_paused` 仓库；成功后解除该仓库暂停。
- 检查只读核对 hook，不创建、更新或解除暂停。
- 批量删除仅在“Webhook 推送”关闭时允许，只删除数据库记录了 hook ID 的 OctoRill hooks。
- 关闭后接收端忽略事件。删除失败保留 hook 记录和错误，允许再次删除。

### 定时巡查

- 管理员配置 `webhook_push_audit_interval_days`，合法范围 `1..=30`，默认 `7`。
- 定时巡查仅处理 `include_own_releases=1 AND webhook_push_enabled=1` 的用户。
- 每轮刷新 PAT owner 的个人仓库基线，检查并注册缺失 hooks。
- `permission_paused` 仓库必须跳过；401、403、仍存在基线时的 404，以及 GitHub 明确返回的权限错误进入该状态。
- 网络、限流和 GitHub 5xx 为暂时错误，不进入权限暂停。

### Webhook 接收

- 接收端必须验证 `X-Hub-Signature-256`，拒绝无签名或签名错误请求。
- `X-GitHub-Delivery` 全局去重；`ping` 安全返回成功。
- 只处理 `X-GitHub-Event: release` 且 `action=published`、`release.draft=false` 的 payload。
- 有效事件通过 repo ID 挂入现有共享 Release 队列；HTTP 请求不得等待 GitHub Release 拉取完成。
- 用户或子开关关闭、hook 记录不存在、repo 不匹配、其他 action 均返回接受但不入队。

## UI Contract

- “Webhook 推送”位于 `/settings?section=my-releases` 现有卡片内，使用独立 Switch。
- 卡片必须展示启用状态、PAT owner、已注册/缺失/权限暂停/可删除数量、最近与下次定时巡查。
- 固定全量按钮名称：
  - `全量注册 Webhook`
  - `全量检查 Webhook`
  - `批量删除 Webhook`
- 仓库行提供 `注册 Webhook` / `重新注册 Webhook` 和 `检查 Webhook`。
- 页面不得出现可点击的“巡查”或“立即巡查”。
- 权限错误必须给出 repo、失败原因与 classic PAT 修复指引；无 PAT 时链接到同一设置页的 GitHub PAT section。

## Acceptance Criteria

- 默认关闭且不创建外部 hook；不满足前置条件时不能开启。
- 开启确认后立即异步注册，部分失败仍保持开启并逐仓展示。
- 定时巡查跳过权限暂停仓库；人工注册成功恢复该仓库自动巡查资格。
- 全量检查没有外部写入；批量删除只删除 OctoRill hook。
- 重复 delivery 只产生一次 Release demand；非 published action 不产生 demand。
- 设置页桌面、移动端以及深色/浅色主题无溢出，所有按钮和 Switch 有可访问名称与忙碌状态。

## Visual Evidence

实施完成后补充 Storybook 或 mock-only UI 的桌面与移动端证据。

