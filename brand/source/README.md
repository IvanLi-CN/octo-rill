# Brand media source packages

`product-poster/` and `social-preview/` hold the source packages for the four
published raster campaign assets in `brand/exports/`.

Each package contains a `manifest.json` and a standard OpenRaster `source.ora`.
The archive opens in layer-aware graphics software and has exactly one
full-canvas layer named `Fidelity master (flattened)`. This is an honest
recovery format: the original independent illustration, layout, and typography
layers were not recoverable, so they are not represented as fabricated layers.

The manifest binds one source package to one canonical export and records its
dimensions and SHA-256. `scripts/brand_media_sources.py --check` verifies the
archive structure, the one-to-one mapping, dimensions, deterministic archive
layout, and byte-identical source-layer/composite recovery for all four files.

To rebuild the tracked archives after intentionally replacing a canonical
export, update its manifest hash and run:

```sh
python3 scripts/brand_media_sources.py --write --check
```
