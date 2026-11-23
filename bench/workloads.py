"""Deterministic workload generator for the neolint CLI benchmark.

Every fixture is a pure function of explicit counts: no randomness, no clock and
no network. Generated files live under a caller-provided directory that is not
part of the semantic corpus manifest.

The generator only produces inputs. It never inspects the analyzer, so a
regression in the tool cannot silently change what is measured.
"""

from __future__ import annotations

import pathlib

# --- corpus classes (checked in) -------------------------------------------

CORPUS_DIR = "corpus"
CORPUS_SMALL = "corpus/small/states_and_timer.lsl"
CORPUS_MEDIUM = "corpus/medium/dialog_menu.lsl"
CORPUS_LARGE = "corpus/large/teleporter.lsl"
CORPUS_INVALID = "corpus/invalid/calls_and_assignments.lsl"


def _write(path: pathlib.Path, text: str) -> pathlib.Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    return path


def many_symbols(root: pathlib.Path, declarations: int, uses: int) -> pathlib.Path:
    """One scope with many declarations and as many reads spread across them.

    Every declaration is read, so the row does not mix lookup cost with
    unused-variable diagnostics. Reads cycle through the declarations, which
    still walks most of the symbol list for the earliest names.
    """
    lines = [f"integer value{index};" for index in range(declarations)]
    lines.append("default")
    lines.append("{")
    lines.append("    state_entry()")
    lines.append("    {")
    for index in range(uses):
        target = index % max(1, declarations)
        # A read of valueN walks the whole symbol list for small N, so the
        # workload still exercises the worst case for the earliest names.
        lines.append(f"        llOwnerSay((string)value{target});")
    lines.append("    }")
    lines.append("}")
    return _write(root / "many_symbols.lsl", "\n".join(lines) + "\n")


def call_graph(root: pathlib.Path, reachable: int, unreachable: int, fanout: int) -> pathlib.Path:
    """A typed call chain plus a disconnected chain and one wide fan-out node.

    Half of the functions are unreachable from any event, so SEM026 has real
    work to do and the graph is large but inside the documented edge budget.
    """
    lines = []
    total = reachable + unreachable
    for index in range(total):
        target = index + 1
        if target < reachable:
            lines.append(f"integer fn{index}() {{ return fn{target}(); }}")
        elif reachable <= index < total - 1:
            lines.append(f"integer fn{index}() {{ return fn{target}(); }}")
        else:
            lines.append(f"integer fn{index}() {{ return 0; }}")
    # A wide fan-out keeps the per-node edge deduplication exercised.
    lines.append("integer hub()")
    lines.append("{")
    lines.append("    integer total = 0;")
    for index in range(min(fanout, reachable)):
        lines.append(f"    total = total + fn{index}();")
    lines.append("    return total;")
    lines.append("}")
    lines.append("default")
    lines.append("{")
    lines.append("    state_entry()")
    lines.append("    {")
    lines.append("        llOwnerSay((string)fn0());")
    lines.append("        llOwnerSay((string)hub());")
    lines.append("    }")
    lines.append("}")
    return _write(root / "call_graph.lsl", "\n".join(lines) + "\n")


def nested_scopes(root: pathlib.Path, depth: int, per_level: int) -> pathlib.Path:
    """Deeply nested blocks, each declaring locals that are read inside."""
    lines = ["default", "{", "    state_entry()", "    {"]
    indent = "        "
    for level in range(depth):
        for item in range(per_level):
            lines.append(f"{indent}integer level{level}_{item};")
        lines.append(f"{indent}level{level}_0 = {level};")
        lines.append(f"{indent}llOwnerSay((string)level{level}_0);")
        lines.append(f"{indent}{{")
        indent += "    "
    lines.append(f"{indent}llOwnerSay(\"end\");")
    for _ in range(depth):
        indent = indent[:-4]
        lines.append(f"{indent}}}")
    lines.append("    }")
    lines.append("}")
    return _write(root / "nested_scopes.lsl", "\n".join(lines) + "\n")


def preprocess(root: pathlib.Path, macros: int, includes: int) -> pathlib.Path:
    """Macro expansion plus an include graph with several files."""
    includes_dir = root / "include"
    per_include = max(1, macros // max(1, includes)) if macros else 0
    generated = 0
    include_names = []
    for include_index in range(includes):
        lines = []
        for _ in range(per_include):
            # Unique names: duplicate user functions would add SEM001 noise and
            # make the preprocessing workload measure the wrong thing.
            lines.append(f"integer helper_{generated}() {{ return {generated % 97 + 1}; }}")
            generated += 1
        name = f"inc{include_index}.lsl"
        _write(includes_dir / name, "\n".join(lines) + "\n")
        include_names.append(name)
    root_lines = []
    for name in include_names:
        root_lines.append(f'#include "{name}"')
    root_lines.append("integer gStart = 0;")
    root_lines.append("default")
    root_lines.append("{")
    root_lines.append("    state_entry()")
    root_lines.append("    {")
    for index in range(generated):
        root_lines.append(f"        gStart = gStart + helper_{index}();")
    root_lines.append("        llOwnerSay((string)gStart);")
    root_lines.append("    }")
    root_lines.append("}")
    return _write(root / "preprocess_root.lsl", "\n".join(root_lines) + "\n")


def near_limit(root: pathlib.Path, statements: int) -> pathlib.Path:
    """A large but valid single file: many statements, no diagnostics."""
    lines = ["default", "{", "    state_entry()", "    {"]
    payload = "x" * 40
    for index in range(statements):
        lines.append(f'        llOwnerSay("{payload}{index % 1000}");')
    lines.append("    }")
    lines.append("}")
    return _write(root / "near_limit.lsl", "\n".join(lines) + "\n")


def malformed(root: pathlib.Path, errors: int) -> pathlib.Path:
    """Recovery-heavy input: one malformed initializer per statement."""
    lines = ["default", "{", "    state_entry()", "    {"]
    for index in range(errors):
        lines.append(f"        integer broken{index} = ;")
    lines.append("    }")
    lines.append("}")
    return _write(root / "malformed.lsl", "\n".join(lines) + "\n")


def diagnostic_flood(root: pathlib.Path, findings: int) -> pathlib.Path:
    """Undeclared references drive the rendered-diagnostic cap."""
    lines = ["default", "{", "    state_entry()", "    {"]
    for index in range(findings):
        lines.append(f"        llOwnerSay((string)undeclared_{index});")
    lines.append("    }")
    lines.append("}")
    return _write(root / "diagnostic_flood.lsl", "\n".join(lines) + "\n")


def multi_file(root: pathlib.Path, files: int) -> pathlib.Path:
    """Many independent units in one directory invocation."""
    directory = root / "multi"
    for index in range(files):
        text = (
            "integer gValue;\n"
            "default\n"
            "{\n"
            "    state_entry()\n"
            "    {\n"
            f"        gValue = {index};\n"
            '        llOwnerSay((string)gValue);\n'
            "    }\n"
            "}\n"
        )
        _write(directory / f"unit{index:03d}.lsl", text)
    return directory


def stdin_source(root: pathlib.Path) -> pathlib.Path:
    text = (
        "integer gValue;\n"
        "default\n"
        "{\n"
        "    state_entry()\n"
        "    {\n"
        "        gValue = 1;\n"
        '        llOwnerSay((string)gValue);\n'
        "    }\n"
        "}\n"
    )
    return _write(root / "stdin.lsl", text)
