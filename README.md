<p align="center">
  <img src="./brand/exports/wordmark-light.svg#gh-light-mode-only" alt="OctoRill" width="420" />
  <img src="./brand/exports/wordmark-dark.svg#gh-dark-mode-only" alt="OctoRill" width="420" />
</p>

# OctoRill

OctoRill 是一个面向个人 GitHub 动态的聚合与阅读界面：在同一工作区集中展示发布更新、个人仓库获星、账号被关注等动态；发布内容支持中文翻译与要点整理，并提供日报与 Inbox 快捷入口。

更多产品与交互说明见：`docs/product.md`。

## Tech stack

- Backend: Rust (axum) + SQLite (sqlx)
- Frontend: React (Vite) + Bun

## Dev

### 0) 安装仓库级开发工具与 Git hooks

```bash
bun install
```

- 请优先在主工作区根目录执行 `bun install`；它会安装本仓库的 Git tooling，并通过 `prepare` 自动把 hooks 装到共享 `.git/hooks`。
- 当前这套 repo-local hooks / worktree bootstrap 安装入口以 POSIX shell 为前提，仅承诺 `macOS/Linux`；原生 `PowerShell-only Windows` 不在支持范围。
- 若你在 linked worktree 内执行 `bun install`，安装脚本会尝试复用主工作区解析出的 hook 安装入口与 `lefthook` 二进制；若主工作区尚未完成初始化，会提示你先回主工作区执行一次。
- hooks 安装后会优先使用仓库内解析出的 `lefthook` 二进制，避免被全局 `lefthook` 抢占；若之前固定的二进制已不存在，hook 会自动回退到当前环境里的可用 `lefthook`。
- 安装脚本会把主工作区根目录记录到共享 Git 配置里，因此 `git clone --separate-git-dir=...` 这类布局下，新 linked worktree 依然能回源复制本地资源。
- 安装脚本会把 repo-local `core.hooksPath` 收敛到共享 hook 目录；若仓库原本已有自定义 hook 目录，或默认 `.git/hooks` 下已有自定义 hook，会把既有 hook 链接进共享目录，避免 `bun install` 后把原有 hook 链静默关掉。
- 当 `lefthook.yml`、`package.json` 或根目录依赖发生变化时，重新执行 `bun install` 或 `bun run hooks:install`；若变更发生在 linked worktree 内，安装脚本会按当前 worktree 的 `lefthook.yml` 生成共享 wrappers，并优先固定到当前 worktree 已安装的 repo-local `lefthook`，再回退到主工作区二进制。

### 1) 配置环境变量

推荐复制 `.env.example` 到 `.env.local` 并填写本地 secrets：

```bash
cp .env.example .env.local
```

- 应用启动顺序是先读取 `.env.local`，再读取 `.env`；`.env.local` 更适合每位开发者自己的配置。
- 安装 hooks 后，新的 linked worktree 在首次 checkout 时会自动补齐清单中缺失的资源（当前默认包含 `.env.local` 与 `.env`）。
- 若要扩展自动同步的资源，请编辑 `scripts/worktree-sync.paths`，只添加适合“缺失时复制”的本地文件或目录。
- 非常规 Git 布局可通过 `git config --local codex.worktree-sync.source-root <path>` 指定源目录；`<path>` 支持绝对路径和相对仓库根目录的写法。

关键配置项：

- GitHub OAuth：
  - `GITHUB_CLIENT_ID`
  - `GITHUB_CLIENT_SECRET`
  - `GITHUB_OAUTH_REDIRECT_URL`
- LinuxDO Connect（可选；用于账号绑定）：
  - `LINUXDO_CLIENT_ID`
  - `LINUXDO_CLIENT_SECRET`
  - `LINUXDO_OAUTH_REDIRECT_URL`
- AI（可选；用于翻译与日报）：
  - `AI_API_KEY`
  - `AI_BASE_URL`
  - `AI_MODEL`
  - `AI_MAX_CONCURRENCY`（可选，单进程 LLM 最大并行数，默认 `1`）
  - `AI_DAILY_AT_LOCAL`（例如 `08:00`，用于“昨日更新”窗口边界；不配置时默认 `08:00`）

### LinuxDO Connect（可选，自托管部署）

如果你的自托管实例需要启用 LinuxDO 账号绑定，请按下面的部署顺序完成；这是一套完整的自托管落地方案，不只是环境变量清单。

#### 1) 确定公开访问地址

- 假设你的 OctoRill 对外地址是 `https://octorill.example.com`
- LinuxDO OAuth 回调地址就固定为：

