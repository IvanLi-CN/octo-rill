---
title: 配置参考
description: OctoRill 本地开发与部署时最常用的配置项。
---

# 配置参考

## 环境变量加载顺序

应用启动时会先读取 `.env.local`，再读取 `.env`。本地开发建议把个人 secrets 放在 `.env.local`。

## GitHub OAuth

这些变量是登录链路的基础配置：

- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `GITHUB_OAUTH_REDIRECT_URL`

如果它们未正确配置，Landing 页面只能展示登录入口，但无法完成回调登录。

## AI 翻译与日报（可选）

- `AI_API_KEY`
- `AI_BASE_URL`
- `AI_MODEL`
- `AI_MAX_CONCURRENCY`
- `AI_MODEL_CONTEXT_LIMIT`
- `AI_DAILY_AT_LOCAL`

说明：

- 未配置 AI 相关项时，核心登录与浏览流程仍可运行，但翻译与日报能力不可用或受限。
- `AI_MAX_CONCURRENCY` 控制单进程内同时在途的上游 LLM 请求数；默认 `1`，可按实例容量逐步调高。
- 管理员在任务中心保存新的 LLM / translation runtime 配置后，这些值会持久化到数据库，并在后续重启时覆盖首次启动的 env/default 种子。
- 对 OpenAI-compatible 网关，`AI_MODEL` 通常需要匹配 `/v1/models` 返回的模型 ID。

## 本地数据与兼容性

- SQLite 默认位于 `./.data/`。
- 本地应用主键已切换为 16 字符 NanoID；如果你还在使用旧版本 SQLite 数据文件，通常需要重建数据库。

## 文档与静态站构建变量

- `DOCS_PORT`：本地 docs-site dev/preview 端口，默认 `50885`。
- `DOCS_BASE`：静态站部署基路径；GitHub Pages 项目页场景通常为 `/<repo>/`。
- `VITE_STORYBOOK_DEV_ORIGIN`：可选，本地 docs-site 跳转到 Storybook dev server 时使用的完整 origin；默认 `http://127.0.0.1:55176`。
- `VITE_DOCS_SITE_ORIGIN`：可选，单独运行 Storybook dev server 时，返回 docs-site 的完整 origin；默认 `http://127.0.0.1:50885`。若先从 docs-site 进入 Storybook，该值会自动记住当前 docs-site origin。

## Storybook 与前端端口

- App dev: `55174`
- App preview: `55175`
- Storybook dev: `55176`
