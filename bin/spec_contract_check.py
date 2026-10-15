#!/usr/bin/env python3
"""Small repository-local contract check for canonical topic Specs."""

from __future__ import annotations

import argparse
import pathlib
import re
import sys


REQ = re.compile(r"\bREQ-[A-Z0-9-]+\b")
VER = re.compile(r"\bVER-[A-Z0-9-]+\b")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--path", required=True)
    parser.add_argument("--repo-root", default=".")
    args = parser.parse_args()
    path = pathlib.Path(args.path)
    if not path.is_absolute():
        path = pathlib.Path(args.repo_root) / path
    text = path.read_text(encoding="utf-8")
    requirements = REQ.findall("\n".join(line for line in text.splitlines() if re.match(r"(?:- |### )REQ-", line)))
    verifications = VER.findall("\n".join(line for line in text.splitlines() if re.match(r"(?:- |### )VER-", line)))
    errors: list[str] = []
    if not requirements:
        errors.append("no REQ-* identifiers")
    if not verifications:
        errors.append("no VER-* identifiers")
    if len(requirements) != len(set(requirements)):
        errors.append("duplicate REQ-* identifier")
    if len(verifications) != len(set(verifications)):
        errors.append("duplicate VER-* identifier")
    if "## Related ADRs" not in text:
        errors.append("missing Related ADRs section")
    if errors:
        for error in errors:
            print(f"{path}: {error}", file=sys.stderr)
        return 1
    print(f"{path}: requirements={len(requirements)} verifications={len(verifications)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
