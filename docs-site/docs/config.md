---
title: 配置参考
description: OctoRill 运行时、AI 与文档预览相关配置项。
---

# 配置参考

## 加载顺序

应用启动时先读 `.env.local`，再读 `.env`。个人密钥与本地覆盖值应写在 `.env.local`。

## 核心运行时

- `OCTORILL_BIND_ADDR`：后端监听地址。默认 `127.0.0.1:58090`。
- `OCTORILL_PUBLIC_BASE_URL`：前端和 OAuth 等对外使用的基础 URL。默认根据 `OCTORILL_BIND_ADDR` 推导；本地默认是 `http://127.0.0.1:58090`，模板里通常改成前端地址 `http://127.0.0.1:55174` 便于本地联调。
- `DATABASE_URL`：数据库连接串。默认 `sqlite:./.data/octo-rill.db`。
- `OCTORILL_TASK_LOG_DIR`：后台任务日志目录。默认 `.data/task-logs`。
- `OCTORILL_TASK_WORKERS`：后台任务 worker 数量。默认 `4`，必须是正整数。
- `OCTORILL_ENCRYPTION_KEY_BASE64`：32 字节 base64 密钥。**必填**；用于本地敏感信息加密。
- `APP_DEFAULT_TIME_ZONE`：默认时区。未设置时会优先尝试系统时区，再回退到内置日报时区；必须是整点偏移的 IANA time zone。

## GitHub OAuth

下面三项缺一不可：

- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `GITHUB_OAUTH_REDIRECT_URL`

本地默认 callback 通常是：

```text
http://127.0.0.1:58090/auth/github/callback
```

如果 OAuth App 上登记的 callback 与这里不一致，登录一定会失败。

## LinuxDO 绑定（可选）

下面三项要么同时为空，要么同时填写：

- `LINUXDO_CLIENT_ID`
- `LINUXDO_CLIENT_SECRET`
- `LINUXDO_OAUTH_REDIRECT_URL`

本地默认 callback 通常是：

```text
http://127.0.0.1:58090/auth/linuxdo/callback
```

如果只填了一部分，后端会直接报错并拒绝启动。公网部署时，`LINUXDO_OAUTH_REDIRECT_URL` 必须和 LinuxDO Connect 后台登记的 callback 完全一致。

## AI、翻译与日报

这些配置是可选项；不填写时，核心登录和浏览链路仍可运行，但翻译、要点整理和部分日报能力不可用或降级。

- `AI_API_KEY`：开启 AI 能力的开关；为空时后端不会初始化 AI provider。
- `AI_BASE_URL`：AI provider base URL。默认 `https://api.openai.com/v1/`。
- `AI_MODEL`：模型 ID。默认 `gpt-4o-mini`。
- `AI_MAX_CONCURRENCY`：单进程内同时在途的上游 LLM 请求数。默认 `1`。
- `AI_DAILY_AT_LOCAL`：日报窗口边界，本地时间格式 `HH:MM`。默认 `08:00`。

对 OpenAI-compatible 网关，`AI_MODEL` 必须和 `/v1/models` 返回值一致；大小写通常也要一致。

## 运行时覆盖与管理员设置

某些值在首次启动后可以被管理员页面持久化覆盖：

- LLM 并发上限
- 翻译 worker 数量
- 可选的模型输入长度上限

一旦管理员在任务中心保存这些设置，后续重启会优先使用数据库中的持久化值，而不是初次启动时的 env/default 种子。

## 文档站与 Storybook 相关变量

- `DOCS_PORT`：docs-site 本地 dev/preview 端口。默认 `50885`。
- `DOCS_BASE`：静态站部署基路径。GitHub Pages 项目页通常是 `/<repo>/`。
- `VITE_STORYBOOK_DEV_ORIGIN`：docs-site 本地跳转到 Storybook dev server 时使用的 origin。默认 `http://127.0.0.1:55176`。
- `VITE_DOCS_SITE_ORIGIN`：单独运行 Storybook dev server 时，返回 docs-site 的 origin。默认 `http://127.0.0.1:50885`。

## 前端常用端口

- App dev：`55174`
- App preview：`55175`
- Storybook dev：`55176`

## 兼容性与常见处理

- 旧版 SQLite 文件如果还在使用 NanoID 切换前的主键格式，通常需要重建数据库。
- 如果你修改了本地预览端口，记得同步检查 docs-site、Storybook 和 OAuth callback 的配置是否仍然对齐。
