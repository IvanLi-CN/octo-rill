---
name: persistent-state-migrations
description: "Design and verify release-to-release migrations for durable application state."
schema_version: 1
kind: policy-skill
slug: persistent-state-migrations
primary_topic: persistent-state-migrations
policy_dependencies: []
visibility: public
public_url_hosts: []
---

# Persistent-state migrations

Use this policy whenever a release creates, reads, transforms, replaces, or stops reading state that survives the running process. Temporary caches and non-persistent fixtures are out of scope.

## Workflow

1. Record the state, source compatibility range, ordered operations, rollout signals, recovery path, and validation in `migration-record.json`.
2. Keep structural changes, current-state DML, and historical-data backfills separate, ordered, observable, and pauseable.
3. Treat deployed migrations as immutable. A stopped release is repaired forward; program rollback does not silently reverse persisted state.
4. Test every declared earlier-version state and the stopped-release repair path. Verify idempotent recognition and safe re-entry.

## Repository Resources

- Topic contract: `docs/specs/persistent-state-migrations/SPEC.md`
- Implementation record: `docs/specs/persistent-state-migrations/IMPLEMENTATION.md`
- Migration record: `docs/specs/persistent-state-migrations/migration-record.json`
- Record template: `assets/templates/persistent-state-migration-record.example.json`
