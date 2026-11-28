#!/usr/bin/env python3
"""Generate complete license notices for the locked Rust dependency graph."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path
from typing import Any


def cargo_packages(root: Path) -> list[dict[str, Any]]:
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--all-features"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    return sorted(
        (package for package in metadata["packages"] if package.get("source")),
        key=lambda package: (package["name"], package["version"]),
    )


def license_files(package: dict[str, Any]) -> list[Path]:
    manifest = Path(package["manifest_path"])
    package_dir = manifest.parent
    selected: list[Path] = []
    explicit = package.get("license_file")
    if explicit:
        path = (package_dir / explicit).resolve()
        if path.is_file():
            selected.append(path)

    for candidate in package_dir.iterdir():
        if not candidate.is_file():
            continue
        name = candidate.name.upper()
        if any(token in name for token in ("LICENSE", "COPYING", "UNLICENSE", "NOTICE", "COPYRIGHT")):
            selected.append(candidate)

    unique = {path.resolve() for path in selected}
    return sorted(unique, key=lambda path: path.name.lower())


def generate(root: Path) -> str:
    packages = cargo_packages(root)
    if not packages:
        raise RuntimeError("Cargo metadata returned no third-party packages")

    package_records: list[tuple[dict[str, Any], list[tuple[str, str]]]] = []
    unique_texts: dict[str, tuple[str, str]] = {}

    for package in packages:
        paths = license_files(package)
        if not paths:
            raise RuntimeError(
                f"no license/notice files found for {package['name']} {package['version']}"
            )

        references: list[tuple[str, str]] = []
        for path in paths:
            content = path.read_text(encoding="utf-8", errors="replace").strip()
            if not content:
                raise RuntimeError(f"empty license/notice file: {path}")
            digest = hashlib.sha256(content.encode("utf-8")).hexdigest()
            if digest not in unique_texts:
                unique_texts[digest] = (
                    f"{package['name']} {package['version']} / {path.name}",
                    content,
                )
            references.append((path.name, digest))

        package_records.append((package, references))

    lines = [
        "# Third-party notices",
        "",
        "This file is generated from the locked Cargo dependency graph by",
        "scripts/generate-third-party-notices.py. It includes the license and",
        "copyright/notice files shipped with each third-party Rust package.",
        "Dependency versions are fixed by Cargo.lock.",
        "",
        "NeoLint's own source code is separately licensed under MIT OR Apache-2.0.",
        "Vendored Second Life and OpenSimulator API metadata has separate notices",
        "in LICENSES/ and is not included in the Rust package list below.",
        "",
        "## Dependency index",
        "",
        "| Package | Version | Declared license | Included files |",
        "| --- | ---: | --- | --- |",
    ]

    digest_labels = {
        digest: f"notice-{index:03d}"
        for index, digest in enumerate(unique_texts, start=1)
    }
    for package, references in package_records:
        file_labels = ", ".join(
            f"{name} (see {digest_labels[digest]})" for name, digest in references
        )
        lines.append(
            f"| {package['name']} | {package['version']} | "
            f"{package.get('license') or 'UNSPECIFIED'} | {file_labels} |"
        )

    lines.extend(["", "## License and copyright texts", ""])
    for digest, (origin, content) in unique_texts.items():
        anchor = digest_labels[digest]
        lines.extend(
            [
                f"### {anchor}: {origin}",
                "",
                "~~~text",
                content,
                "~~~",
                "",
                "---",
                "",
            ]
        )

    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        required=True,
        help="destination Markdown file for the generated notices",
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    content = generate(root)
    output.write_text(content, encoding="utf-8")
    print(f"generated {output} ({len(content)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
