# Brand Media Fidelity Master Recovery

The current product posters and GitHub social previews are shipped raster
artwork. Their original editable working files are unavailable: historical
inspection recovered an older, different light-poster draft but no source that
faithfully reproduces any current campaign export.

Each current export therefore has one adjacent OpenRaster source package with
a single full-canvas fidelity-master layer. The package preserves the exact
published pixels in a standard editor-readable format, declares its flattened
status, and maps to exactly one canonical export through a tracked manifest.
The repository must not invent independent illustration, logo, typography, or
layout layers that were not recovered.

`scripts/brand_media_sources.py --check` validates the OpenRaster structure,
manifest coverage, canvas dimensions, deterministic archive bytes, and the
byte-identical source-layer/composite recovery of all four exports. This makes
the recovery boundary visible and prevents a source package from silently
drifting away from the file it represents.

The consequence is intentionally limited editability: the source package can
be reopened and adjusted as one master layer, but individual original elements
cannot be edited separately until a genuine layered source is supplied.
