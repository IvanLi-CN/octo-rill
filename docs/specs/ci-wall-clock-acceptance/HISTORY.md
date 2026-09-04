# CI 墙钟性能验收主题历史

## Lifecycle / Compatibility

- None

## Replacements / Background

- This topic records the durable CI scheduling and controlled performance-acceptance contract introduced after the post-merge pipeline wall-clock investigation.
- The contract now includes two-worker Frontend E2E execution, always-run JSON reporting, 14-day result artifacts, deterministic translation-loading fixture gates, and job-level E2E A/B acceptance metrics.
- Historical target commits remain comparable after their topic branch is deleted: the current `main` workflow supplies stable reporting tooling while each controlled run checks out its immutable target SHA.

## Related Changes

- None

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
