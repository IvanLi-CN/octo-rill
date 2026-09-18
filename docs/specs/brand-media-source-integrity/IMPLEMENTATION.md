# Brand Media Source Integrity Implementation

> The current requirements contract remains in `./SPEC.md`; this document records implementation coverage and rollout facts.

## Current Status

- Implementation: complete
- Lifecycle: active
- Catalog note: Four campaign exports have truthful, validated OpenRaster fidelity-master packages.

## Implementation Coverage

- `REQ-BMS-001`: `brand/source/product-poster/*/manifest.json` and `brand/source/social-preview/*/manifest.json` map the four canonical exports to source packages.
- `REQ-BMS-002`: `brand/source/README.md`, package manifests, and ADR 0011 define the non-separable fidelity-master boundary.
- `REQ-BMS-003`: `scripts/brand_media_sources.py` writes and validates deterministic OpenRaster source packages; the existing `Lint & Checks` CI job runs `--check`.
- Verification commands: `python3 scripts/brand_media_sources.py --write --check` and `python3 scripts/brand_media_sources.py --check --json`.
- Rollout facts: The two posters and light social preview remain unchanged. The dark social preview uses the verified paired dark-theme layout; no runtime consumer changes are required.

## Coverage / Rollout Summary

- The source contract protects four current assets without asserting recovery of unavailable design layers. The dark social preview visual correction is recorded in the Spec evidence.

## Remaining Gaps

- A future genuine layered working file may replace its corresponding recovered fidelity-master package after the manifest and validation contract are updated.

## Related Changes

- [ADR 0011](../../adr/0011-brand-media-fidelity-master-recovery.md)

## References

- `./SPEC.md`
- `./HISTORY.md`
