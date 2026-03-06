# OctoRill

OctoRill 是一个 GitHub 信息聚合与阅读界面：把 Releases 整理成类似 GitHub dashboard 的信息流，并用 AI 自动翻译成用户语言（当前默认中文）；同时提供 Release 日报与 Inbox 快捷入口。

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
- 若你在 linked worktree 内执行 `bun install`，安装脚本会尝试复用主工作区的 `node_modules`；若主工作区尚未完成初始化，会提示你先回主工作区执行一次。
- 当 `lefthook.yml`、`package.json` 或根目录依赖发生变化时，重新执行 `bun install` 或 `bun run hooks:install`。

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
- AI（可选；用于翻译与日报）：
  - `AI_API_KEY`
  - `AI_BASE_URL`
  - `AI_MODEL`
  - `AI_MODEL_CONTEXT_LIMIT`（可选，手动覆盖模型输入上限）
  - `AI_DAILY_AT_LOCAL`（例如 `08:00`，用于“昨日更新”窗口边界；不配置时默认 `08:00`）

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

## Auth model

- OAuth（默认登录通道）：仅用于登录、读取与同步（Feed / Notifications / Starred / Releases）。
- OAuth scope 策略：采用最小授权，默认不为站内反馈申请额外写权限。
- Release 反馈（👍 😄 ❤️ 🎉 🚀 👀）写操作：要求用户额外提供 GitHub PAT（Personal Access Token）。
  - Fine-grained PAT：按 GitHub Reactions 文档可不额外申请 repository permissions，但 token 仍需覆盖目标仓库。
  - Classic PAT：公共仓库建议 `public_repo`，私有仓库需 `repo`。

## Notes

- OAuth callback is handled by the backend (`/auth/github/callback`).
- Local data (SQLite) lives under `./.data/`.
- For OpenAI-compatible gateways, `AI_MODEL` usually needs to match an ID from `/v1/models` (often case-sensitive).
- 模型输入上限会按内置目录解析，并固定每天同步外部目录（OpenRouter + LiteLLM）；若设置 `AI_MODEL_CONTEXT_LIMIT`，会以手动值优先。
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
