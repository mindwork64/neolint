#!/usr/bin/env python3
"""Generate pinned, version-specific OpenSimulator API snapshots.

Each profile source directory contains ILSL_Api.cs, IOSSL_Api.cs, and
LSL_Constants.cs from one OpenSim source revision. XEngine events are extracted
from lsl.lexer.cs and lsl.parser.cs; YEngine events are extracted from
MMRIEventHandlers.cs and MMRScriptEventCode.cs. No network access or third-party
Python packages are used by this generator.
"""

from __future__ import annotations

import argparse
import re
from collections import defaultdict
from pathlib import Path
from typing import Iterable

TYPE_MAP = {
    "void": "void",
    "LSL_Integer": "integer",
    "LSLInteger": "integer",
    "int": "integer",
    "LSL_Float": "float",
    "double": "float",
    "LSL_String": "string",
    "string": "string",
    "LSL_Key": "key",
    "key": "key",
    "LSL_List": "list",
    "LSL_Vector": "vector",
    "vector": "vector",
    "LSL_Types.Vector3": "vector",
    "LSL_Rotation": "rotation",
    "rotation": "rotation",
    "Quaternion": "rotation",
    "bool": "integer",
}


def strip_comments(source: str) -> str:
    """Remove C# comments without deleting // or /* inside string literals."""
    out: list[str] = []
    i = 0
    state = "code"
    while i < len(source):
        char = source[i]
        nxt = source[i + 1] if i + 1 < len(source) else ""
        if state == "code":
            if char == '"':
                state = "string"
                out.append(char)
                i += 1
                continue
            if char == "'":
                state = "char"
                out.append(char)
                i += 1
                continue
            if char == "/" and nxt == "/":
                state = "line-comment"
                out.extend("  ")
                i += 2
                continue
            if char == "/" and nxt == "*":
                state = "block-comment"
                out.extend("  ")
                i += 2
                continue
            out.append(char)
            i += 1
            continue
        if state == "string" or state == "char":
            out.append(char)
            if char == "\\" and i + 1 < len(source):
                out.append(source[i + 1])
                i += 2
            else:
                if (state == "string" and char == '"') or (state == "char" and char == "'"):
                    state = "code"
                i += 1
            continue
        if state == "line-comment":
            if char == "\n":
                out.append(char)
                state = "code"
            else:
                out.append(" ")
            i += 1
            continue
        if state == "block-comment":
            if char == "*" and nxt == "/":
                out.extend("  ")
                state = "code"
                i += 2
            else:
                out.append("\n" if char == "\n" else " ")
                i += 1
    return "".join(out)


def split_arguments(source: str) -> list[str]:
    parts: list[str] = []
    start = 0
    paren = bracket = angle = 0
    in_string = False
    escaped = False
    for i, char in enumerate(source):
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
        elif char == "(":
            paren += 1
        elif char == ")":
            paren -= 1
        elif char == "[":
            bracket += 1
        elif char == "]":
            bracket -= 1
        elif char == "<":
            angle += 1
        elif char == ">" and angle:
            angle -= 1
        elif char == "," and not (paren or bracket or angle):
            parts.append(source[start:i].strip())
            start = i + 1
    tail = source[start:].strip()
    if tail:
        parts.append(tail)
    return parts


def canonical_signatures(path: Path) -> dict[str, list[tuple[str, list[str]]]]:
    signatures: dict[str, list[tuple[str, list[str]]]] = defaultdict(list)
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        match = re.match(r"(\w+)\s+(ll\w+)\s*\((.*?)\)", line)
        if not match:
            continue
        ret, name, args = match.groups()
        params = []
        for part in split_arguments(args):
            fields = part.split()
            if fields:
                params.append(fields[0])
        signatures[name].append((ret, params))
    return signatures


def canonical_event_signatures(path: Path) -> dict[str, list[str]]:
    signatures: dict[str, list[str]] = defaultdict(list)
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        match = re.match(r"event\s+([a-z][a-z0-9_]*)\s*\(", line)
        if match:
            signatures[match.group(1)].append(line)
    return signatures


def parse_xengine_event_catalogue(
    lexer_path: Path,
    parser_path: Path,
    profile: str,
    canonical_events: dict[str, list[str]],
) -> dict[str, str]:
    lexer_source = strip_comments(lexer_path.read_text(encoding="utf-8"))
    parser_source = strip_comments(parser_path.read_text(encoding="utf-8"))
    event_tokens = set(
        re.findall(
            r"public\s+class\s+([A-Z][A-Z0-9_]*_EVENT)\s*:\s*TOKEN",
            lexer_source,
        )
    )
    parser_tokens = set(
        re.findall(
            r"\(\(\s*([A-Z][A-Z0-9_]*_EVENT)\s*\)",
            parser_source,
        )
    )
    if event_tokens != parser_tokens:
        missing = sorted(event_tokens - parser_tokens)
        extra = sorted(parser_tokens - event_tokens)
        raise ValueError(
            f"{profile}: lexer/parser event mismatch; unparsed={missing}, unknown={extra}"
        )

    events: dict[str, str] = {}
    for token in sorted(event_tokens):
        name = token.removesuffix("_EVENT").lower()
        signatures = canonical_events.get(name)
        if not signatures:
            raise ValueError(
                f"{profile}: event {name!r} has no canonical LSL signature; "
                "add an explicit mapping rather than guessing"
            )
        events[name] = signatures[0]
    return dict(sorted(events.items()))


