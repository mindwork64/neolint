#!/usr/bin/env python3
"""Validate the contents of a packaged NeoLint VS Code extension."""

from __future__ import annotations

import json
import sys
import zipfile
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {Path(sys.argv[0]).name} EXTENSION.vsix")

    archive_path = Path(sys.argv[1])
    with zipfile.ZipFile(archive_path) as archive:
        names = set(archive.namelist())
        required = {
            "extension/package.json",
            "extension/out/extension.js",
            "extension/node_modules/vscode-languageclient/package.json",
            "extension/LICENSE",
            "extension/LICENSE-MIT",
            "extension/LICENSE-APACHE",
        }
        missing = sorted(required - names)
        if missing:
            raise SystemExit(f"VSIX is missing required files: {', '.join(missing)}")

        manifest = json.loads(archive.read("extension/package.json"))
        if manifest.get("name") != "neolint-vscode":
            raise SystemExit("VSIX package name does not match neolint-vscode")
        if manifest.get("version") != "0.2.0":
            raise SystemExit(
                f"unexpected VSIX version: {manifest.get('version')!r}; expected 0.2.0"
            )
        client_path = "extension/node_modules/vscode-languageclient/package.json"
        client_manifest = json.loads(archive.read(client_path))
        required_client_version = manifest.get("dependencies", {}).get(
            "vscode-languageclient"
        )
        if required_client_version != "9.0.1":
            raise SystemExit(
                "VSIX manifest must pin vscode-languageclient 9.0.1; "
                f"found {required_client_version!r}"
            )
        if client_manifest.get("version") != required_client_version:
            raise SystemExit(
                "packaged vscode-languageclient version does not match the manifest: "
                f"{client_manifest.get('version')!r}"
            )
        for required_file in required:
            if archive.getinfo(required_file).file_size == 0:
                raise SystemExit(f"VSIX contains an empty required file: {required_file}")

    print(f"VSIX package smoke: {archive_path.name} contains runtime files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
