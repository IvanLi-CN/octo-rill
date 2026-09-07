# CI 墙钟性能验收主题历史

## Lifecycle / Compatibility

- None

## Replacements / Background

- This topic records the durable CI scheduling and controlled performance-acceptance contract introduced after the post-merge pipeline wall-clock investigation.
- The contract now includes two-worker Frontend E2E execution, always-run JSON reporting, 14-day result artifacts, deterministic translation-loading fixture gates, and job-level E2E A/B acceptance metrics.
- Historical target commits remain comparable: each control/candidate run uses its frozen role-specific workflow ref, checks out its immutable target SHA, and binds summary tooling to the workflow SHA selected by that ref.

## Related Changes

- None

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
