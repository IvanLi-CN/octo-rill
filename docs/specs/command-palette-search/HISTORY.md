# 命令面板与个人工作区搜索主题历史

> 这里记录主题局部生命周期、兼容性与必要背景；规范正文仍以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- New active topic. No predecessor or successor.
- The endpoint is additive and the existing reading, synchronization, notification and brief routes remain the source of truth for their own workflows.

## Replacements / Background

- The feature deliberately separates local workspace search from GitHub Search API so search cannot expand the user's visibility scope or trigger feed-side writes.
- The quota model is a lazy-start anchored fixed-window counter: a user's first valid search anchors a 300-second interval, avoiding strict sliding-window storage while retaining cross-tab and restart consistency through the shared database state.
- Original, translated and smart expressions are modeled as one canonical search document so opening a result preserves the existing reading target instead of creating parallel result rows.
- The published `0081` migration is retained byte-for-byte under `migrations/legacy/` and validated by exact SHA-384 only for databases that already applied it. New installs use schema-only `0082`; historical projection work runs in a resumable background worker with 25-row transactions, a short pause between batches and a database-directory free-space watermark.

## Related Changes

- Repository rename projection cleanup keeps one canonical repository result when star synchronization updates the name before the user association projection.
- The search response exposes `index_status` so partially indexed or low-disk workspaces remain usable without implying full historical coverage.
- Release projections refresh their repository metadata when owned-release visibility is discovered, renamed, or toggled, keeping repository filters and deep links aligned with the visibility view.
- Migration `0089_association_search_update_repair.sql` narrows association-triggered search refreshes to repository projection changes and adds a `(resource_type, repo_id)` lookup for the retained release refresh path; source/follow/observation-only updates remain outside the FTS rebuild path.
- Migration `0090_search_fts_rowid_recovery.sql` replaces the unindexed `doc_id` FTS maintenance path with rowid mapping tables and rebuildable v2 corpora, keeps compatibility view names for existing source triggers, and queues repository metadata fanout for resumable 25-release-row Background batches.
- Metadata source deletion also queues the affected repository, and a new metadata event resets its persisted release cursor so an in-flight fanout cannot leave the already-processed prefix stale.
- The recovery worker now persists both historical projection cursors and per-repository metadata cursors, continues polling after reaching `ready`, and keeps search usable through `LIKE` fallback while FTS rebuild or metadata repair is pending.
- Recovery commits at most 25 source rows per writer transaction, relies on projection triggers for ordinary FTS updates, and pauses repeated background acquisition between batches. Low-disk pause and resume events report observed free space and the configured watermark; an already-paused poll does not write the state row again.

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
