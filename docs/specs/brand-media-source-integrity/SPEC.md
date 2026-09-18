# Brand Media Source Integrity

> This file is the durable topic requirements contract. Current implementation facts belong in `IMPLEMENTATION.md`; lifecycle and change references belong in `HISTORY.md`.

## Context and Scope

- Context: The four shipped campaign raster assets require durable, truthful source packages rather than export-only delivery.
- In scope: Product-poster and GitHub social-preview source packages, their export mappings, and source-integrity validation.
- Out of scope: Reconstructing unavailable independent design layers or changing the approved exported artwork.

## Terms and Interfaces

- `fidelity master`: A full-canvas, flattened source layer that preserves the published artwork exactly while declaring its lack of separable original layers.
- `source package`: A `manifest.json` and sibling OpenRaster `source.ora` that map to one canonical export.
- Interface: `python3 scripts/brand_media_sources.py --write --check`.

## Requirements

### REQ-BMS-001

- The repository MUST retain one source package for each shipped product-poster and GitHub social-preview raster export.
- Inputs: The four canonical files in `brand/exports/`.
- Outputs: One mapped package under `brand/source/product-poster/` or `brand/source/social-preview/`.

### REQ-BMS-002

- A recovered package MUST identify a flattened fidelity master and MUST NOT claim unrecovered illustration, layout, logo, or typography components are independently editable.

### REQ-BMS-003

- Local and CI source validation MUST prove one-to-one export coverage, archive integrity, dimensions, deterministic source-package layout, and pixel-identical source recovery.

## Verification

### VER-BMS-001

- Method: Run `python3 scripts/brand_media_sources.py --check`.
- covers: `REQ-BMS-001`
- Pass condition: Every canonical campaign export has exactly one valid source package.

### VER-BMS-002

- Method: Inspect the package manifests and `brand/source/README.md`.
- covers: `REQ-BMS-002`
- Pass condition: All packages declare `recovered-fidelity-master` and state the unavailable original layers.

### VER-BMS-003

- Method: Run `python3 scripts/brand_media_sources.py --write --check`.
- covers: `REQ-BMS-003`
- Pass condition: Each OpenRaster archive has the expected integrity and its master/composite are byte-identical to the mapped export; the existing `Lint & Checks` CI job runs the same validator.

## Related ADRs

- [ADR 0011: Brand Media Fidelity Master Recovery](../../adr/0011-brand-media-fidelity-master-recovery.md)

## Visual Evidence

- Corrected dark social preview:

![Corrected dark social preview](./assets/social-preview-dark.png)

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
