<p align="center">
  <img src="./brand/exports/wordmark-light.svg#gh-light-mode-only" alt="OctoRill" width="420" />
  <img src="./brand/exports/wordmark-dark.svg#gh-dark-mode-only" alt="OctoRill" width="420" />
</p>

<p align="center">
  <a href="./README.md">English</a> · <a href="./README.zh-CN.md">简体中文</a>
</p>

# OctoRill

<p align="center">
  <a href="https://github.com/IvanLi-CN/octo-rill/releases"><img src="https://img.shields.io/github/v/release/IvanLi-CN/octo-rill?display_name=tag&label=release&style=flat-square" alt="Latest release" /></a>
  <a href="https://github.com/IvanLi-CN/octo-rill/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/IvanLi-CN/octo-rill/ci.yml?branch=main&label=ci&style=flat-square" alt="CI status" /></a>
  <a href="https://ivanli-cn.github.io/octo-rill/"><img src="https://img.shields.io/badge/docs-pages-0f172a?style=flat-square" alt="Public docs" /></a>
  <a href="https://ivanli-cn.github.io/octo-rill/demo/"><img src="https://img.shields.io/badge/web%20demo-mock--only-0f766e?style=flat-square" alt="Mock-only web demo" /></a>
  <a href="https://ivanli-cn.github.io/octo-rill/storybook.html"><img src="https://img.shields.io/badge/storybook-static-b45309?style=flat-square" alt="Static Storybook" /></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-22c55e?style=flat-square" alt="MIT license" /></a>
</p>

OctoRill is a personal workspace for GitHub activity. It brings release updates, direct social signals, daily briefs, and an inbox entry point into one interface. It is not a full GitHub client.

<p align="center">
  <img src="./brand/exports/octo-rill-github-social-preview.png#gh-light-mode-only" alt="OctoRill dashboard preview" width="100%" />
  <img src="./brand/exports/octo-rill-github-social-preview-dark.png#gh-dark-mode-only" alt="OctoRill dashboard preview" width="100%" />
</p>

## What it does

- Read releases in `original`, `translated`, or `polished` mode.
- Show direct social signals: stars on personal repositories and new followers.
- Build daily briefs using the user's local day boundary.
- Provide a dashboard, scoped focus pages, admin pages, and a mock-only web demo.

## Repository layout

- `src/`: Rust backend for auth, sync, translation, briefs, admin APIs, and static asset serving.
- `web/`: React + Vite frontend, Storybook, Playwright tests, and the `/demo/` surface.
- `docs-site/`: Public docs site built with Rspress.
- `docs/`: Internal project docs, architecture notes, and specs.
- `migrations/`: SQLite schema migrations.
- `brand/`: Source brand files and exported assets.

## Quick start

Requirements:

- Rust `1.91.0`
- Bun `1.x`
- SQLite development libraries
- A GitHub OAuth app

Install repository tools:

```bash
bun install
```

Create local env:

```bash
cp .env.example .env.local
```

Required variables:

- `OCTORILL_ENCRYPTION_KEY_BASE64`
- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `GITHUB_OAUTH_REDIRECT_URL`

If you are not testing LinuxDO, leave these three variables empty together:

- `LINUXDO_CLIENT_ID`
- `LINUXDO_CLIENT_SECRET`
- `LINUXDO_OAUTH_REDIRECT_URL`

If you want to test translation and daily briefs, also set:

- `AI_API_KEY`
- `AI_BASE_URL`
- `AI_MODEL`

Start the backend:

```bash
cargo run
```

The backend listens on `http://127.0.0.1:58090` by default.

Start the frontend:

```bash
cd web
bun install
bun run dev
```

Open `http://127.0.0.1:55174`.

Optional local entry points:

- Storybook: `cd web && bun run storybook`
- Web demo build: `cd web && bun run build:demo`
- Docs site: `cd docs-site && bun install && bun run dev`

## Common commands

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --locked --all-targets --all-features
(cd web && bun run lint)
(cd web && bun run storybook:build)
(cd docs-site && bun run build)
```

## More docs

- Public docs site: [ivanli-cn.github.io/octo-rill](https://ivanli-cn.github.io/octo-rill/)
- Quick start: [docs-site/docs/quick-start.md](./docs-site/docs/quick-start.md) `Chinese`
- Config reference: [docs-site/docs/config.md](./docs-site/docs/config.md) `Chinese`
- Product notes: [docs-site/docs/product.md](./docs-site/docs/product.md) `Chinese`
- Internal docs: [docs/README.md](./docs/README.md)
- Architecture: [docs/architecture.md](./docs/architecture.md)
- Frontend notes: [web/README.md](./web/README.md)

## License

[MIT](./LICENSE)
