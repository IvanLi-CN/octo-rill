# Brand Media Source Integrity History

> This document records topic-local lifecycle and compatibility facts. Decision rationale remains in `docs/adr/`.

## Lifecycle / Compatibility

- Recovered source packages are compatible with the existing exported PNG paths and do not change runtime consumers.

## Replacements / Background

- Export-only campaign artwork is replaced by explicitly flattened, editor-readable source packages. ADR 0011 records why unavailable original layers are not simulated.
- The dark social preview uses the verified dark-theme companion to the current light social preview, replacing an unrelated legacy composition.

## Related Changes

- [ADR 0011](../../adr/0011-brand-media-fidelity-master-recovery.md)

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
