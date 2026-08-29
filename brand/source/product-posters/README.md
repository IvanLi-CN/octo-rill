# Product Poster Sources

This directory contains the editable, semantic source package for the approved
OctoRill product posters. The published files remain in brand/exports/.

The historical repository did not contain a complete editable design master.
These source packages were recovered from the approved canonical exports so
future review is based on real composition layers rather than a full poster
plus a repair delta.

Each theme has these ordered, full-canvas layers:

- background.png: opaque clean hero-scene plate.
- decorative.png: independent small background decorations.
- dashboard.png: release, repository, follower, and daily-brief cards.
- platform.png: the foreground floor and shelf composition.
- mascot.png: the central OctoRill mascot and its local shadow.
- inbox.png: the inbox card.
- wordmark.png: the approved brand lettering raster material.

The corresponding .ora files embed exactly those seven layers in the same
order. They are self-contained OpenRaster documents and can be opened in
standard image editors. shared/wordmark-lettering.svg is the canonical pure
path source; each theme also has a placed wordmark.svg and decorative.svg with
no embedded raster image or text.

The hero scene remains raster material by design. The source package does not
claim that model-originated scenery is editable vector art.

To reproduce and validate the approved exports:

    python3 scripts/render_product_posters.py --verify

The verifier reads the committed PNG layers, recomposites them, reads the
actual ORA layer stack, recomposites that stack, checks pure vector sources,
and compares decoded RGB pixels against the approved exports.
