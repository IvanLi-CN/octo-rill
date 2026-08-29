#!/usr/bin/env python3
"""Build and verify the editable source package for the approved posters.

The approved product posters are raster-led compositions. Their source package
therefore keeps the supplied hero scene as named raster materials, while the
brand lettering and small decoration retain independent editable SVG sources.
The script deliberately renders from the committed source layers. Canonical
exports are only used when explicitly recovering the source package and when
verifying that a render remains approved.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
from io import BytesIO
from pathlib import Path
from tempfile import NamedTemporaryFile
from xml.etree import ElementTree
from zipfile import ZIP_DEFLATED, ZIP_STORED, ZipFile, ZipInfo

from PIL import Image, ImageChops, ImageDraw, ImageFilter


REPO = Path(__file__).resolve().parent.parent
SOURCE_ROOT = REPO / "brand" / "source" / "product-posters"
EXPORT_ROOT = REPO / "brand" / "exports"
CANVAS = (3072, 3840)
LAYER_ORDER = (
    "background",
    "decorative",
    "dashboard",
    "platform",
    "mascot",
    "inbox",
    "wordmark",
)
ZIP_EPOCH = (1980, 1, 1, 0, 0, 0)

EXPORTS = {
    "light": EXPORT_ROOT / "octo-rill-product-poster-light.png",
    "dark": EXPORT_ROOT / "octo-rill-product-poster.png",
}
ORAS = {
    "light": SOURCE_ROOT / "octo-rill-product-poster-light.ora",
    "dark": SOURCE_ROOT / "octo-rill-product-poster-dark.ora",
}
CANONICAL_WORDMARK = REPO / "brand" / "source" / "wordmark-lettering.svg"
SHARED_WORDMARK = SOURCE_ROOT / "shared" / "wordmark-lettering.svg"
MANIFEST = SOURCE_ROOT / "manifest.json"
README = SOURCE_ROOT / "README.md"


def load_rgb(path: Path, expected_size: tuple[int, int] = CANVAS) -> Image.Image:
    if not path.is_file():
        raise FileNotFoundError(f"Missing material: {path}")
    image = Image.open(path).convert("RGB")
    if image.size != expected_size:
        raise ValueError(f"Unexpected dimensions for {path}: {image.size}, expected {expected_size}")
    return image


def load_rgba(path: Path, expected_size: tuple[int, int] = CANVAS) -> Image.Image:
    if not path.is_file():
        raise FileNotFoundError(f"Missing material: {path}")
    image = Image.open(path).convert("RGBA")
    if image.size != expected_size:
        raise ValueError(f"Unexpected dimensions for {path}: {image.size}, expected {expected_size}")
    return image


def rgb_sha256(image: Image.Image) -> str:
    return hashlib.sha256(image.convert("RGB").tobytes()).hexdigest()


def rgba_sha256(image: Image.Image) -> str:
    return hashlib.sha256(image.convert("RGBA").tobytes()).hexdigest()


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def png_bytes(image: Image.Image) -> bytes:
    buffer = BytesIO()
    image.save(buffer, format="PNG")
    return buffer.getvalue()


def atomic_write_bytes(path: Path, contents: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: Path | None = None
    try:
        with NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.", delete=False) as temporary:
            temporary.write(contents)
            temporary_path = Path(temporary.name)
        temporary_path.replace(path)
    finally:
        if temporary_path is not None and temporary_path.exists():
            temporary_path.unlink()


def write_png(path: Path, image: Image.Image) -> None:
    atomic_write_bytes(path, png_bytes(image))


def blank_mask() -> Image.Image:
    return Image.new("L", CANVAS)


def rounded_rectangle_mask(box: tuple[int, int, int, int], radius: int) -> Image.Image:
    mask = blank_mask()
    ImageDraw.Draw(mask).rounded_rectangle(box, radius=radius, fill=255)
    return mask


def ellipse_mask(boxes: tuple[tuple[int, int, int, int], ...]) -> Image.Image:
    mask = blank_mask()
    draw = ImageDraw.Draw(mask)
    for box in boxes:
        draw.ellipse(box, fill=255)
    return mask


def union_masks(masks: list[Image.Image]) -> Image.Image:
    merged = blank_mask()
    for mask in masks:
        merged = ImageChops.lighter(merged, mask)
    return merged


def mint_candidate_mask(reference: Image.Image) -> Image.Image:
    left, top, right, bottom = (620, 1130, 2410, 2920)
    crop = reference.crop((left, top, right, bottom))
    source = crop.load()
    matte = Image.new("L", crop.size)
    pixels = matte.load()
    for y in range(crop.height):
        for x in range(crop.width):
            red, green, blue = source[x, y]
            if green >= 112 and green >= red + 14 and green >= blue + 6:
                pixels[x, y] = 255
    result = blank_mask()
    result.paste(matte, (left, top))
    return result


def decorative_mask(reference: Image.Image, mascot: Image.Image) -> Image.Image:
    mask = blank_mask()
    draw = ImageDraw.Draw(mask)
    for x, y, radius in (
        (1215, 713, 10),
        (1330, 758, 7),
        (1510, 718, 8),
        (1720, 1010, 18),
        (2500, 1725, 13),
        (2410, 1800, 8),
    ):
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=255)
    for x, y, radius in ((1450, 480, 34), (2770, 830, 22), (2350, 2480, 20)):
        draw.polygon(
            (
                (x, y - radius),
                (x + radius // 3, y - radius // 3),
                (x + radius, y),
                (x + radius // 3, y + radius // 3),
                (x, y + radius),
                (x - radius // 3, y + radius // 3),
                (x - radius, y),
                (x - radius // 3, y - radius // 3),
            ),
            fill=255,
        )
    isolated_mint = mint_candidate_mask(reference).filter(ImageFilter.MaxFilter(15))
    mascot_halo = mascot.filter(ImageFilter.MaxFilter(81))
    return ImageChops.lighter(mask, ImageChops.subtract(isolated_mint, mascot_halo))


def mascot_mask(reference: Image.Image) -> Image.Image:
    """Recover a generous silhouette around the central mint mascot."""

    left, top, right, bottom = (620, 1130, 2410, 2920)
    candidate = mint_candidate_mask(reference).crop((left, top, right, bottom))
    selection_scale = 4
    selection = candidate.resize(
        ((candidate.width + selection_scale - 1) // selection_scale,
         (candidate.height + selection_scale - 1) // selection_scale),
        Image.Resampling.NEAREST,
    )
    source = selection.load()
    seed: tuple[int, int] | None = None
    best_distance = float("inf")
    anchor_x, anchor_y = selection.width // 2, selection.height * 2 // 5
    for y in range(selection.height):
        for x in range(selection.width):
            if source[x, y] != 255:
                continue
            distance = (x - anchor_x) ** 2 + (y - anchor_y) ** 2
            if distance < best_distance:
                best_distance = distance
                seed = (x, y)
    if seed is None:
        raise RuntimeError("Could not find the central mascot material")

    # Select only the connected central body, leaving floating UI glyphs for
    # the decorative layer. A modest expansion bridges the mascot's dark line
    # work before the flood fill; the final halo carries its outline and shadow.
    connected = selection.filter(ImageFilter.MaxFilter(5))
    ImageDraw.floodfill(connected, seed, 128, border=0)
    matte = connected.point(lambda value: 255 if value == 128 else 0)
    matte = matte.resize(candidate.size, Image.Resampling.NEAREST)
    matte = matte.filter(ImageFilter.MaxFilter(61)).filter(ImageFilter.MinFilter(7))
    result = blank_mask()
    result.paste(matte, (left, top))
    draw = ImageDraw.Draw(result)
    draw.ellipse((945, 1250, 2140, 2220), fill=255)
    draw.ellipse((1030, 2510, 2070, 2800), fill=255)
    return result


def wordmark_mask(theme: str) -> Image.Image:
    """Rasterize the committed pure SVG geometry into a padded matte."""

    renderer = shutil.which("rsvg-convert")
    if renderer is None:
        raise RuntimeError("rsvg-convert is required only when recovering wordmark source materials")
    svg_path = SOURCE_ROOT / theme / "wordmark.svg"
    temporary_path: Path | None = None
    try:
        with NamedTemporaryFile(suffix=".png", delete=False) as temporary:
            temporary_path = Path(temporary.name)
        subprocess.run(
            [
                renderer,
                "-w",
                str(CANVAS[0]),
                "-h",
                str(CANVAS[1]),
                str(svg_path),
                "-o",
                str(temporary_path),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        bounds = (500, 50, 2600, 600)
        alpha = load_rgba(temporary_path).getchannel("A")
        expanded = alpha.crop(bounds).filter(ImageFilter.GaussianBlur(radius=28))
        matte = expanded.point(lambda value: 255 if value > 1 else 0)
        result = blank_mask()
        result.paste(matte, bounds[:2])
        return result
    finally:
        if temporary_path is not None and temporary_path.exists():
            temporary_path.unlink()


def masks_for_theme(theme: str, reference: Image.Image) -> dict[str, Image.Image]:
    dashboard = union_masks(
        [
            rounded_rectangle_mask((75, 520, 1165, 1550), 100),
            rounded_rectangle_mask((1890, 520, 2995, 1550), 100),
            rounded_rectangle_mask((55, 1920, 1040, 2990), 94),
            rounded_rectangle_mask((2160, 1820, 3035, 3050), 94),
        ]
    )
    inbox = rounded_rectangle_mask((850, 2800, 2130, 3838), 88)
    if theme == "light":
        platform = ellipse_mask(((30, 2740, 3040, 3530), (420, 2530, 2650, 3370)))
    else:
        platform = ellipse_mask(((220, 2720, 2860, 3520), (720, 2540, 2360, 3210)))
    mascot = mascot_mask(reference)

    return {
        "decorative": decorative_mask(reference, mascot),
        "dashboard": dashboard,
        "platform": platform,
        "mascot": mascot,
        "inbox": inbox,
        "wordmark": wordmark_mask(theme),
    }


def clean_scene_plate(theme: str) -> Image.Image:
    """Create a neutral scene continuation with no UI, mascot, or lettering."""

    top, bottom, glow = (
        ((253, 249, 241), (253, 251, 243), (189, 245, 223, 20))
        if theme == "light"
        else ((0, 13, 27), (1, 14, 30), (0, 125, 145, 18))
    )
    column = Image.new("RGB", (1, CANVAS[1]))
    pixels = column.load()
    for y in range(CANVAS[1]):
        ratio = y / (CANVAS[1] - 1)
        pixels[0, y] = tuple(round(start + (end - start) * ratio) for start, end in zip(top, bottom))

    plate = column.resize(CANVAS, Image.Resampling.BILINEAR).convert("RGBA")
    atmosphere = Image.new("RGBA", CANVAS)
    draw = ImageDraw.Draw(atmosphere)
    draw.ellipse((650, 900, 2420, 2860), fill=glow)
    atmosphere = atmosphere.filter(ImageFilter.GaussianBlur(radius=210))
    return Image.alpha_composite(plate, atmosphere).convert("RGB")


def clean_component(
    target: Image.Image,
    mask: Image.Image,
    occluding_mask: Image.Image,
    clean_plate: Image.Image,
) -> Image.Image:
    component = target.convert("RGBA")
    if occluding_mask.getbbox() is not None:
        component.paste(clean_plate, (0, 0), occluding_mask)
    component.putalpha(mask)
    return component


def source_layers_from_export(
    theme: str,
    target: Image.Image,
) -> dict[str, Image.Image]:
    masks = masks_for_theme(theme, target)
    all_foreground = union_masks([masks[name] for name in LAYER_ORDER[1:]])
    clean_plate = clean_scene_plate(theme)

    background = clean_plate.copy()
    background.paste(target, (0, 0), ImageChops.invert(all_foreground))
    layers: dict[str, Image.Image] = {"background": background.convert("RGBA")}

    for index, name in enumerate(LAYER_ORDER[1:], start=1):
        upper_masks = [masks[upper] for upper in LAYER_ORDER[index + 1:]]
        layers[name] = clean_component(
            target,
            masks[name],
            union_masks(upper_masks),
            clean_plate,
        )
    return layers


def layer_path(theme: str, name: str) -> Path:
    return SOURCE_ROOT / theme / f"{name}.png"


def load_source_layers(theme: str) -> dict[str, Image.Image]:
    return {name: load_rgba(layer_path(theme, name)) for name in LAYER_ORDER}


def composite_layers(layers: dict[str, Image.Image]) -> Image.Image:
    composite = layers["background"].convert("RGBA")
    for name in LAYER_ORDER[1:]:
        composite = Image.alpha_composite(composite, layers[name].convert("RGBA"))
    return composite.convert("RGB")


def assert_equal(actual: Image.Image, expected: Image.Image | Path, label: str) -> None:
    expected_image = load_rgb(expected) if isinstance(expected, Path) else expected.convert("RGB")
    if ImageChops.difference(actual.convert("RGB"), expected_image).getbbox() is not None:
        raise RuntimeError(f"{label} differs from the approved pixels")


def wordmark_path_groups() -> tuple[str, str]:
    source = CANONICAL_WORDMARK.read_text(encoding="utf-8")
    match = re.search(r'<path[^>]*\sd="([^"]+)"', source)
    if match is None:
        raise RuntimeError(f"Could not recover path data from {CANONICAL_WORDMARK}")
    fragments = re.findall(r"M[^M]+", match.group(1))
    if len(fragments) < 2:
        raise RuntimeError("The canonical wordmark path cannot be split into editable glyph groups")
    split = max(1, len(fragments) // 2)
    return (" ".join(fragments[:split]), " ".join(fragments[split:]))


def write_vector_sources() -> None:
    canonical = CANONICAL_WORDMARK.read_bytes()
    atomic_write_bytes(SHARED_WORDMARK, canonical)
    left, right = wordmark_path_groups()
    for theme, dark_fill, mint_fill in (
        ("light", "#102a46", "#49d6ab"),
        ("dark", "#f4fbf9", "#69d8b4"),
    ):
        wordmark = f'''<svg xmlns="http://www.w3.org/2000/svg" width="3072" height="3840" viewBox="0 0 3072 3840">
  <g transform="translate(609 132) scale(3.484 3.209)">
    <path fill="{dark_fill}" d="{left}"/>
    <path fill="{mint_fill}" d="{right}"/>
  </g>
</svg>
'''
        decorative = f'''<svg xmlns="http://www.w3.org/2000/svg" width="3072" height="3840" viewBox="0 0 3072 3840">
  <g fill="{mint_fill}">
    <circle cx="1215" cy="713" r="10"/>
    <circle cx="1330" cy="758" r="7"/>
    <circle cx="1510" cy="718" r="8"/>
    <path d="M1450 446 L1461 469 L1484 480 L1461 491 L1450 514 L1439 491 L1416 480 L1439 469 Z"/>
    <path d="M2770 808 L2777 823 L2792 830 L2777 837 L2770 852 L2763 837 L2748 830 L2763 823 Z"/>
  </g>
</svg>
'''
        atomic_write_bytes(SOURCE_ROOT / theme / "wordmark.svg", wordmark.encode("utf-8"))
        atomic_write_bytes(SOURCE_ROOT / theme / "decorative.svg", decorative.encode("utf-8"))


def zip_info(name: str, compression: int) -> ZipInfo:
    info = ZipInfo(name, date_time=ZIP_EPOCH)
    info.compress_type = compression
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    return info


def stack_xml(theme: str) -> bytes:
    display_name = f"OctoRill approved {theme} product poster"
    layers = "\n".join(
        (
            f'    <layer name="{name}" src="data/layer{index}.png" opacity="1" '
            'composite-op="svg:src-over" visibility="visible" />'
        )
        for index, name in reversed(tuple(enumerate(LAYER_ORDER)))
    )
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<image version="0.0.1" w="{CANVAS[0]}" h="{CANVAS[1]}" name="{display_name}">
  <stack opacity="1" composite-op="svg:src-over">
{layers}
  </stack>
</image>
'''.encode("utf-8")


def write_openraster(theme: str, layers: dict[str, Image.Image], composite: Image.Image) -> None:
    path = ORAS[theme]
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: Path | None = None
    try:
        with NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.", delete=False) as temporary:
            temporary_path = Path(temporary.name)
        with ZipFile(
            temporary_path,
            "w",
            compression=ZIP_DEFLATED,
            compresslevel=9,
            strict_timestamps=True,
        ) as archive:
            archive.writestr(zip_info("mimetype", ZIP_STORED), b"image/openraster")
            archive.writestr(zip_info("mergedimage.png", ZIP_DEFLATED), png_bytes(composite))
            thumbnail = composite.copy()
            thumbnail.thumbnail((256, 320), Image.Resampling.LANCZOS)
            archive.writestr(zip_info("Thumbnails/thumbnail.png", ZIP_DEFLATED), png_bytes(thumbnail))
            for index, name in enumerate(LAYER_ORDER):
                archive.writestr(
                    zip_info(f"data/layer{index}.png", ZIP_DEFLATED),
                    png_bytes(layers[name].convert("RGBA")),
                )
            archive.writestr(zip_info("stack.xml", ZIP_DEFLATED), stack_xml(theme))
        temporary_path.replace(path)
    finally:
        if temporary_path is not None and temporary_path.exists():
            temporary_path.unlink()


def source_file_paths() -> list[Path]:
    paths = [SHARED_WORDMARK]
    for theme in ("light", "dark"):
        paths.extend(layer_path(theme, name) for name in LAYER_ORDER)
        paths.extend(
            (
                SOURCE_ROOT / theme / "wordmark.svg",
                SOURCE_ROOT / theme / "decorative.svg",
                ORAS[theme],
            )
        )
    return paths


def write_manifest() -> None:
    manifest = {
        "canvas": {"width": CANVAS[0], "height": CANVAS[1]},
        "layer_order": list(LAYER_ORDER),
        "source_kind": "recovered-semantic-layers",
        "themes": {
            theme: {
                "export": str(EXPORTS[theme].relative_to(REPO)),
                "export_rgb_sha256": rgb_sha256(load_rgb(EXPORTS[theme])),
                "layers": [
                    {
                        "name": name,
                        "path": str(layer_path(theme, name).relative_to(SOURCE_ROOT)),
                        "rgba_sha256": rgba_sha256(load_rgba(layer_path(theme, name))),
                    }
                    for name in LAYER_ORDER
                ],
                "openraster": ORAS[theme].name,
                "wordmark_vector": f"{theme}/wordmark.svg",
                "decorative_vector": f"{theme}/decorative.svg",
            }
            for theme in ("light", "dark")
        },
        "files": [
            {
                "path": str(path.relative_to(SOURCE_ROOT)),
                "sha256": file_sha256(path),
            }
            for path in source_file_paths()
        ],
    }
    atomic_write_bytes(MANIFEST, (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode("utf-8"))


def write_readme() -> None:
    contents = """# Product Poster Sources

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
"""
    atomic_write_bytes(README, contents.encode("utf-8"))


def write_source_packages() -> None:
    targets = {theme: load_rgb(path) for theme, path in EXPORTS.items()}
    write_vector_sources()
    generated: dict[str, dict[str, Image.Image]] = {}
    for theme in ("light", "dark"):
        generated[theme] = source_layers_from_export(theme, targets[theme])
        for name in LAYER_ORDER:
            write_png(layer_path(theme, name), generated[theme][name])
        composite = composite_layers(generated[theme])
        assert_equal(composite, targets[theme], f"Recovered {theme} layer stack")

    for theme in ("light", "dark"):
        write_openraster(theme, generated[theme], composite_layers(generated[theme]))
    write_manifest()
    write_readme()


def validate_pure_svg(path: Path) -> None:
    contents = path.read_text(encoding="utf-8").lower()
    for forbidden in ("<image", "<text", "href=", "xlink:href"):
        if forbidden in contents:
            raise RuntimeError(f"Vector source contains forbidden embedded content: {path}")
    if "<path" not in contents and "<circle" not in contents:
        raise RuntimeError(f"Vector source contains no editable geometry: {path}")


def verify_manifest() -> None:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if manifest.get("layer_order") != list(LAYER_ORDER):
        raise RuntimeError("Manifest layer order does not match the committed source package")
    if manifest.get("source_kind") != "recovered-semantic-layers":
        raise RuntimeError("Manifest source kind is not declared")
    for entry in manifest.get("files", []):
        path = SOURCE_ROOT / entry["path"]
        if file_sha256(path) != entry["sha256"]:
            raise RuntimeError(f"Manifest hash differs for {path}")
    if SHARED_WORDMARK.read_bytes() != CANONICAL_WORDMARK.read_bytes():
        raise RuntimeError("Shared wordmark geometry differs from the canonical source")


def verify_openraster(theme: str, expected: Image.Image, sources: dict[str, Image.Image]) -> None:
    path = ORAS[theme]
    expected_members = {
        "mimetype",
        "mergedimage.png",
        "Thumbnails/thumbnail.png",
        "stack.xml",
        *(f"data/layer{index}.png" for index in range(len(LAYER_ORDER))),
    }
    with ZipFile(path) as archive:
        infos = archive.infolist()
        if set(archive.namelist()) != expected_members:
            raise RuntimeError(f"OpenRaster package has unexpected members: {path}")
        if not infos or infos[0].filename != "mimetype" or infos[0].compress_type != ZIP_STORED:
            raise RuntimeError(f"OpenRaster mimetype entry is invalid: {path}")
        if archive.read("mimetype") != b"image/openraster":
            raise RuntimeError(f"OpenRaster mimetype value is invalid: {path}")
        for info in infos:
            if info.date_time != ZIP_EPOCH:
                raise RuntimeError(f"OpenRaster zip timestamp is not reproducible: {path}")

        root = ElementTree.fromstring(archive.read("stack.xml"))
        stack_layers = root.findall("./stack/layer")
        expected_sources = [f"data/layer{index}.png" for index in reversed(range(len(LAYER_ORDER)))]
        actual_sources = [layer.attrib.get("src") for layer in stack_layers]
        if actual_sources != expected_sources:
            raise RuntimeError(f"OpenRaster layer stack order is invalid: {path}")

        embedded: dict[str, Image.Image] = {}
        for index, name in enumerate(LAYER_ORDER):
            image = Image.open(BytesIO(archive.read(f"data/layer{index}.png"))).convert("RGBA")
            if image.size != CANVAS:
                raise RuntimeError(f"OpenRaster layer dimensions are invalid: {path}")
            if ImageChops.difference(image, sources[name]).getbbox() is not None:
                raise RuntimeError(f"OpenRaster layer differs from committed source: {path} ({name})")
            embedded[name] = image

        merged = Image.open(BytesIO(archive.read("mergedimage.png"))).convert("RGB")
        assert_equal(merged, expected, f"OpenRaster merged image for {theme}")
        assert_equal(composite_layers(embedded), expected, f"OpenRaster layer stack for {theme}")


def verify_theme(theme: str) -> Image.Image:
    sources = load_source_layers(theme)
    background_alpha = sources["background"].getchannel("A").getextrema()
    if background_alpha != (255, 255):
        raise RuntimeError(f"{theme} background must be an opaque clean plate")
    for name in LAYER_ORDER[1:]:
        alpha = sources[name].getchannel("A")
        if alpha.getbbox() is None or alpha.getextrema()[1] != 255:
            raise RuntimeError(f"{theme} {name} layer has no opaque semantic material")

    for vector in (
        SHARED_WORDMARK,
        SOURCE_ROOT / theme / "wordmark.svg",
        SOURCE_ROOT / theme / "decorative.svg",
    ):
        validate_pure_svg(vector)

    rendered = composite_layers(sources)
    expected = load_rgb(EXPORTS[theme])
    assert_equal(rendered, expected, f"Rendered {theme} source layers")
    verify_openraster(theme, expected, sources)
    return rendered


def checkerboard(size: tuple[int, int]) -> Image.Image:
    tile = 32
    image = Image.new("RGB", size, "#e7edf0")
    draw = ImageDraw.Draw(image)
    for y in range(0, size[1], tile):
        for x in range(0, size[0], tile):
            if (x // tile + y // tile) % 2:
                draw.rectangle((x, y, x + tile - 1, y + tile - 1), fill="#cad5da")
    return image


def review_preview(image: Image.Image, name: str) -> Image.Image:
    tile_size = (768, 1020)
    preview_area = (720, 900)
    panel = Image.new("RGB", tile_size, "#f6f8fa")
    panel.paste(checkerboard(preview_area), (24, 84))
    resized = image.convert("RGBA").resize(preview_area, Image.Resampling.LANCZOS)
    panel.paste(resized, (24, 84), resized)
    ImageDraw.Draw(panel).text((24, 30), name, fill="#102a46")
    return panel


def write_review_board(theme: str, layers: dict[str, Image.Image], composite: Image.Image, destination: Path) -> None:
    tiles = [review_preview(layers[name], name) for name in LAYER_ORDER]
    tiles.append(review_preview(composite.convert("RGBA"), "approved composite"))
    board = Image.new("RGB", (4 * 768, 2 * 1020), "#dbe4e8")
    for index, tile in enumerate(tiles):
        board.paste(tile, ((index % 4) * 768, (index // 4) * 1020))
    destination.mkdir(parents=True, exist_ok=True)
    write_png(destination / f"octo-rill-product-poster-{theme}-layers.png", board)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, help="Write rendered approved PNGs to this directory.")
    parser.add_argument(
        "--review-dir",
        type=Path,
        help="Write a non-source layer review board for each requested theme.",
    )
    parser.add_argument(
        "--write-source-packages",
        action="store_true",
        help="Recover semantic materials and deterministic OpenRaster documents from approved exports.",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Validate committed layers, vectors, ORA stacks, and approved RGB pixels.",
    )
    args = parser.parse_args()

    if args.write_source_packages:
        write_source_packages()

    if args.verify:
        verify_manifest()

    rendered = {theme: verify_theme(theme) for theme in ("light", "dark")}

    if args.output_dir is not None:
        args.output_dir.mkdir(parents=True, exist_ok=True)
        for theme in ("light", "dark"):
            write_png(args.output_dir / EXPORTS[theme].name, rendered[theme])

    if args.review_dir is not None:
        for theme in ("light", "dark"):
            write_review_board(theme, load_source_layers(theme), rendered[theme], args.review_dir)

    for theme in ("light", "dark"):
        print(f"{theme} RGB sha256: {rgb_sha256(rendered[theme])}")


if __name__ == "__main__":
    main()
