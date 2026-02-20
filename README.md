# OctoRill

Fetch your GitHub starred repos' release history, show Notifications, and generate an AI daily brief.

## Tech stack

- Backend: Rust (axum) + SQLite (sqlx)
- Frontend: React (Vite) + Bun

## Dev

### 1) Configure env

Copy `.env.example` to `.env` and fill values.

### 2) Start backend

```bash
cargo run
```

### 3) Start frontend

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
