---
title: 快速开始
description: 用一条最短路径在本地启动 OctoRill。
---

# 快速开始

这份快速开始只覆盖一条最短可用路径：启动后端、启动前端，然后进入登录页或 Dashboard。可选工具（Storybook、docs-site）放在文末。

## 准备环境

- Rust 工具链 `1.91.0`（CI 与仓库约定版本）
- Bun `1.x`
- SQLite 开发库（CI 使用 `pkg-config` 与 `libsqlite3-dev`）
- 一个可用的 GitHub OAuth App

## 1. 安装仓库级工具与 hooks

在仓库根目录运行：

```bash
bun install
```

这一步会安装仓库级开发工具，并通过 `prepare` 自动写入共享 Git hooks。

## 2. 复制环境变量模板

```bash
cp .env.example .env.local
```

先补齐这些必需项：

- `OCTORILL_ENCRYPTION_KEY_BASE64`
- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `GITHUB_OAUTH_REDIRECT_URL`

如果这次不测试 LinuxDO 绑定，先把下面三项都留空。`.env.example` 默认给了 callback 示例值，只保留其中一项会让后端拒绝启动：

- `LINUXDO_CLIENT_ID`
- `LINUXDO_CLIENT_SECRET`
- `LINUXDO_OAUTH_REDIRECT_URL`

如果你还要联调 LinuxDO 绑定，再额外补齐：

- `LINUXDO_CLIENT_ID`
- `LINUXDO_CLIENT_SECRET`
- `LINUXDO_OAUTH_REDIRECT_URL`

如果你要测试翻译与日报，再补：

- `AI_API_KEY`
- `AI_BASE_URL`
- `AI_MODEL`

变量解释见 [配置参考](/config)。

## 3. 启动后端

```bash
cargo run
```

默认情况下：

- 后端监听 `127.0.0.1:58090`
- OAuth callback 由后端处理
- SQLite 默认写到 `./.data/octo-rill.db`

如果这里启动失败，先检查 `.env.local` 是否缺少加密密钥或 OAuth 配置。若你要联调 Passkey，请再确认 `OCTORILL_PUBLIC_BASE_URL` 指向浏览器实际打开的 origin，并使用 HTTPS 或 loopback (`localhost` / `127.0.0.1`)。

## 4. 启动前端

```bash
cd web
bun install
bun run dev
```

打开 `http://127.0.0.1:55174`。

你应该看到：

- 未登录：Landing 登录页，包含 GitHub、LinuxDO 与 Passkey 入口
- 已登录：Dashboard

## 5. 完成一次最小验证

推荐至少确认下面三件事：

- 登录页可以正常打开
- GitHub OAuth 跳转地址和本地配置一致
- 登录后能进入 Dashboard，而不是停在空白页或 callback 错误页

## 可选本地入口

### Storybook

```bash
cd web
bun run storybook
```

打开 `http://127.0.0.1:55176`。

适合在改 UI、补 stories、核对边界状态时使用。

### 文档站

```bash
cd docs-site
bun install
bun run dev
```

打开 `http://127.0.0.1:50885`。

这是公开文档站的本地预览；如果你已经同时启动了 Storybook，本地文档里的 Storybook 链接会自动跳到本机 Storybook 服务。
