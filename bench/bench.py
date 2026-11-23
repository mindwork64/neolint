#!/usr/bin/env python3
"""Dependency-free end-to-end CLI benchmark harness for neolint.

Design constraints (Issue #107):

* build the release binary once; compilation is never measured;
* measure the CLI as a subprocess with a monotonic clock;
* capture exit status and SHA-256 of stdout/stderr so timing can never hide a
  correctness or determinism difference;
* record peak RSS only where the host provides a reliable child-process value
  (``os.wait4``), otherwise report it as unavailable;
* keep raw samples, metadata and a readable summary;
* never turn timing into a pass/fail threshold.

All figures are end-to-end CLI latency and include process startup. Plain
``--engine legacy`` runs a direct no-preprocess path, while AST always goes
through an identity-prepared unit, and ``--engine compare`` deliberately runs
both analyzers. Ratios between those modes are therefore not parser-only
comparisons.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import pathlib
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

REPO = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "bench"))

import workloads  # noqa: E402  (local module, resolved above)

BINARY = REPO / "target" / "release" / "neolint"
HAS_WAIT4 = hasattr(os, "wait4")
DIGEST_CHUNK = 1 << 20

# Sample counts are declared per row and echoed into the report. Large or
# near-limit workloads use fewer samples; the reason is recorded in the summary.
SAMPLES_DEFAULT = 50
SAMPLES_LARGE = 30
SAMPLES_DIR = 40
SAMPLES_SYNTHETIC = 30
SAMPLES_NEAR_LIMIT = 5
SAMPLES_FLOOD = 20


class Row:
    def __init__(self, name, argv, samples, stdin=None, note=""):
        self.name = name
        self.argv = argv
        self.samples = samples
        self.stdin = stdin
        self.note = note


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(DIGEST_CHUNK), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_child(argv, stdin_path, stdout_path, stderr_path):
    """Run one child and return (elapsed_ns, exit_code)."""
    stdin_handle = open(stdin_path, "rb") if stdin_path else subprocess.DEVNULL
    stdout_handle = open(stdout_path, "wb")
    stderr_handle = open(stderr_path, "wb")
    start = time.perf_counter_ns()
    process = subprocess.Popen(
        argv,
        stdin=stdin_handle,
        stdout=stdout_handle,
        stderr=stderr_handle,
        close_fds=True,
        cwd=str(REPO),
    )
    exit_code = process.wait()
    elapsed_ns = time.perf_counter_ns() - start
    stdout_handle.close()
    stderr_handle.close()
    if stdin_path:
        stdin_handle.close()
    return elapsed_ns, exit_code


def peak_rss_kb(argv, stdin_path):
    """Best-effort peak RSS from ``/proc/<pid>/status`` (VmHWM).

    ``ru_maxrss`` from ``os.wait4`` is not usable here: a forked/vforked child
    inherits the harness footprint before ``exec``, so every row reported the
    same ~26 MB. ``VmHWM`` is the kernel's high-water mark for the child alone.
    Returns None when the platform or timing does not expose it; the report must
    then say "unavailable" rather than guess.
    """
    if not pathlib.Path("/proc/self/status").exists():
        return None
    stdin_handle = open(stdin_path, "rb") if stdin_path else subprocess.DEVNULL
    process = subprocess.Popen(
        argv,
        stdin=stdin_handle,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        close_fds=True,
        cwd=str(REPO),
    )
    best = None
    status_path = pathlib.Path(f"/proc/{process.pid}/status")
    while process.poll() is None:
        try:
            for line in status_path.read_text().splitlines():
                if line.startswith("VmHWM:"):
                    value = int(line.split()[1])
                    best = value if best is None else max(best, value)
                    break
        except OSError:
            break
    process.wait()
    if stdin_path:
        stdin_handle.close()
    return best


def measure_rss(row, binary, repetitions=3):
    """Untimed RSS sampling so polling never perturbs the latency figures."""
    argv = [str(binary)] + row.argv
    values = [peak_rss_kb(argv, row.stdin) for _ in range(repetitions)]
    values = [value for value in values if value]
    return statistics.median(values) if values else None


def measure(row, binary, scratch):
    """Warm up, then take `row.samples` measured samples with digest checks."""
    warmups = 3
    samples = []
    with tempfile.TemporaryDirectory(dir=scratch) as temporary:
        temporary = pathlib.Path(temporary)
        stdout_path = temporary / "stdout"
        stderr_path = temporary / "stderr"
        for index in range(warmups + row.samples):
            argv = [str(binary)] + row.argv
            elapsed_ns, exit_code = run_child(argv, row.stdin, stdout_path, stderr_path)
            if index < warmups:
                continue
            samples.append(
                {
                    "ns": elapsed_ns,
                    "exit": exit_code,
                    "stdout_sha256": sha256_file(stdout_path),
                    "stderr_sha256": sha256_file(stderr_path),
                    "stdout_bytes": stdout_path.stat().st_size,
                    "stderr_bytes": stderr_path.stat().st_size,
                }
            )
    return samples


def percentile(sorted_ns, fraction):
    if not sorted_ns:
        return 0
    index = max(0, min(len(sorted_ns) - 1, int(round(fraction * (len(sorted_ns) - 1)))))
    return sorted_ns[index]


def summarise(row, pass_number, samples, rss_kb):
    timed = sorted(sample["ns"] for sample in samples)
    exits = {sample["exit"] for sample in samples}
    stdout_digests = {sample["stdout_sha256"] for sample in samples}
    stderr_digests = {sample["stderr_sha256"] for sample in samples}
    return {
        "row": row.name,
        "argv": row.argv,
        "pass": pass_number,
        "samples": len(samples),
        "note": row.note,
        "median_ms": statistics.median(timed) / 1e6,
        "p95_ms": percentile(timed, 0.95) / 1e6,
        "min_ms": timed[0] / 1e6,
        "max_ms": timed[-1] / 1e6,
        "exit": sorted(exits),
        "stdout_sha256": sorted(stdout_digests)[0] if len(stdout_digests) == 1 else None,
        "stderr_sha256": sorted(stderr_digests)[0] if len(stderr_digests) == 1 else None,
        "stdout_bytes": samples[0]["stdout_bytes"],
        "stderr_bytes": samples[0]["stderr_bytes"],
        "digest_stable": len(stdout_digests) == 1 and len(stderr_digests) == 1 and len(exits) == 1,
        "rss_kb_median": rss_kb,
        "rss_available": rss_kb is not None,
    }


def fixtures_root() -> pathlib.Path:
    return REPO / "target" / "bench-fixtures"


def build_fixtures(root: pathlib.Path) -> dict:
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    created = {}
    created["many_symbols"] = workloads.many_symbols(root, declarations=2000, uses=2000)
    created["call_graph"] = workloads.call_graph(root, reachable=800, unreachable=700, fanout=400)
    created["nested_scopes"] = workloads.nested_scopes(root, depth=40, per_level=5)
    created["preprocess"] = workloads.preprocess(root, macros=400, includes=8)
    created["near_limit"] = workloads.near_limit(root, statements=120000)
    created["malformed"] = workloads.malformed(root, errors=500)
    created["diagnostic_flood"] = workloads.diagnostic_flood(root, findings=10000)
    created["multi_file"] = workloads.multi_file(root, files=24)
    created["stdin"] = workloads.stdin_source(root)
    return created


def build_rows(fixtures: dict) -> list[Row]:
    replace = lambda path: str(path).replace(str(REPO) + os.sep, "")  # noqa: E731
    include_dir = str(fixtures["preprocess"].parent / "include")

    rows = [
        Row("startup", ["--version"], SAMPLES_DEFAULT, note="binary/startup baseline"),
    ]

    corpus_files = [
        ("corpus_small", workloads.CORPUS_SMALL, SAMPLES_DEFAULT),
        ("corpus_medium", workloads.CORPUS_MEDIUM, SAMPLES_DEFAULT),
        ("corpus_large", workloads.CORPUS_LARGE, SAMPLES_LARGE),
        ("corpus_invalid", workloads.CORPUS_INVALID, SAMPLES_DEFAULT),
    ]
    modes = [
        ("default", []),
        ("ast", ["--engine", "ast"]),
        ("legacy", ["--engine", "legacy"]),
        ("compare", ["--engine", "compare"]),
    ]
    for name, path, samples in corpus_files:
        for mode_name, flags in modes:
            rows.append(Row(f"{name}/{mode_name}", flags + [path], samples))

    for mode_name, flags in modes:
        rows.append(Row(f"corpus_dir/{mode_name}", flags + [workloads.CORPUS_DIR], SAMPLES_DIR))

    synthetic = [
        ("many_symbols", ["default", "--engine ast", "--engine legacy"]),
        ("call_graph", ["default", "--engine legacy"]),
        ("nested_scopes", ["default", "--engine legacy"]),
        ("malformed", ["default", "--engine legacy"]),
        ("multi_file", ["default", "--engine legacy"]),
    ]
    for name, mode_list in synthetic:
        for mode in mode_list:
            flags = [] if mode == "default" else mode.split()
            rows.append(
                Row(f"{name}/{mode.replace('--engine ', '')}", flags + [replace(fixtures[name])], SAMPLES_SYNTHETIC)
            )

    rows.append(
        Row(
            "near_limit/default",
            [replace(fixtures["near_limit"])],
            SAMPLES_NEAR_LIMIT,
            note="5 samples: 7.65 MiB valid file, ~0.5 s per run",
        )
    )
    rows.append(
        Row(
            "near_limit/legacy",
            ["--engine", "legacy", replace(fixtures["near_limit"])],
            SAMPLES_NEAR_LIMIT,
            note="reduced samples: multi-megabyte parse",
        )
    )
    rows.append(
        Row(
            "diagnostic_flood/default",
            [replace(fixtures["diagnostic_flood"])],
            SAMPLES_FLOOD,
            note="reduced samples: rendered-diagnostic cap",
        )
    )
    rows.append(
        Row(
            "diagnostic_flood/legacy",
            ["--engine", "legacy", replace(fixtures["diagnostic_flood"])],
            SAMPLES_FLOOD,
            note="reduced samples: rendered-diagnostic cap",
        )
    )
    rows.append(
        Row(
            "preprocess/default",
            ["--preprocess", "-I", include_dir, replace(fixtures["preprocess"])],
            SAMPLES_SYNTHETIC,
        )
    )
    rows.append(
        Row(
            "preprocess/legacy",
            ["--engine", "legacy", "--preprocess", "-I", include_dir, replace(fixtures["preprocess"])],
            SAMPLES_SYNTHETIC,
        )
    )
    rows.append(Row("stdin/default", ["-"], SAMPLES_SYNTHETIC, stdin=str(fixtures["stdin"])))
    rows.append(Row("stdin/ast", ["--engine", "ast", "-"], SAMPLES_SYNTHETIC, stdin=str(fixtures["stdin"])))
    rows.append(Row("stdin/legacy", ["--engine", "legacy", "-"], SAMPLES_SYNTHETIC, stdin=str(fixtures["stdin"])))
    return rows


def git(*args) -> str:
    return subprocess.run(
        ["git", *args], cwd=str(REPO), capture_output=True, text=True, check=True
    ).stdout.strip()


def hash_inputs(paths) -> dict:
    """Hash every benchmark input file, recursing into directories.

    Keys are repository-relative POSIX paths so the manifest is stable across
    hosts and checkout locations. Directories are never skipped: generated
    multi-file and include trees are expanded file by file.
    """
    entries = {}
    for path in paths:
        full = path if path.is_absolute() else REPO / path
        if full.is_dir():
            for child in sorted(full.rglob("*")):
                if child.is_file():
                    entries[child.relative_to(REPO).as_posix()] = {
                        "sha256": sha256_file(child),
                        "bytes": child.stat().st_size,
                    }
        elif full.is_file():
            entries[full.relative_to(REPO).as_posix()] = {
                "sha256": sha256_file(full),
                "bytes": full.stat().st_size,
            }
    return dict(sorted(entries.items()))


def signature(entry: dict) -> tuple:
    """Identity of a row's observable behaviour within one pass."""
    return (tuple(entry["exit"]), entry["stdout_sha256"], entry["stderr_sha256"])


