<p align="center">
  <img src="./brand/exports/wordmark-light.svg#gh-light-mode-only" alt="OctoRill" width="420" />
  <img src="./brand/exports/wordmark-dark.svg#gh-dark-mode-only" alt="OctoRill" width="420" />
</p>

<p align="center">
  <a href="./README.md">English</a> · <a href="./README.zh-CN.md">简体中文</a>
</p>

# OctoRill

<p align="center">
  <a href="https://github.com/IvanLi-CN/octo-rill/releases"><img src="https://img.shields.io/github/v/release/IvanLi-CN/octo-rill?display_name=tag&label=%E5%8F%91%E5%B8%83&style=flat-square" alt="最新发布" /></a>
  <a href="https://github.com/IvanLi-CN/octo-rill/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/IvanLi-CN/octo-rill/ci.yml?branch=main&label=CI&style=flat-square" alt="持续集成状态" /></a>
  <a href="https://ivanli-cn.github.io/octo-rill/"><img src="https://img.shields.io/badge/%E6%96%87%E6%A1%A3-Pages-0f172a?style=flat-square" alt="公开文档站" /></a>
  <a href="https://ivanli-cn.github.io/octo-rill/demo/"><img src="https://img.shields.io/badge/Web%20Demo-mock--only-0f766e?style=flat-square" alt="仅 mock 的 Web 演示" /></a>
  <a href="https://ivanli-cn.github.io/octo-rill/storybook.html"><img src="https://img.shields.io/badge/Storybook-%E9%9D%99%E6%80%81%E7%AB%99-b45309?style=flat-square" alt="静态 Storybook" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/%E8%AE%B8%E5%8F%AF-MIT-22c55e?style=flat-square" alt="MIT 许可" /></a>
</p>

OctoRill 是一个面向 GitHub 动态的个人阅读工作区。它把发布更新、与我直接相关的社交信号、日报，以及收件箱入口放到同一个界面里。它不是一个完整的 GitHub 客户端。

<p align="center">
  <img src="./brand/exports/octo-rill-github-social-preview.png#gh-light-mode-only" alt="OctoRill 主界面预览" width="100%" />
  <img src="./brand/exports/octo-rill-github-social-preview-dark.png#gh-dark-mode-only" alt="OctoRill 主界面预览" width="100%" />
</p>

## 它做什么

- 用 `原文`、`翻译`、`润色` 三种方式阅读发布内容。
- 展示与我直接相关的社交信号：谁给我的个人仓库加星，谁关注了我。
- 按用户本地自然日生成日报。
- 暂停连续 30 天未活动的账号，并保留自助查看状态与恢复流程。
- 提供主工作区、范围收窄页面、管理页面，以及仅使用 mock 数据的 Web 演示页。

## 仓库结构

- `src/`：Rust 后端，负责认证、同步、翻译、日报、管理员 API 和静态资源托管。
- `web/`：React + Vite 前端、Storybook、Playwright 测试，以及 `/demo/` 演示页。
- `docs-site/`：Rspress 公共文档站。
- `docs/`：内部项目文档、架构说明和 specs。
- `migrations/`：SQLite schema migrations。
- `brand/`：品牌源文件和导出资产。

## 快速开始

环境要求：

- Rust `1.91.0`
- Bun `1.x`
- SQLite 开发库
- 一个可用的 GitHub OAuth 应用

安装仓库级工具：

```bash
bun install
```

创建本地环境文件：

```bash
cp .env.example .env.local
```

必填变量：

- `OCTORILL_ENCRYPTION_KEY_BASE64`
- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `GITHUB_OAUTH_REDIRECT_URL`

如果这次不测试 LinuxDO，这三项要一起留空：

- `LINUXDO_CLIENT_ID`
- `LINUXDO_CLIENT_SECRET`
- `LINUXDO_OAUTH_REDIRECT_URL`

如果要测试翻译和日报，还需要：

- `AI_API_KEY`
- `AI_BASE_URL`
- `AI_MODEL`

启动后端：

```bash
cargo run
```

默认监听 `http://127.0.0.1:58090`。

启动前端：

```bash
cd web
bun install
bun run dev
```

打开 `http://127.0.0.1:55174`。

可选本地入口：

- Storybook：`cd web && bun run storybook`
- Web 演示构建：`cd web && bun run build:demo`
- 文档站：`cd docs-site && bun install && bun run dev`

## 常用命令

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --locked --all-targets --all-features
(cd web && bun run lint)
(cd web && bun run storybook:build)
(cd docs-site && bun run build)
```

## 更多文档

- 公共文档站：[ivanli-cn.github.io/octo-rill](https://ivanli-cn.github.io/octo-rill/)
- 快速开始：[docs-site/docs/quick-start.md](./docs-site/docs/quick-start.md)
- 配置说明：[docs-site/docs/config.md](./docs-site/docs/config.md)
- 产品说明：[docs-site/docs/product.md](./docs-site/docs/product.md)
- 内部文档入口：[docs/README.md](./docs/README.md)
- 系统架构：[docs/architecture.md](./docs/architecture.md)
- 前端说明：[web/README.md](./web/README.md)

## 许可

[MIT](./LICENSE)
