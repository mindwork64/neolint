#!/usr/bin/env python3
"""Generate runtime-specific OpenSim LSL/OSSL interface name snapshots.

The inputs are the upstream C# interface files downloaded at pinned commits.
Only API names are copied to the bundled catalogue; common LSL signatures
continue to come from the canonical Linden Lab definitions.
"""

from __future__ import annotations

import argparse
import re
from collections import defaultdict
from pathlib import Path


METHOD_DECLARATION = re.compile(
    r"^\s*[A-Za-z_][A-Za-z0-9_.<>]*\s+"
    r"((?:ll|os)[A-Za-z0-9_]+)\s*\((.*?)\)\s*;",
    re.MULTILINE | re.DOTALL,
)
CANONICAL_DECLARATION = re.compile(r"^\s*\w+\s+(ll\w+)\s*\((.*?)\)")


def strip_comments(source: str) -> str:
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.DOTALL)
    return re.sub(r"//[^\n]*", "", source)


def interface_methods(path: Path) -> list[tuple[str, list[str]]]:
    source = strip_comments(path.read_text(encoding="utf-8"))
    methods: list[tuple[str, list[str]]] = []
    for match in METHOD_DECLARATION.finditer(source):
        name, arguments = match.groups()
        parameter_types: list[str] = []
        valid = True
        for argument in arguments.split(",") if arguments.strip() else []:
            parts = argument.strip().split()
            if len(parts) != 2:
                valid = False
                break
            parameter_types.append(parts[0])
        if valid:
            methods.append((name, parameter_types))
    return methods


def canonical_key_parameters(path: Path) -> dict[str, list[tuple[str, ...]]]:
    signatures: dict[str, list[tuple[str, ...]]] = defaultdict(list)
    for line in path.read_text(encoding="utf-8").splitlines():
        match = CANONICAL_DECLARATION.match(line)
        if not match:
            continue
        name, arguments = match.groups()
        parameter_types = tuple(
            argument.strip().split()[0]
            for argument in arguments.split(",")
            if argument.strip()
        )
        signatures[name].append(parameter_types)
    return signatures


def write_names(path: Path, heading: str, methods: list[tuple[str, list[str]]]) -> int:
    names = sorted({name for name, _ in methods})
    path.write_text(
        f"# Generated from the OpenSimulator {heading} interface.\n"
        "# API names only; interface presence does not prove live region availability.\n"
        + "".join(f"{name}\n" for name in names),
        encoding="utf-8",
    )
    return len(names)


def write_key_string_parameters(
    path: Path,
    runtime: str,
    methods: list[tuple[str, list[str]]],
    canonical: dict[str, list[tuple[str, ...]]],
) -> int:
    overrides: set[tuple[str, int]] = set()
    for name, interface_types in methods:
        for index, interface_type in enumerate(interface_types):
            if interface_type == "string" and any(
                index < len(signature) and signature[index] == "key"
                for signature in canonical.get(name, [])
            ):
                overrides.add((name, index))
    path.write_text(
        "# OpenSim interface parameters declared as C# string where canonical LSL declares key.\n"
        "# Format: function-name|zero-based-parameter-index\n"
        + "".join(f"{name}|{index}\n" for name, index in sorted(overrides)),
        encoding="utf-8",
    )
    return len(overrides)


def generate(
    runtime: str,
    lsl_interface: Path,
    ossl_interface: Path,
    data_dir: Path,
    canonical: dict[str, list[tuple[str, ...]]],
) -> dict[str, int]:
    lsl_methods = interface_methods(lsl_interface)
    ossl_methods = interface_methods(ossl_interface)
    counts = {
        "ll_functions": write_names(
            data_dir / f"opensim-{runtime}-ll-functions.txt",
            "ILSL_Api.cs",
            lsl_methods,
        ),
        "ossl_functions": write_names(
            data_dir / f"opensim-{runtime}-ossl-functions.txt",
            "IOSSL_Api.cs",
            ossl_methods,
        ),
        "key_string_parameter_overrides": write_key_string_parameters(
            data_dir / f"opensim-{runtime}-ll-key-string-params.txt",
            runtime,
            lsl_methods,
            canonical,
        ),
    }
    return counts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xengine-dir", type=Path, required=True)
    parser.add_argument("--yengine-dir", type=Path, required=True)
    parser.add_argument("--data-dir", type=Path, default=Path("data"))
    parser.add_argument(
        "--canonical-lsl",
        type=Path,
        default=Path("data/lsl-builtins.txt"),
    )
    args = parser.parse_args()

    if not args.xengine_dir.is_dir() or not args.yengine_dir.is_dir():
        parser.error("both interface directories must exist")
    args.data_dir.mkdir(parents=True, exist_ok=True)
    canonical = canonical_key_parameters(args.canonical_lsl)

    for runtime, directory in (
        ("xengine", args.xengine_dir),
        ("yengine", args.yengine_dir),
    ):
        counts = generate(
            runtime,
            directory / "ILSL_Api.cs",
            directory / "IOSSL_Api.cs",
            args.data_dir,
            canonical,
        )
        print(
            f"{runtime}: {counts['ll_functions']} LSL functions, "
            f"{counts['ossl_functions']} OSSL functions, "
            f"{counts['key_string_parameter_overrides']} key/string parameter aliases"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
