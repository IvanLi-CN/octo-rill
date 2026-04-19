<p align="center">
  <img src="./brand/exports/wordmark-light.svg#gh-light-mode-only" alt="OctoRill" width="420" />
  <img src="./brand/exports/wordmark-dark.svg#gh-dark-mode-only" alt="OctoRill" width="420" />
</p>

# OctoRill

OctoRill 把与“我”相关的 GitHub 动态整理成一个更适合持续阅读的工作区：集中查看发布更新、获星与关注动态、日报，以及 Inbox 入口；发布内容支持中文翻译与要点整理。

## 先去哪里看

- 公共文档站：[`ivanli-cn.github.io/octo-rill`](https://ivanli-cn.github.io/octo-rill/)
- 本地启动步骤：[`docs-site/docs/quick-start.md`](./docs-site/docs/quick-start.md)
- 配置参考：[`docs-site/docs/config.md`](./docs-site/docs/config.md)
- 公开产品说明：[`docs-site/docs/product.md`](./docs-site/docs/product.md)
- 内部产品参考：[`docs/product.md`](./docs/product.md)
- 前端与 Storybook 贡献说明：[`web/README.md`](./web/README.md)

## 仓库结构

- `src/`：Rust 后端，负责 OAuth、同步、翻译、日报、通知与管理员任务接口。
- `web/`：React + Vite 前端，以及 Storybook。
- `docs-site/`：Rspress 文档站；发布时与 Storybook 组装到同一个 GitHub Pages 站点。
- `docs/specs/`：工作项规格与追踪台账，不是公开文档入口。
- `migrations/`：SQLite schema 迁移。
- `brand/`：品牌资源与导出文件。

## 本地最短启动路径

1. 在仓库根目录安装仓库级工具与 hooks：

   ```bash
   bun install
   ```

2. 复制环境变量模板并填写最少必需项：

   ```bash
   cp .env.example .env.local
   ```

   至少补齐：`OCTORILL_ENCRYPTION_KEY_BASE64`、`GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET`、`GITHUB_OAUTH_REDIRECT_URL`。如果这次不测试 LinuxDO，记得把 `LINUXDO_CLIENT_ID`、`LINUXDO_CLIENT_SECRET`、`LINUXDO_OAUTH_REDIRECT_URL` 三项都清空；如果要测试 LinuxDO 绑定，则三项必须同时出现。

3. 启动后端：

   ```bash
   cargo run
   ```

   默认监听 `http://127.0.0.1:58090`。

4. 启动前端：

   ```bash
   cd web
   bun install
   bun run dev
   ```

   打开 `http://127.0.0.1:55174`。首次成功时，你会先看到 GitHub 登录页；已有会话时会直接进入 Dashboard。

可选本地入口：

- Storybook：`cd web && bun run storybook` → `http://127.0.0.1:55176`
- 文档站：`cd docs-site && bun install && bun run dev` → `http://127.0.0.1:50885`

## 常用命令

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --locked --all-targets --all-features
(cd web && bun run lint)
(cd web && bun run storybook:build)
(cd docs-site && bun run build)
```

## 开发流程提醒

- 变更代码时同步更新文档；不要让 README、docs-site 与行为实现分叉。
- 合到 `main` 的 PR 需要且只能有一个 `type:*` 标签和一个 `channel:*` 标签；文档改动通常使用 `type:docs`。
- LinuxDO 绑定、自托管 callback 与 AI 配置的细项统一写在 [`docs-site/docs/config.md`](./docs-site/docs/config.md)，不要再把 README 扩写成部署手册。
- 如果改动影响页面或 Storybook 文档入口，先验证 docs-site build、Storybook build 与 assembled Pages smoke check。
