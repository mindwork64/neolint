#!/usr/bin/env python3
"""Extract canonical deprecated LSL function names from lsl_definitions.yaml.

Only the LSL `deprecated` property inside the `functions` section is read.
The separate `slua-deprecated` property is intentionally ignored.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


FUNCTION_HEADER = re.compile(r"^  (ll[A-Za-z0-9_]+):\s*$")
DEPRECATED_FIELD = re.compile(r"^    deprecated:\s*(.*?)\s*$")
TOP_LEVEL_KEY = re.compile(r"^[a-z][a-z0-9_-]*:\s*$")


def extract_deprecated_functions(source: str) -> list[str]:
    lines = source.splitlines()
    in_functions = False
    current_name: str | None = None
    current_deprecated = False
    result: list[str] = []

    def finish_current() -> None:
        if current_name is not None and current_deprecated:
            result.append(current_name)

    for line in lines:
        if not in_functions:
            if line == "functions:":
                in_functions = True
            continue

        if TOP_LEVEL_KEY.match(line):
            finish_current()
            break

        header = FUNCTION_HEADER.match(line)
        if header:
            finish_current()
            current_name = header.group(1)
            current_deprecated = False
            continue

        if current_name is not None:
            field = DEPRECATED_FIELD.match(line)
            if field:
                value = field.group(1).strip().lower()
                current_deprecated = value != "false"

    if not in_functions:
        raise ValueError("Could not find the top-level 'functions' section")

    if len(result) != len(set(result)):
        raise ValueError("Duplicate deprecated function names found")

    return result


def main() -> int:
    if len(sys.argv) != 4:
        print(
            f"Usage: {Path(sys.argv[0]).name} INPUT_YAML OUTPUT_FILE UPSTREAM_REVISION",
            file=sys.stderr,
        )
        return 2

    source_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])
    revision = sys.argv[3]

    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        print("Upstream revision must be a 40-character lowercase SHA", file=sys.stderr)
        return 2

    try:
        names = extract_deprecated_functions(source_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError) as error:
        print(f"Cannot extract deprecated LSL functions: {error}", file=sys.stderr)
        return 1

    output = [
        "# Canonical deprecated LSL functions.",
        "# Source: secondlife/lsl-definitions/lsl_definitions.yaml",
        f"# Pinned upstream revision: {revision}",
        "# Only the LSL 'deprecated' property is included; 'slua-deprecated' is excluded.",
        "# Replacement hints from the upstream YAML are omitted because they may refer to SLua APIs.",
        *names,
        "",
    ]
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text("\n".join(output), encoding="utf-8")
    print(f"Extracted {len(names)} deprecated LSL function(s) to {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
