# Persistent State Migration History

- The topic was adopted for the online SQLite migration and no-freeze content-processing admission implementation.
- The initial operation definition is `content-processing-online-v1` with ordered DDL, DML and historical backfill phases.
- Historical `legacy` and `rollback_freeze` control values remain readable but are repaired forward to `global` by valid admission.