def cross_pass_inconsistencies(summaries) -> list:
    """Rows whose exit status or output digests differ between passes.

    Within a pass the harness already rejects unstable samples. This check
    compares the per-pass signatures with each other, so a row that is stable in
    pass 1 with one output and stable in pass 2 with a different output cannot
    pass silently.
    """
    by_row = {}
    for entry in summaries:
        by_row.setdefault(entry["row"], {})[entry["pass"]] = signature(entry)
    problems = []
    for row, per_pass in sorted(by_row.items()):
        if len(per_pass) < 2:
            continue
        signatures = {value for value in per_pass.values()}
        if len(signatures) > 1:
            problems.append(
                {"row": row, "passes": {str(pass_number): value for pass_number, value in per_pass.items()}}
            )
    return problems


def git_show(commit: str, format_string: str) -> str | None:
    result = subprocess.run(
        ["git", "show", "-s", f"--format={format_string}", commit],
        cwd=str(REPO),
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def verify_build(commit: str, expected_sha256: str, scratch: pathlib.Path) -> dict:
    """Rebuild `commit` in a throwaway worktree and compare the binary digest.

    This is what proves the measured binary really corresponds to the recorded
    source commit: the harness itself checks out the commit, builds it and hashes
    the result. It needs git and cargo on the host and is therefore opt-in.
    """
    worktree = scratch / f"verify-{commit[:12]}"
    if worktree.exists():
        shutil.rmtree(worktree)
    result = {"attempted": True, "commit": commit, "rebuilt_sha256": None, "matched": False, "error": None}
    added = subprocess.run(
        ["git", "worktree", "add", "--detach", str(worktree), commit],
        cwd=str(REPO),
        capture_output=True,
        text=True,
    )
    if added.returncode != 0:
        result["error"] = added.stderr.strip()[-400:]
        return result
    try:
        status = subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=str(worktree),
            capture_output=True,
            text=True,
        )
        if status.stdout.strip():
            result["error"] = "verification worktree is not clean"
            return result
        build = subprocess.run(
            ["cargo", "build", "--release", "--bin", "neolint"],
            cwd=str(worktree),
            capture_output=True,
            text=True,
        )
        if build.returncode != 0:
            result["error"] = build.stderr.strip()[-400:]
            return result
        rebuilt = worktree / "target" / "release" / "neolint"
        result["rebuilt_sha256"] = sha256_file(rebuilt)
        result["matched"] = result["rebuilt_sha256"] == expected_sha256
    finally:
        subprocess.run(
            ["git", "worktree", "remove", "--force", str(worktree)],
            cwd=str(REPO),
            capture_output=True,
            text=True,
        )
        subprocess.run(["git", "worktree", "prune"], cwd=str(REPO), capture_output=True, text=True)
    return result


