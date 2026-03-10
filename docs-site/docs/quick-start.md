---
title: 快速开始
description: 在本地启动 OctoRill 后端、前端与 Storybook。
---

# 快速开始

## 环境要求

- Rust 工具链（仓库当前使用 `1.91.0`）。
- Bun（前端、Storybook 与 docs-site 都依赖 Bun）。
- SQLite 开发库（CI 使用 `pkg-config` 与 `libsqlite3-dev`）。

## 1. 安装根仓库工具与 hooks

```bash
bun install
```

这一步会安装仓库级 Git tooling，并通过 `prepare` 自动安装共享 hooks。

## 2. 配置环境变量

```bash
cp .env.example .env.local
```

优先填写这些关键项：

- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `GITHUB_OAUTH_REDIRECT_URL`
- `AI_API_KEY`（可选，用于翻译与日报）
- `AI_BASE_URL`（可选）
- `AI_MODEL`（可选）
- `AI_MAX_CONCURRENCY`（可选，默认 `1`）

更多变量说明见 [配置参考](/config)。

## 3. 启动后端

```bash
cargo run
```

默认后端会负责 OAuth callback、API 与本地 SQLite 数据目录初始化。

## 4. 启动前端

```bash
cd web
bun install
bun run dev
```

然后访问 `http://127.0.0.1:55174`。

## 5. 启动 Storybook（可选）

```bash
cd web
bun run storybook
```

然后访问 `http://127.0.0.1:55176`。

## 6. 启动 docs-site（可选）

```bash
cd docs-site
bun install
bun run dev
```

然后访问 `http://127.0.0.1:50885`。
