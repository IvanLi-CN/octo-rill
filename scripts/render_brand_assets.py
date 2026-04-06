#!/usr/bin/env python3
from __future__ import annotations

import base64
import shutil
import subprocess
from collections import deque
from io import BytesIO
from pathlib import Path
from textwrap import dedent

from PIL import Image, ImageChops, ImageFilter

REPO = Path(__file__).resolve().parent.parent
BRAND_SOURCE = REPO / "brand" / "source"
BRAND_EXPORTS = REPO / "brand" / "exports"
WEB_PUBLIC = REPO / "web" / "public"
WEB_BRAND = WEB_PUBLIC / "brand"
DOCS_PUBLIC = REPO / "docs-site" / "docs" / "public"
DOCS_BRAND = DOCS_PUBLIC / "brand"
REFERENCE = BRAND_SOURCE / "reference" / "generated-brand-refresh-reference.png"

WORDMARK_NAVY = "#495675"
WORDMARK_CREAM = "#FFF8EE"
ICON_BG_TOP = "#515A66"
ICON_BG_BOTTOM = "#252A30"
ICON_GLOW = "#FFF6E2"

for path in [BRAND_SOURCE, BRAND_EXPORTS, WEB_BRAND, DOCS_BRAND, REFERENCE.parent]:
    path.mkdir(parents=True, exist_ok=True)

if not REFERENCE.exists():
    raise FileNotFoundError(
        f"Missing brand reference PNG: {REFERENCE}. Download or place the approved source image first."
    )


def _largest_alpha_component(image: Image.Image, threshold: int = 20) -> tuple[tuple[int, int, int, int], list[tuple[int, int]]]:
    alpha = image.getchannel("A")
    width, height = image.size
    pixels = alpha.load()
    visited = bytearray(width * height)

    def index(x: int, y: int) -> int:
        return y * width + x

    best_count = 0
    best_bbox: tuple[int, int, int, int] | None = None
    best_pixels: list[tuple[int, int]] = []
    for y in range(height):
        for x in range(width):
            i = index(x, y)
            if visited[i] or pixels[x, y] <= threshold:
                continue
            queue = deque([(x, y)])
            visited[i] = 1
            count = 0
            component_pixels: list[tuple[int, int]] = []
            min_x = max_x = x
            min_y = max_y = y
            while queue:
                cx, cy = queue.popleft()
                component_pixels.append((cx, cy))
                count += 1
                if cx < min_x:
                    min_x = cx
                if cx > max_x:
                    max_x = cx
                if cy < min_y:
                    min_y = cy
                if cy > max_y:
                    max_y = cy
                for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                    if 0 <= nx < width and 0 <= ny < height:
                        j = index(nx, ny)
                        if not visited[j] and pixels[nx, ny] > threshold:
                            visited[j] = 1
                            queue.append((nx, ny))
            if count > best_count:
                best_count = count
                best_bbox = (min_x, min_y, max_x + 1, max_y + 1)
                best_pixels = component_pixels

    if best_bbox is None:
        raise RuntimeError("Could not isolate mascot component from approved brand reference.")
    return best_bbox, best_pixels


def _extract_mascot() -> Image.Image:
    source = Image.open(REFERENCE).convert("RGBA")
    (left, top, right, bottom), pixels = _largest_alpha_component(source)
    width = right - left
    height = bottom - top
    component_mask = Image.new("L", source.size, 0)
    mask_pixels = component_mask.load()
    for x, y in pixels:
        mask_pixels[x, y] = 255
    component_mask = component_mask.filter(ImageFilter.MaxFilter(7))
    pad = round(max(width, height) * 0.1)
    crop_box = (
        max(0, left - pad),
        max(0, top - pad),
        min(source.width, right + pad),
        min(source.height, bottom + pad),
    )
    crop = source.crop(crop_box)
    crop_mask = component_mask.crop(crop_box)
    alpha = ImageChops.multiply(crop.getchannel("A"), crop_mask)
    crop.putalpha(alpha)
    bbox = crop.getchannel("A").getbbox()
    if bbox is None:
        raise RuntimeError("Mascot crop became empty after trimming.")
    return crop.crop(bbox)


