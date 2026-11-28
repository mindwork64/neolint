#!/usr/bin/env python3
"""Verify the contents of NeoLint's Linux and Windows release archives."""

from __future__ import annotations

import argparse
import tarfile
import zipfile
from pathlib import Path


REQUIRED_FILES = {
    "neolint",
    "neolint-lsp",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "DATA-NOTICES.md",
    "THIRD-PARTY-NOTICES.md",
    "LICENSES/LINDEN-RESEARCH-MIT.txt",
    "LICENSES/OPENSIM-NOTICE.txt",
}


def verify_linux(path: Path) -> None:
    with tarfile.open(path, "r:gz") as archive:
        members = {member.name.removeprefix("./"): member for member in archive.getmembers()}
        missing = REQUIRED_FILES - members.keys()
        if missing:
            raise ValueError(f"{path}: missing files: {', '.join(sorted(missing))}")
        for name in REQUIRED_FILES:
            member = members[name]
            if not member.isfile() or member.size == 0:
                raise ValueError(f"{path}: required entry is not a non-empty file: {name}")
        notices = archive.extractfile(members["THIRD-PARTY-NOTICES.md"])
        if notices is None or b"License and copyright texts" not in notices.read():
            raise ValueError(f"{path}: generated third-party notices are empty or incomplete")


def verify_windows(path: Path) -> None:
    expected = {
        f"{name}.exe" if name in {"neolint", "neolint-lsp"} else name
        for name in REQUIRED_FILES
    }
    with zipfile.ZipFile(path) as archive:
        entries = {item.filename: item for item in archive.infolist()}
        missing = expected - entries.keys()
        if missing:
            raise ValueError(f"{path}: missing files: {', '.join(sorted(missing))}")
        for name in expected:
            item = entries[name]
            if item.is_dir() or item.file_size == 0:
                raise ValueError(f"{path}: required entry is not a non-empty file: {name}")
        notices = archive.read("THIRD-PARTY-NOTICES.md")
        if b"License and copyright texts" not in notices:
            raise ValueError(f"{path}: generated third-party notices are incomplete")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--linux", required=True, type=Path, help="Linux .tar.gz archive")
    parser.add_argument("--windows", required=True, type=Path, help="Windows .zip archive")
    args = parser.parse_args()

    verify_linux(args.linux)
    verify_windows(args.windows)
    print("Release archive contents: verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
