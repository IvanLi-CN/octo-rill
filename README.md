# OctoRill

OctoRill 是一个 GitHub 信息聚合与阅读界面：把 Releases 整理成类似 GitHub dashboard 的信息流，并用 AI 自动翻译成用户语言（当前默认中文）；同时提供 Release 日报与 Inbox 快捷入口。所有可操作入口最终都会跳转回 GitHub 完成操作。

更多产品与交互说明见：`docs/product.md`。

## Tech stack

- Backend: Rust (axum) + SQLite (sqlx)
- Frontend: React (Vite) + Bun

## Dev

### 1) 配置环境变量

Copy `.env.example` to `.env` and fill values.

关键配置项：

- GitHub OAuth：
  - `GITHUB_CLIENT_ID`
  - `GITHUB_CLIENT_SECRET`
  - `GITHUB_OAUTH_REDIRECT_URL`
- AI（可选；用于翻译与日报）：
  - `AI_API_KEY`
  - `AI_BASE_URL`
  - `AI_MODEL`
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

## Notes

- OAuth callback is handled by the backend (`/auth/github/callback`).
- Local data (SQLite) lives under `./.data/`.
- For OpenAI-compatible gateways, `AI_MODEL` usually needs to match an ID from `/v1/models` (often case-sensitive).

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