def _prepared_png(
    image: Image.Image,
    *,
    max_box: tuple[int, int] | int | None = None,
    colors: int | None = None,
) -> Image.Image:
    prepared = image.copy()
    if max_box is not None:
        if isinstance(max_box, int):
            max_box = (max_box, max_box)
        prepared.thumbnail(max_box, Image.Resampling.LANCZOS)
    if colors is not None:
        prepared = prepared.quantize(colors=colors, method=Image.Quantize.FASTOCTREE)
    return prepared


def _png_bytes(
    image: Image.Image,
    *,
    max_box: tuple[int, int] | int | None = None,
    colors: int | None = None,
) -> bytes:
    prepared = _prepared_png(image, max_box=max_box, colors=colors)
    buffer = BytesIO()
    prepared.save(buffer, format="PNG", optimize=True)
    return buffer.getvalue()


def _data_uri(
    image: Image.Image,
    *,
    max_box: tuple[int, int] | int | None = None,
    colors: int | None = None,
) -> str:
    return "data:image/png;base64," + base64.b64encode(
        _png_bytes(image, max_box=max_box, colors=colors)
    ).decode("ascii")


def _image_tag(href: str, *, x: int, y: int, width: int, height: int) -> str:
    return (
        f'<image href="{href}" x="{x}" y="{y}" width="{width}" height="{height}" '
        'preserveAspectRatio="xMidYMid meet"/>'
    )


def mark_svg(mascot_uri: str) -> str:
    return dedent(
        f'''\
        <svg width="100%" height="100%" viewBox="0 0 256 256" fill="none" xmlns="http://www.w3.org/2000/svg">
          {_image_tag(mascot_uri, x=14, y=10, width=228, height=228)}
        </svg>
        '''
    ).strip() + "\n"


def wordmark_svg(mascot_uri: str, text_fill: str) -> str:
    font_stack = "'Avenir Next Rounded', 'SF Pro Rounded', 'Nunito Sans', 'Trebuchet MS', 'Segoe UI', sans-serif"
    return dedent(
        f'''\
        <svg width="100%" height="100%" viewBox="0 0 860 256" fill="none" xmlns="http://www.w3.org/2000/svg">
          {_image_tag(mascot_uri, x=0, y=8, width=232, height=232)}
          <text x="246" y="152" fill="{text_fill}" font-family="{font_stack}" font-size="106" font-weight="800" letter-spacing="-2">OctoRill</text>
        </svg>
        '''
    ).strip() + "\n"


def _icon_shell(mascot_uri: str, *, view_box: int, inset: int, radius: int, mascot_size: int, mascot_y: int) -> str:
    glow_cx = view_box // 2
    glow_cy = int(view_box * 0.73)
    return dedent(
        f'''\
        <svg width="100%" height="100%" viewBox="0 0 {view_box} {view_box}" fill="none" xmlns="http://www.w3.org/2000/svg">
          <defs>
            <linearGradient id="bg" x1="{view_box * 0.18}" y1="{inset}" x2="{view_box * 0.86}" y2="{view_box - inset}" gradientUnits="userSpaceOnUse">
              <stop stop-color="{ICON_BG_TOP}"/>
              <stop offset="1" stop-color="{ICON_BG_BOTTOM}"/>
            </linearGradient>
            <radialGradient id="glow" cx="0" cy="0" r="1" gradientUnits="userSpaceOnUse" gradientTransform="translate({glow_cx} {glow_cy}) rotate(90) scale({view_box * 0.11} {view_box * 0.28})">
              <stop stop-color="{ICON_GLOW}" stop-opacity="0.88"/>
              <stop offset="1" stop-color="{ICON_GLOW}" stop-opacity="0"/>
            </radialGradient>
          </defs>
          <rect x="{inset}" y="{inset}" width="{view_box - inset * 2}" height="{view_box - inset * 2}" rx="{radius}" fill="url(#bg)"/>
          <ellipse cx="{glow_cx}" cy="{glow_cy}" rx="{view_box * 0.2}" ry="{view_box * 0.055}" fill="url(#glow)"/>
          {_image_tag(mascot_uri, x=(view_box - mascot_size) // 2, y=mascot_y, width=mascot_size, height=mascot_size)}
        </svg>
        '''
    ).strip() + "\n"