```text
https://octorill.example.com/auth/linuxdo/callback
```

- 这里必须使用 **LinuxDO Connect 能访问到的公开地址**
- 不要填写 `127.0.0.1`、容器内地址、内网 IP、宿主机私网端口

#### 2) 确认反向代理会把 callback 打到后端

如果你前面有 Nginx / Caddy / Traefik，至少要保证：

- `/auth/linuxdo/callback` 会被代理到 OctoRill backend
- 用户最终访问的协议、域名、端口，与 LinuxDO Connect 后台登记的 callback 完全一致

最常见的错误就是：

- 页面用的是 `https://octorill.example.com`
- 但回调地址登记成了 `http://127.0.0.1:58090/auth/linuxdo/callback`

这样授权一定回不来。

#### 3) 在 LinuxDO Connect 后台创建应用

- 打开 LinuxDO Connect 后台，申请新接入 / 编辑已有应用
- 把回调地址填写成上面的完整 callback URL
- 保存后拿到 `Client Id` 和 `Client Secret`

LinuxDO Wiki 官方说明见：

- [Linux DO Connect](https://wiki.linux.do/Community/LinuxDoConnect)

#### 4) 在 OctoRill 服务端同时设置这三项环境变量

缺一不可：

```bash
LINUXDO_CLIENT_ID=<linuxdo-client-id>
LINUXDO_CLIENT_SECRET=<linuxdo-client-secret>
LINUXDO_OAUTH_REDIRECT_URL=https://octorill.example.com/auth/linuxdo/callback
```

规则是：

- 三项都为空：LinuxDO 绑定保持关闭
- 三项都存在：LinuxDO 绑定启用
- 只填一部分：OctoRill 启动失败

#### 5) 常见部署写法

如果你用 `docker compose`，可以直接这样写：

```yaml
services:
  octorill:
    image: <your-octorill-image>
    env_file:
      - .env.local
    environment:
      LINUXDO_CLIENT_ID: ${LINUXDO_CLIENT_ID}
      LINUXDO_CLIENT_SECRET: ${LINUXDO_CLIENT_SECRET}
      LINUXDO_OAUTH_REDIRECT_URL: ${LINUXDO_OAUTH_REDIRECT_URL}
```

如果你用 `systemd`，可以把三项写进 environment file，例如：

```ini
LINUXDO_CLIENT_ID=your-linuxdo-client-id
LINUXDO_CLIENT_SECRET=your-linuxdo-client-secret
LINUXDO_OAUTH_REDIRECT_URL=https://octorill.example.com/auth/linuxdo/callback
```

然后重启 OctoRill 后端进程，使配置生效。

#### 6) 部署后验证

以已登录用户身份打开 `/settings?section=linuxdo`，逐项检查：

- 页面不再显示“暂未启用 LinuxDO 绑定”
- 点击“连接 LinuxDO”会跳转到 LinuxDO Connect 授权页
- 授权完成后会回到 `/settings?section=linuxdo`
- 设置页显示绑定后的 LinuxDO 快照

如果你点“连接 LinuxDO”后根本没有跳到授权页，通常是服务端没配全。

#### 7) 停用 / 回滚

停用 LinuxDO 绑定时，请 **同时移除这三项环境变量**；只保留其中一部分会导致服务启动失败。

#### 8) 自托管最常见的错误

- `LINUXDO_OAUTH_REDIRECT_URL` 与 LinuxDO Connect 后台登记的 callback URL 不完全一致
- 公网部署仍然把 callback 写成 `127.0.0.1`、内网地址或错误端口
- 反向代理没有把 `/auth/linuxdo/callback` 转发到后端
- 把 `LINUXDO_CLIENT_SECRET` 暴露到前端或提交进仓库

#### 9) OctoRill 实际会保存什么

OctoRill 只会持久化 LinuxDO 绑定快照，不会保存 LinuxDO access token、refresh token 或 `api_key`。

更细的部署口径见：`docs/specs/y9ngx-linuxdo-user-settings/contracts/deployment.md`。

### 2) 启动后端

```bash
cargo run
```

### 3) 启动前端

```bash
cd web
bun install
bun run dev
```

Then open `http://127.0.0.1:55174`.

### 4) 启动 Storybook（可选）

```bash
cd web
bun run storybook
```

Then open `http://127.0.0.1:55176`.

### 5) 启动 docs-site（可选）

```bash
cd docs-site
bun install
bun run dev
```

Then open `http://127.0.0.1:50885`.

`docs-site` 单独运行时只承载文档壳层；如果同机已经执行 `cd web && bun run storybook`，文档里的 Storybook 入口会自动跳到本地 Storybook dev server。若你改过本地端口，可额外设置 `VITE_STORYBOOK_DEV_ORIGIN` 或 `VITE_DOCS_SITE_ORIGIN`。若要验证发布后的 `/storybook/` 子路径，请先构建 docs-site 与 Storybook，再执行 `.github/scripts/assemble-pages-site.sh` 进行装配预览。

## Auth model

- OAuth（默认登录通道）：仅用于登录、读取与同步（Feed / Notifications / Starred / Releases / Followers / owned repo stargazers）。
- OAuth scope 策略：采用最小授权，默认不为站内反馈申请额外写权限。
- Release 反馈（👍 😄 ❤️ 🎉 🚀 👀）写操作：要求用户额外提供 GitHub PAT（Personal Access Token）。
  - Fine-grained PAT：按 GitHub Reactions 文档可不额外申请 repository permissions，但 token 仍需覆盖目标仓库。
  - Classic PAT：公共仓库建议 `public_repo`，私有仓库需 `repo`。

## Docs & Storybook

- Public docs site source lives in `docs-site/` and is built with Rspress.
- Storybook stays under `web/` and is published as a nested static site at `/storybook/`; the primary docs navigation goes through `/storybook.html` and immediately redirects to `/storybook/index.html`; the curated hub page lives at `/storybook-guide.html`.
- `Docs Pages` GitHub Actions workflow builds the docs site and Storybook separately, then assembles them into one GitHub Pages artifact.
- For project Pages deployments, `DOCS_BASE` must be set to `/<repo>/`; the workflow computes this automatically.
- `CI Pipeline` also runs docs-site build plus the assembled-site smoke check, so base-path regressions can fail in PR before Pages deploy.

## Notes

- OAuth callback is handled by the backend (`/auth/github/callback`).
- Local data (SQLite) lives under `./.data/`.
- Local application primary keys now use 16-character NanoIDs; older SQLite files created before the NanoID cutover are not compatible and should be rebuilt (for the default path, remove `./.data/octo-rill.db` before restarting).
- For OpenAI-compatible gateways, `AI_MODEL` usually needs to match an ID from `/v1/models` (often case-sensitive).
- 模型输入上限会按内置目录解析，并固定每天同步外部目录（OpenRouter + LiteLLM）；如需手动覆盖，请在管理员任务中心 `LLM 调度` 页签的“配置 LLM 运行参数”里保存 `LLM 输入长度上限（tokens）`。
- LLM 调度默认只允许单进程内 `1` 个上游请求并行；如需提速，可通过 `AI_MAX_CONCURRENCY` 提高 permit 并发上限。
- 管理员任务中心支持在线调整 LLM 并发上限，以及翻译 worker 数量 / 可选的模型输入上限；首次启动时这些值以 env/default 或空值为种子，管理员保存后会持久化到数据库，并在后续重启时继续生效。
- Release 数据按“共享事实语义”处理：取消 Star 只影响当前用户列表可见性，不影响历史日报里的 release 详情访问与详情翻译。
- 日报落库前会做 `release_id` 内链完整性校验与补齐，按查询参数做精确匹配（避免 `12/123` 前缀误判）。

## Release automation (PR label driven)

Releases are decided by PR labels and executed only after `CI Pipeline` succeeds on `main`.

### Required PR labels

Every PR must contain exactly one `type:*` label and one `channel:*` label:

- `type:*`: `type:docs`, `type:skip`, `type:patch`, `type:minor`, `type:major`
- `channel:*`: `channel:stable`, `channel:rc`

### Decision matrix

| Type label | Channel label | Release result |
| --- | --- | --- |
| `type:docs` / `type:skip` | `channel:stable` / `channel:rc` | No release |
| `type:patch` / `type:minor` / `type:major` | `channel:stable` | Stable release (`vX.Y.Z`) |
| `type:patch` / `type:minor` / `type:major` | `channel:rc` | Prerelease (`vX.Y.Z-rc.<sha7>`) |

### Image tags

- Stable release: publish `${image}:vX.Y.Z` and `${image}:latest`
- RC release: publish `${image}:vX.Y.Z-rc.<sha7>` only (no `latest`)

### Troubleshooting

- `PR Label Gate` fails:
  - Missing or conflicting `type:*` / `channel:*` labels
  - Unknown labels under `type:*` or `channel:*`
- `Release` workflow skips:
  - Commit cannot be mapped to exactly one PR
  - GitHub API lookup failure for PR mapping/labels
- `Release` workflow fails:
  - Invalid label combination detected by `.github/scripts/release-intent.sh`
