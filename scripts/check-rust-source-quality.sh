#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
checker_manifest="$repo_root/tools/rust-source-check/Cargo.toml"

cargo fmt --manifest-path "$repo_root/Cargo.toml" --all -- --check
cargo fmt --manifest-path "$checker_manifest" -- --check
cargo clippy --manifest-path "$repo_root/Cargo.toml" --locked --all-targets --all-features -- -D warnings
cargo check --manifest-path "$repo_root/Cargo.toml" --locked --all-targets --all-features
cargo clippy --manifest-path "$checker_manifest" --locked --all-targets -- -D warnings
cargo test --manifest-path "$checker_manifest" --locked
cargo run --manifest-path "$checker_manifest" --locked -- "$repo_root"
