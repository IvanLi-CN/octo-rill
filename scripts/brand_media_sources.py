#!/usr/bin/env python3
"""Build and validate recovered OpenRaster source packages for brand media."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import xml.etree.ElementTree as ET
import zipfile


REPO = Path(__file__).resolve().parent.parent
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
MIMETYPE = b"image/openraster"
ARCHIVE_MEMBERS = (
    "mimetype",
    "stack.xml",
    "data/fidelity-master.png",
    "mergedimage.png",
)
SOURCE_MANIFESTS = (
    "brand/source/product-poster/dark/manifest.json",
    "brand/source/product-poster/light/manifest.json",
    "brand/source/social-preview/light/manifest.json",
    "brand/source/social-preview/dark/manifest.json",
)
ZIP_TIMESTAMP = (1980, 1, 1, 0, 0, 0)


class SourceValidationError(RuntimeError):
    """Raised when a tracked source package no longer matches its export."""


def fail(message: str) -> None:
    raise SourceValidationError(message)


def digest(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def png_dimensions(content: bytes, label: str) -> tuple[int, int]:
    if len(content) < 24 or content[:8] != PNG_SIGNATURE or content[12:16] != b"IHDR":
        fail(f"{label} is not a PNG with an IHDR header")
    return struct.unpack(">II", content[16:24])


def repo_path(value: str, label: str) -> Path:
    candidate = (REPO / value).resolve()
    try:
        candidate.relative_to(REPO)
    except ValueError:
        fail(f"{label} escapes the repository: {value}")
    return candidate


def display_path(path: Path) -> str:
    try:
        return path.relative_to(REPO).as_posix()
    except ValueError:
        return str(path)


def read_manifest(manifest_path: Path) -> dict[str, object]:
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {manifest_path.relative_to(REPO)}: {error}")
    if not isinstance(manifest, dict):
        fail(f"{manifest_path.relative_to(REPO)} must contain a JSON object")
    return manifest


def required_string(manifest: dict[str, object], key: str, label: str) -> str:
    value = manifest.get(key)
    if not isinstance(value, str) or not value:
        fail(f"{label} must define a non-empty {key}")
    return value


def required_int(mapping: dict[str, object], key: str, label: str) -> int:
    value = mapping.get(key)
    if not isinstance(value, int) or value <= 0:
        fail(f"{label} must define a positive integer {key}")
    return value


def source_details(manifest_path: Path) -> tuple[dict[str, object], Path, Path, bytes]:
    manifest = read_manifest(manifest_path)
    label = manifest_path.relative_to(REPO).as_posix()
    if manifest.get("schema_version") != 1:
        fail(f"{label} must use schema_version 1")
    if manifest.get("source_kind") != "recovered-fidelity-master":
        fail(f"{label} must declare source_kind recovered-fidelity-master")

    canvas = manifest.get("canvas")
    archive = manifest.get("archive")
    recovery = manifest.get("recovery")
    if not isinstance(canvas, dict) or not isinstance(archive, dict) or not isinstance(recovery, dict):
        fail(f"{label} must define canvas, archive, and recovery objects")
    width = required_int(canvas, "width", f"{label}.canvas")
    height = required_int(canvas, "height", f"{label}.canvas")
    if required_string(archive, "layer_path", f"{label}.archive") != ARCHIVE_MEMBERS[2]:
        fail(f"{label} must use {ARCHIVE_MEMBERS[2]} as its source layer")
    if required_string(archive, "composite_path", f"{label}.archive") != ARCHIVE_MEMBERS[3]:
        fail(f"{label} must use {ARCHIVE_MEMBERS[3]} as its composite")
    if recovery.get("original_editable_layers_available") is not False:
        fail(f"{label} must state that original editable layers are unavailable")

    export_path = repo_path(required_string(manifest, "canonical_export", label), f"{label}.canonical_export")
    archive_name = required_string(manifest, "source_archive", label)
    archive_path = (manifest_path.parent / archive_name).resolve()
    if archive_path.parent != manifest_path.parent.resolve() or archive_path.name != "source.ora":
        fail(f"{label} must use a sibling source.ora archive")
    if not export_path.is_file():
        fail(f"missing canonical export: {export_path.relative_to(REPO)}")

    export_content = export_path.read_bytes()
    if png_dimensions(export_content, export_path.relative_to(REPO).as_posix()) != (width, height):
        fail(f"{label} canvas does not match its canonical export dimensions")
    if digest(export_content) != required_string(manifest, "canonical_export_sha256", label):
        fail(f"{label} canonical_export_sha256 does not match its export")
    return manifest, export_path, archive_path, export_content


def stack_xml(manifest: dict[str, object]) -> bytes:
    canvas = manifest["canvas"]
    archive = manifest["archive"]
    assert isinstance(canvas, dict)
    assert isinstance(archive, dict)
    width = required_int(canvas, "width", "canvas")
    height = required_int(canvas, "height", "canvas")
    layer_name = required_string(archive, "layer_name", "archive")
    markup = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<image version="0.0.1" w="%d" h="%d">\n'
        '  <stack opacity="1" composite-op="svg:src-over">\n'
        '    <layer name="%s" src="data/fidelity-master.png" x="0" y="0" '
        'opacity="1" visibility="visible" composite-op="svg:src-over"/>\n'
        "  </stack>\n"
        "</image>\n"
    ) % (width, height, layer_name)
    return markup.encode("utf-8")


def fixed_zip_info(name: str) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, date_time=ZIP_TIMESTAMP)
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    return info


def archive_bytes(manifest: dict[str, object], export_content: bytes) -> bytes:
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        archive.writestr(fixed_zip_info("mimetype"), MIMETYPE, compress_type=zipfile.ZIP_STORED)
        archive.writestr(fixed_zip_info("stack.xml"), stack_xml(manifest))
        archive.writestr(fixed_zip_info("data/fidelity-master.png"), export_content)
        archive.writestr(fixed_zip_info("mergedimage.png"), export_content)
    return buffer.getvalue()


def write_archive(archive_path: Path, content: bytes) -> None:
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = archive_path.with_name(f".{archive_path.name}.tmp")
    temporary_path.write_bytes(content)
    temporary_path.replace(archive_path)


def verify_stack(content: bytes, manifest: dict[str, object], label: str) -> None:
    try:
        root = ET.fromstring(content)
    except ET.ParseError as error:
        fail(f"{label} stack.xml is invalid XML: {error}")
    canvas = manifest["canvas"]
    archive = manifest["archive"]
    assert isinstance(canvas, dict)
    assert isinstance(archive, dict)
    expected_size = (str(required_int(canvas, "width", "canvas")), str(required_int(canvas, "height", "canvas")))
    if root.tag != "image" or (root.get("w"), root.get("h")) != expected_size:
        fail(f"{label} stack.xml canvas does not match its manifest")
    stack = root.find("stack")
    layers = [] if stack is None else stack.findall("layer")
    if len(layers) != 1:
        fail(f"{label} must contain exactly one declared source layer")
    layer = layers[0]
    if layer.get("name") != required_string(archive, "layer_name", "archive"):
        fail(f"{label} source-layer name does not match its manifest")
    if layer.get("src") != ARCHIVE_MEMBERS[2] or layer.get("x") != "0" or layer.get("y") != "0":
        fail(f"{label} source layer must occupy the full source canvas at origin")


def verify_archive(manifest: dict[str, object], archive_path: Path, export_content: bytes) -> None:
    label = display_path(archive_path)
    if not archive_path.is_file():
        fail(f"missing source archive: {label}")
    content = archive_path.read_bytes()
    try:
        with zipfile.ZipFile(io.BytesIO(content)) as archive:
            infos = archive.infolist()
            names = tuple(info.filename for info in infos)
            if names != ARCHIVE_MEMBERS:
                fail(f"{label} must contain exactly {', '.join(ARCHIVE_MEMBERS)}")
            if infos[0].compress_type != zipfile.ZIP_STORED:
                fail(f"{label} mimetype must be stored without compression")
            corrupt_member = archive.testzip()
            if corrupt_member is not None:
                fail(f"{label} has a corrupt archive member: {corrupt_member}")
            members = {name: archive.read(name) for name in ARCHIVE_MEMBERS}
    except (OSError, zipfile.BadZipFile) as error:
        fail(f"cannot read {label}: {error}")
    if members["mimetype"] != MIMETYPE:
        fail(f"{label} has an invalid OpenRaster mimetype")
    verify_stack(members["stack.xml"], manifest, label)
    for member in ARCHIVE_MEMBERS[2:]:
        if png_dimensions(members[member], f"{label}:{member}") != png_dimensions(export_content, "canonical export"):
            fail(f"{label}:{member} dimensions differ from its canonical export")
        if members[member] != export_content:
            fail(f"{label}:{member} is not byte-identical to its canonical export")
    if content != archive_bytes(manifest, export_content):
        fail(f"{label} differs from the deterministic recovered-source archive")


def write_all() -> None:
    for relative_manifest in SOURCE_MANIFESTS:
        manifest_path = repo_path(relative_manifest, "source manifest")
        manifest, _export_path, archive_path, export_content = source_details(manifest_path)
        write_archive(archive_path, archive_bytes(manifest, export_content))
        print(f"wrote {archive_path.relative_to(REPO)}")


def check_all() -> list[dict[str, object]]:
    results: list[dict[str, object]] = []
    seen_exports: set[str] = set()
    for relative_manifest in SOURCE_MANIFESTS:
        manifest_path = repo_path(relative_manifest, "source manifest")
        manifest, export_path, archive_path, export_content = source_details(manifest_path)
        export_relative = export_path.relative_to(REPO).as_posix()
        if export_relative in seen_exports:
            fail(f"multiple source manifests map to {export_relative}")
        seen_exports.add(export_relative)
        verify_archive(manifest, archive_path, export_content)
        canvas = manifest["canvas"]
        assert isinstance(canvas, dict)
        results.append(
            {
                "id": required_string(manifest, "id", manifest_path.as_posix()),
                "canonical_export": export_relative,
                "dimensions": [required_int(canvas, "width", "canvas"), required_int(canvas, "height", "canvas")],
                "pixel_comparison": "byte-identical",
                "sha256": digest(export_content),
            }
        )
    expected_exports = {
        "brand/exports/octo-rill-product-poster.png",
        "brand/exports/octo-rill-product-poster-light.png",
        "brand/exports/octo-rill-github-social-preview.png",
        "brand/exports/octo-rill-github-social-preview-dark.png",
    }
    if seen_exports != expected_exports:
        missing = sorted(expected_exports - seen_exports)
        extra = sorted(seen_exports - expected_exports)
        fail(f"source manifest coverage mismatch; missing={missing}, extra={extra}")
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="write deterministic source.ora packages")
    parser.add_argument("--check", action="store_true", help="validate all source packages and export mappings")
    parser.add_argument("--json", action="store_true", help="emit successful check results as JSON")
    args = parser.parse_args()
    if not args.write and not args.check:
        parser.error("at least one of --write or --check is required")
    try:
        if args.write:
            write_all()
        if args.check:
            results = check_all()
            if args.json:
                print(json.dumps(results, indent=2, sort_keys=True))
            else:
                print(f"brand media sources: passed ({len(results)} packages)")
    except SourceValidationError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