def metadata(fixtures: dict, rows: list[Row], binary, binary_provenance: dict) -> dict:
    rustc = subprocess.run(["rustc", "-Vv"], capture_output=True, text=True, check=False)
    cargo = subprocess.run(["cargo", "-V"], capture_output=True, text=True, check=False)
    cpu_model = "unknown"
    try:
        for line in pathlib.Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu_model = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    inputs = hash_inputs([REPO / workloads.CORPUS_DIR, fixtures_root()])
    return {
        "issue": "https://github.com/mindwork64/neolint/issues/107",
        "harness_commit": git("rev-parse", "HEAD"),
        "harness_tree": git("rev-parse", "HEAD^{tree}"),
        "harness_branch": git("rev-parse", "--abbrev-ref", "HEAD"),
        "binary_source_commit": binary_provenance.get("commit"),
        "binary_source_tree": binary_provenance.get("tree"),
        "binary_sha256_verified": binary_provenance.get("sha256_verified", False),
        "binary_source_build_verified": binary_provenance["build_verification"]["matched"],
        "binary_source_rebuilt_sha256": binary_provenance["build_verification"]["rebuilt_sha256"],
        "binary_sha256_expected": binary_provenance.get("expected_sha256"),
        "rustc": rustc.stdout.strip() if rustc.returncode == 0 else "unavailable",
        "cargo": cargo.stdout.strip() if cargo.returncode == 0 else "unavailable",
        "python": platform.python_version(),
        "os": platform.platform(),
        "kernel": platform.release(),
        "machine": platform.machine(),
        "cpu_model": cpu_model,
        "cpu_count": os.cpu_count(),
        "binary": str(binary),
        "binary_sha256": sha256_file(binary) if binary.exists() else None,
        "binary_bytes": binary.stat().st_size if binary.exists() else None,
        "profile": "release",
        "features": "default",
        "rss_method": "proc_vmhwm_poll" if pathlib.Path("/proc/self/status").exists() else "unavailable",
        "rss_available": pathlib.Path("/proc/self/status").exists(),
        "rss_samples_per_row": 3,
        "warmup_iterations": 3,
        "inputs": inputs,
        "input_file_count": len(inputs),
        "input_total_bytes": sum(entry["bytes"] for entry in inputs.values()),
        "rows": [
            {"row": row.name, "argv": row.argv, "samples": row.samples, "note": row.note}
            for row in rows
        ],
    }


