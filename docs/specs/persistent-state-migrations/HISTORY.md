# Persistent State Migration History

- The topic was adopted for the online SQLite migration and no-freeze content-processing admission implementation.
- The initial operation definition is `content-processing-online-v1` with ordered DDL, DML and historical backfill phases.
- `content-processing-online-v2` is the immutable forward-repair identity for observation-absence backfill and legacy evidence classification; v1 checksums remain read-only historical evidence.
- Historical `legacy` and `rollback_freeze` control values remain readable but are repaired forward to `global` by valid admission.
