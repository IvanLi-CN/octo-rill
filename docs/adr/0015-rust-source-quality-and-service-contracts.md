# Rust Source Quality and Service Contracts

Status: accepted

OctoRill adopts three related but separate contracts: `Rust source quality` defines how Rust source remains formatted, linted, structurally reviewable, and covered for the host/features it actually ships; `Rust web service` defines the bind, authentication, SQLite, HTTP/SSE, embedded-asset, worker-lifecycle, health, and shutdown boundaries of the long-running binary; `quality-gates` defines how those validations become merge and release evidence.

The repository keeps Rust source quality as an independent required check named `Rust Source Quality`. The existing `Lint & Checks` job retains Web, docs, brand, and quality-gates contract responsibilities. This makes a source-quality regression visible without changing the meaning of backend tests, browser E2E, worktree bootstrap, Docker smoke, review, or release checks.

The source-quality contract uses `scripts/check-rust-source-quality.sh` as its local and CI entry. It runs the application and checker formatting/lint checks, locked host all-targets/all-features coverage, and an AST-based checker. The checker keeps the current item-level suppressions in `rust-source-quality.toml` as a ratcheted legacy baseline. It does not impose a repository-wide file-length threshold and does not copy another project's numeric thresholds or embedded target matrix. New suppressions must be narrow, reviewed, and reflected in the baseline in the same change.

The service contract preserves loopback local binding (`127.0.0.1:58090`), the explicit Docker override (`0.0.0.0:3000`), SQLite migrations/WAL persistence, `/api/health`, `/api/version`, SSE, SPA/static fallback, startup recovery, and Ctrl+C/SIGTERM cleanup. Docker release smoke remains responsible for exercising the assembled web/Rust artifact and the health/version response.

**Considered Options**

- Put Rust source quality inside `Lint & Checks`: rejected because the failure would be mixed with Web/docs validation and would not have an independent required-check identity.
- Copy Flux Purr's `100/7/4`, zero-debt, or target-specific rules: rejected because OctoRill is a host-target Rust service with large existing modules and a different delivery boundary.
- Treat Docker build success as the service contract: rejected because it does not prove health, version, static asset, persistence, SSE, or shutdown behavior.

**Consequences**

- Pull requests gain one additional required check and GitHub ruleset alignment must be updated with the checked-in declaration.
- Existing suppressions remain reviewable without forcing an unrelated debt-remediation rewrite.
- Future Rust changes have one canonical local command and one explicit CI owner.
- Service runtime behavior is documented as a product boundary instead of being inferred from route handlers or the Dockerfile alone.