def mapped_type(csharp_type: str, profile: str, name: str, index: int,
                canonical: dict[str, list[tuple[str, list[str]]]]) -> str:
    result = TYPE_MAP.get(csharp_type)
    if result is None:
        raise ValueError(f"{profile}: unsupported C# type {csharp_type!r} in {name} parameter {index + 1}")
    # OpenSim exposes LSL key as a string wrapper. Preserve the language's key
    # type in common SL APIs where Linden's signature declares key.
    if csharp_type in {"string", "LSL_String"}:
        if any(index < len(params) and params[index] == "key" for _, params in canonical.get(name, [])):
            return "key"
    return result


def parse_methods(path: Path, prefix: str, profile: str,
                  canonical: dict[str, list[tuple[str, list[str]]]]) -> dict[str, list[str]]:
    source = strip_comments(path.read_text(encoding="utf-8"))
    pattern = re.compile(
        r"^\s*(?:(?:public|private|protected|internal)\s+)?"
        r"([A-Za-z_][A-Za-z0-9_.<>]*)\s+(" + re.escape(prefix)
        + r"[A-Za-z0-9_]+)\s*\((.*?)\)\s*;",
        re.MULTILINE | re.DOTALL,
    )
    signatures: dict[str, set[str]] = defaultdict(set)
    for match in pattern.finditer(source):
        return_type, name, raw_args = match.groups()
        if return_type not in TYPE_MAP:
            raise ValueError(f"{profile}: unsupported return type {return_type!r} in {name}")
        parameters = []
        for index, raw in enumerate(split_arguments(raw_args)):
            fields = raw.split()
            if len(fields) < 2:
                raise ValueError(f"{profile}: cannot parse parameter {raw!r} in {name}")
            while fields and fields[0] in {"ref", "out", "in", "params", "this"}:
                fields.pop(0)
            if len(fields) < 2:
                raise ValueError(f"{profile}: cannot parse parameter {raw!r} in {name}")
            csharp_type, parameter_name = fields[-2], fields[-1]
            type_name = mapped_type(csharp_type, profile, name, index, canonical)
            parameters.append((type_name, parameter_name))
        if name in canonical and canonical[name]:
            # Keep Linden's return type when OpenSim's wrapper erases key to string.
            canonical_return = canonical[name][0][0]
            if return_type in {"string", "LSL_String"} and canonical_return == "key":
                mapped_return = "key"
            else:
                mapped_return = TYPE_MAP[return_type]
        else:
            mapped_return = TYPE_MAP[return_type]
        arguments = ", ".join(f"{type_name} {parameter_name}" for type_name, parameter_name in parameters)
        signatures[name].add(f"{mapped_return} {name}({arguments})")
    return {name: sorted(rows) for name, rows in sorted(signatures.items())}


def parse_constants(path: Path, profile: str) -> dict[str, str]:
    source = strip_comments(path.read_text(encoding="utf-8"))
    pattern = re.compile(
        r"\bpublic\s+(?:(?:static\s+readonly)|const)\s+"
        r"([A-Za-z_][A-Za-z0-9_.]*)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([^;]+);"
    )
    constants: dict[str, str] = {}
    for match in pattern.finditer(source):
        csharp_type, name, value = match.groups()
        if csharp_type not in TYPE_MAP:
            raise ValueError(f"{profile}: unsupported constant type {csharp_type!r} for {name}")
        value = value.strip()
        if not value:
            raise ValueError(f"{profile}: empty constant value for {name}")
        constants[name] = f"const {TYPE_MAP[csharp_type]} {name} = {value}"
    return dict(sorted(constants.items()))


