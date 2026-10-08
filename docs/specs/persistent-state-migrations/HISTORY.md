# Persistent State Migration History

- The topic was introduced for online SQLite migration and no-freeze content-processing admission.
- `content-processing-online-v1` remains immutable historical evidence; `content-processing-online-v2` is the forward-repair identity used by the current operator.
- DDL, current-state repair, and historical observation backfill are separate durable operations with independent checksums and progress.
- Legacy observation identity now includes the source hash so reused legacy primary keys are observable as distinct incarnations.
- Historical `legacy` and `rollback_freeze` values remain readable but are repaired forward by valid global admission.
- ADR 0019 supersedes the old validation-only SQLx startup rule: existing history is validated first, then pending migrations run through SQLx before the listener binds, with transactional failure diagnostics.