def write_summary(path, meta, summaries, variance):
    lines = []
    lines.append("# neolint CLI benchmark — Phase 8 baseline\n")
    lines.append(
        f"- harness revision: `{meta['harness_commit']}` (tree `{meta['harness_tree']}`), "
        f"branch `{meta['harness_branch']}`"
    )
    if meta["binary_source_build_verified"]:
        verification = (
            "digest matches the expected value; a fresh build of the same commit "
            "reproduced the same digest"
        )
    elif meta["binary_sha256_verified"]:
        verification = "digest matches the expected value; source-commit rebuild not requested"
    else:
        verification = "digest not checked against an expected value"
    lines.append(
        f"- measured binary: built from `{meta['binary_source_commit']}` "
        f"(tree `{meta['binary_source_tree']}`), sha256 `{meta['binary_sha256']}`, {verification}"
    )
    lines.append(
        f"- benchmark inputs: {meta['input_file_count']} files, "
        f"{meta['input_total_bytes']} bytes (per-file hashes in `meta.json`)"
    )
    lines.append(f"- rustc `{meta['rustc'].splitlines()[0] if meta['rustc'] else 'n/a'}`, cargo `{meta['cargo']}`")
    lines.append(f"- host: {meta['os']} / {meta['machine']} / {meta['cpu_model']} / {meta['cpu_count']} threads")
    lines.append(f"- binary path: `{meta['binary']}` ({meta['binary_bytes']} bytes)")
    lines.append(f"- warm-up: {meta['warmup_iterations']} iterations per row; two independent passes")
    lines.append(
        f"- RSS: {'peak VmHWM polled from /proc, 3 untimed samples per row' if meta['rss_available'] else 'unavailable on this host'}"
    )
    lines.append("")
    lines.append("All figures are **end-to-end CLI** measurements and include process startup.")
    lines.append("Plain `--engine legacy` is a direct no-preprocess path; AST always runs an")
    lines.append("identity-prepared unit; `--engine compare` runs both analyzers. Ratios")
    lines.append("between those modes are not parser-only comparisons.")
    lines.append("")
    lines.append("| Row | Pass | Samples | Median ms | p95 ms | Min ms | Max ms | Exit | RSS KB | Digests |")
    lines.append("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for entry in summaries:
        digests = "stable" if entry["digest_stable"] else "**UNSTABLE**"
        rss = entry["rss_kb_median"] if entry["rss_kb_median"] is not None else "n/a"
        exit_text = ",".join(str(code) for code in entry["exit"])
        lines.append(
            f"| `{entry['row']}` | {entry['pass']} | {entry['samples']} | "
            f"{entry['median_ms']:.3f} | {entry['p95_ms']:.3f} | {entry['min_ms']:.3f} | "
            f"{entry['max_ms']:.3f} | {exit_text} | {rss} | {digests} |"
        )
    lines.append("")
    lines.append("## Two-pass variation (median)\n")
    lines.append("| Row | Pass 1 ms | Pass 2 ms | Delta % |")
    lines.append("| --- | --- | --- | --- |")
    for name in sorted(variance):
        first, second = variance[name]
        delta = abs(second - first) / min(first, second) * 100 if min(first, second) else 0.0
        lines.append(f"| `{name}` | {first:.3f} | {second:.3f} | {delta:.1f} |")
    lines.append("")
    lines.append("## Notes\n")
    lines.append("- Sample counts are declared per row in `meta.json`; reduced counts are noted in the table above via `meta.json` notes.")
    lines.append("- Oversized or near-limit workloads use fewer samples to keep a full pass practical; the exact counts are recorded.")
    lines.append("- Digests are SHA-256 of stdout and stderr; a row with unstable digests must be treated as a correctness/determinism failure, not averaged away.")
    lines.append("- Timing is never a pass/fail threshold in CI.")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv=None):
    parser = argparse.ArgumentParser(description="neolint CLI benchmark harness")
    parser.add_argument("--passes", type=int, default=2)
    parser.add_argument("--quick", action="store_true", help="smoke run with tiny sample counts")
    parser.add_argument("--self-test", action="store_true", help="verify the harness itself")
    parser.add_argument("--out", default=None)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument(
        "--binary",
        default=None,
        help="measure an existing binary instead of building target/release/neolint",
    )
    parser.add_argument(
        "--binary-source-commit",
        default=None,
        help="commit the measured binary was built from (defaults to the harness commit)",
    )
    parser.add_argument(
        "--expect-binary-sha256",
        default=None,
        help="refuse to measure unless the binary has this SHA-256",
    )
    parser.add_argument(
        "--verify-build",
        action="store_true",
        help="rebuild the binary source commit in a throwaway worktree and compare SHA-256",
    )
    args = parser.parse_args(argv)

    if not args.skip_build:
        build = subprocess.run(
            ["cargo", "build", "--release", "--bin", "neolint"],
            cwd=str(REPO),
            capture_output=True,
            text=True,
        )
        if build.returncode != 0:
            print(build.stderr[-2000:], file=sys.stderr)
            return 1
    binary = pathlib.Path(args.binary).resolve() if args.binary else BINARY
    if not binary.exists():
        print(f"binary missing: {binary}", file=sys.stderr)
        return 1

    binary_sha = sha256_file(binary)
    if args.expect_binary_sha256 and binary_sha != args.expect_binary_sha256:
        print(
            f"binary SHA-256 mismatch: expected {args.expect_binary_sha256}, got {binary_sha}",
            file=sys.stderr,
        )
        return 5
    source_commit = args.binary_source_commit or git("rev-parse", "HEAD")
    source_tree = git_show(source_commit, "%T")
    provenance = {
        "commit": source_commit,
        "tree": source_tree,
        "sha256": binary_sha,
        "expected_sha256": args.expect_binary_sha256,
        # This flag only means "the measured binary matches the digest the caller
        # passed on the command line". It says nothing about which source commit
        # produced it; that is what --verify-build proves.
        "sha256_verified": bool(args.expect_binary_sha256) and binary_sha == args.expect_binary_sha256,
        "build_verification": {"attempted": False, "matched": False, "rebuilt_sha256": None, "error": None},
    }
    scratch_dir = REPO / "target" / "bench-scratch"
    scratch_dir.mkdir(parents=True, exist_ok=True)
    if args.verify_build:
        provenance["build_verification"] = verify_build(source_commit, binary_sha, scratch_dir)
        if not provenance["build_verification"]["matched"]:
            print(
                "binary source verification failed: "
                f"{provenance['build_verification']}",
                file=sys.stderr,
            )
            return 6
        print(
            "binary source verified: freshly built "
            f"{source_commit[:12]} reproduces {binary_sha[:16]}",
            flush=True,
        )

    fixtures = build_fixtures(fixtures_root())
    rows = build_rows(fixtures)
    if args.quick:
        rows = [row for row in rows if row.name in {"startup", "corpus_small/default", "stdin/default"}]
        for row in rows:
            row.samples = 5

    scratch = REPO / "target" / "bench-scratch"
    scratch.mkdir(parents=True, exist_ok=True)

    summaries = []
    raw_lines = []
    unstable = []
    for pass_number in range(1, args.passes + 1):
        for row in rows:
            samples = measure(row, binary, scratch)
            rss_kb = measure_rss(row, binary)
            entry = summarise(row, pass_number, samples, rss_kb)
            summaries.append(entry)
            if not entry["digest_stable"]:
                unstable.append(entry)
            for index, sample in enumerate(samples):
                raw_lines.append(
                    json.dumps(
                        {
                            "pass": pass_number,
                            "row": row.name,
                            "sample": index,
                            "ns": sample["ns"],
                            "exit": sample["exit"],
                            "stdout_sha256": sample["stdout_sha256"],
                            "stderr_sha256": sample["stderr_sha256"],
                            "stdout_bytes": sample["stdout_bytes"],
                            "stderr_bytes": sample["stderr_bytes"],
                        },
                        sort_keys=True,
                    )
                )
            print(
                f"pass {pass_number} {row.name}: median={entry['median_ms']:.3f} ms "
                f"p95={entry['p95_ms']:.3f} ms exit={entry['exit']} "
                f"digests={'stable' if entry['digest_stable'] else 'UNSTABLE'}",
                flush=True,
            )

    inconsistencies = cross_pass_inconsistencies(summaries)
    meta = metadata(fixtures, rows, binary, provenance)
    meta["cross_pass_consistent"] = not inconsistencies
    meta["cross_pass_inconsistencies"] = inconsistencies
    variance = {}
    for pass_number in (1, 2):
        for entry in summaries:
            if entry["pass"] != pass_number:
                continue
            if entry["row"] == "startup":
                continue
            pair = variance.setdefault(entry["row"], [None, None])
            pair[pass_number - 1] = entry["median_ms"]
    variance = {name: pair for name, pair in variance.items() if None not in pair}

    default_name = meta["harness_commit"][:12]
    if args.binary:
        default_name = f"{default_name}-vs-{meta['binary_sha256'][:8]}"
    out_dir = pathlib.Path(args.out) if args.out else REPO / "bench" / "results" / default_name
    out_dir.mkdir(parents=True, exist_ok=True)
    raw_text = "\n".join(raw_lines) + "\n"
    (out_dir / "raw.jsonl").write_text(raw_text, encoding="utf-8")
    with gzip.open(out_dir / "raw.jsonl.gz", "wb", compresslevel=9) as compressed:
        compressed.write(raw_text.encode("utf-8"))
    (out_dir / "meta.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    write_summary(out_dir / "summary.md", meta, summaries, variance)

    try:
        shown = out_dir.resolve().relative_to(REPO)
    except ValueError:
        shown = out_dir
    print(f"\nwrote {shown}")
    if inconsistencies:
        print(
            f"CROSS-PASS MISMATCH in {len(inconsistencies)} row(s): "
            f"{[entry['row'] for entry in inconsistencies]}",
            file=sys.stderr,
        )
        return 7
    if unstable:
        print(f"UNSTABLE DIGESTS in {len(unstable)} row(s): {[row['row'] for row in unstable]}", file=sys.stderr)
        return 4
    if args.self_test:
        # Self-test only checks that the harness observed stable digests and
        # recorded timing; it never asserts a latency value.
        print("self-test: digest stability verified; no timing threshold applied")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