def parse_event_catalogue(
    handlers_path: Path,
    event_codes_path: Path,
    profile: str,
    canonical_events: dict[str, list[str]],
) -> dict[str, str]:
    handlers_source = strip_comments(handlers_path.read_text(encoding="utf-8"))
    event_codes_source = strip_comments(event_codes_path.read_text(encoding="utf-8"))
    supported = set(
        re.findall(
            r"^\s*([a-z][a-z0-9_]*)\s*=\s*\d+\s*,?\s*$",
            event_codes_source,
            re.MULTILINE,
        )
    )
    pattern = re.compile(
        r"^\s*void\s+([a-z][a-z0-9_]*)\s*\((.*?)\)\s*;",
        re.MULTILINE | re.DOTALL,
    )
    events: dict[str, str] = {}
    for match in pattern.finditer(handlers_source):
        name, raw_args = match.groups()
        if name not in supported:
            continue
        parameters = []
        for raw in split_arguments(raw_args):
            fields = raw.split()
            if len(fields) < 2:
                raise ValueError(f"{profile}: cannot parse event parameter {raw!r} in {name}")
            while fields and fields[0] in {"ref", "out", "in", "params"}:
                fields.pop(0)
            csharp_type, parameter_name = fields[-2], fields[-1]
            type_name = TYPE_MAP.get(csharp_type)
            if type_name is None:
                raise ValueError(
                    f"{profile}: unsupported event parameter type {csharp_type!r} in {name}"
                )
            parameters.append((type_name, parameter_name))

        # Canonical LSL declarations carry language-level types, including key
        # where the C# handler interface uses the key wrapper's string type.
        declarations = canonical_events.get(name, [])
        if declarations:
            declaration = declarations[0]
            open_paren = declaration.find("(")
            close_paren = declaration.rfind(")")
            if open_paren >= 0 and close_paren > open_paren:
                canonical_types = [
                    part.split()[0]
                    for part in split_arguments(declaration[open_paren + 1:close_paren])
                    if part.split()
                ]
                if len(canonical_types) == len(parameters):
                    parameters = [
                        (canonical_types[index], parameter_name)
                        for index, (_, parameter_name) in enumerate(parameters)
                    ]
        arguments = ", ".join(f"{kind} {parameter}" for kind, parameter in parameters)
        events[name] = f"event {name}({arguments})"

    missing = sorted(supported - set(events))
    if missing:
        raise ValueError(
            f"{profile}: event codes lack handler declarations: {', '.join(missing)}"
        )
    return dict(sorted(events.items()))


def write_catalogue(path: Path, comment: str, rows: Iterable[str]) -> None:
    path.write_text(comment + "\n" + "\n".join(rows) + "\n", encoding="utf-8")


def generate(profile: str, source_dir: Path, output_dir: Path,
             canonical_file: Path) -> dict[str, int]:
    required = ["ILSL_Api.cs", "IOSSL_Api.cs", "LSL_Constants.cs"]
    is_xengine = profile.startswith("xengine-")
    if is_xengine:
        required.extend(["lsl.lexer.cs", "lsl.parser.cs"])
    else:
        required.extend(["MMRIEventHandlers.cs", "MMRScriptEventCode.cs"])
    missing = [name for name in required if not (source_dir / name).is_file()]
    if missing:
        raise FileNotFoundError(f"{profile}: missing source files: {', '.join(missing)}")
    canonical = canonical_signatures(canonical_file)
    canonical_events = canonical_event_signatures(canonical_file)
    functions = parse_methods(source_dir / "ILSL_Api.cs", "ll", profile, canonical)
    ossl = parse_methods(source_dir / "IOSSL_Api.cs", "os", profile, canonical)
    constants = parse_constants(source_dir / "LSL_Constants.cs", profile)
    if is_xengine:
        events = parse_xengine_event_catalogue(
            source_dir / "lsl.lexer.cs",
            source_dir / "lsl.parser.cs",
            profile,
            canonical_events,
        )
    else:
        events = parse_event_catalogue(
            source_dir / "MMRIEventHandlers.cs",
            source_dir / "MMRScriptEventCode.cs",
            profile,
            canonical_events,
        )
    output_dir.mkdir(parents=True, exist_ok=True)
    write_catalogue(output_dir / "functions.txt",
                    f"// Generated from {profile} ILSL_Api.cs; retain all overloads.",
                    (row for values in functions.values() for row in values))
    write_catalogue(output_dir / "ossl-functions.txt",
                    f"// Generated from {profile} IOSSL_Api.cs; retain all overloads.",
                    (row for values in ossl.values() for row in values))
    event_source = (
        "lsl.lexer.cs + lsl.parser.cs"
        if is_xengine
        else "MMRScriptEventCode.cs + MMRIEventHandlers.cs"
    )
    write_catalogue(output_dir / "events.txt",
                    f"// Generated from {profile} {event_source}.",
                    events.values())
    write_catalogue(output_dir / "constants.txt",
                    f"// Generated from {profile} LSL_Constants.cs.",
                    constants.values())
    return {
        "functions": len(functions),
        "function_signatures": sum(map(len, functions.values())),
        "ossl_functions": len(ossl),
        "ossl_signatures": sum(map(len, ossl.values())),
        "events": len(events),
        "constants": len(constants),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--profile", action="append", required=True, metavar="NAME=SOURCE_DIR",
        help="OpenSim snapshot profile and directory containing the upstream C# sources.",
    )
    parser.add_argument("--data-dir", type=Path, default=Path("data/opensim"))
    parser.add_argument("--canonical-lsl", type=Path, default=Path("data/lsl-builtins.txt"))
    args = parser.parse_args()
    for value in args.profile:
        if "=" not in value:
            parser.error(f"invalid --profile {value!r}; expected NAME=SOURCE_DIR")
        name, source_dir = value.split("=", 1)
        if not re.fullmatch(r"[a-z0-9-]+", name):
            parser.error(f"invalid profile name {name!r}")
        counts = generate(name, Path(source_dir), args.data_dir / name, args.canonical_lsl)
        print(name + ": " + ", ".join(f"{key}={value}" for key, value in counts.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