def app_icon_svg(mascot_uri: str) -> str:
    return _icon_shell(
        mascot_uri,
        view_box=1024,
        inset=72,
        radius=224,
        mascot_size=620,
        mascot_y=132,
    )


def favicon_svg(mascot_uri: str) -> str:
    return _icon_shell(
        mascot_uri,
        view_box=128,
        inset=8,
        radius=28,
        mascot_size=82,
        mascot_y=14,
    )


mascot_image = _extract_mascot()
mark_uri = _data_uri(mascot_image, max_box=288, colors=96)
wordmark_uri = _data_uri(mascot_image, max_box=288, colors=96)
favicon_uri = _data_uri(mascot_image, max_box=96, colors=64)
app_icon_uri = _data_uri(mascot_image, max_box=512, colors=96)

masters = {
    "mark-master.svg": mark_svg(mark_uri),
    "favicon-master.svg": favicon_svg(favicon_uri),
    "wordmark-light-master.svg": wordmark_svg(wordmark_uri, WORDMARK_NAVY),
    "wordmark-dark-master.svg": wordmark_svg(wordmark_uri, WORDMARK_CREAM),
    "app-icon-master.svg": app_icon_svg(app_icon_uri),
}

for filename, content in masters.items():
    (BRAND_SOURCE / filename).write_text(content, encoding="utf-8")

shutil.copy2(BRAND_SOURCE / "mark-master.svg", BRAND_EXPORTS / "mark.svg")
shutil.copy2(BRAND_SOURCE / "wordmark-light-master.svg", BRAND_EXPORTS / "wordmark-light.svg")
shutil.copy2(BRAND_SOURCE / "wordmark-dark-master.svg", BRAND_EXPORTS / "wordmark-dark.svg")
shutil.copy2(BRAND_SOURCE / "favicon-master.svg", BRAND_EXPORTS / "favicon.svg")

subprocess.run(
    [
        "rsvg-convert",
        "-w",
        "1024",
        "-h",
        "1024",
        str(BRAND_SOURCE / "app-icon-master.svg"),
        "-o",
        str(BRAND_EXPORTS / "app-icon-1024.png"),
    ],
    check=True,
)

subprocess.run(
    [
        "rsvg-convert",
        "-w",
        "256",
        "-h",
        "256",
        str(BRAND_SOURCE / "favicon-master.svg"),
        "-o",
        str(BRAND_EXPORTS / "favicon-256.png"),
    ],
    check=True,
)

subprocess.run(
    [
        "magick",
        str(BRAND_EXPORTS / "favicon-256.png"),
        "-define",
        "icon:auto-resize=16,32,48",
        str(BRAND_EXPORTS / "favicon.ico"),
    ],
    check=True,
)

for src_name, target in [
    ("mark.svg", WEB_BRAND / "mark.svg"),
    ("wordmark-light.svg", WEB_BRAND / "wordmark-light.svg"),
    ("wordmark-dark.svg", WEB_BRAND / "wordmark-dark.svg"),
    ("favicon.svg", WEB_BRAND / "favicon.svg"),
    ("favicon.svg", WEB_PUBLIC / "favicon.svg"),
    ("favicon.ico", WEB_PUBLIC / "favicon.ico"),
    ("mark.svg", DOCS_BRAND / "mark.svg"),
    ("favicon.svg", DOCS_BRAND / "favicon.svg"),
    ("favicon.svg", DOCS_PUBLIC / "favicon.svg"),
    ("favicon.ico", DOCS_PUBLIC / "favicon.ico"),
]:
    shutil.copy2(BRAND_EXPORTS / src_name, target)

print("rendered brand assets")
