"""Unit tests for the Phase 8 benchmark harness helpers.

The harness is a script rather than a package, so it is loaded by path. These
tests cover the deterministic bookkeeping that CI can check without running a
benchmark: cross-pass signature comparison and the provenance field semantics.
"""

import importlib.util
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bench"))

_SPEC = importlib.util.spec_from_file_location("neolint_bench_harness", ROOT / "bench" / "bench.py")
bench = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(bench)


def entry(row, pass_number, exit_codes=(0,), stdout="out", stderr="err"):
    return {
        "row": row,
        "pass": pass_number,
        "exit": list(exit_codes),
        "stdout_sha256": stdout,
        "stderr_sha256": stderr,
    }


class CrossPassConsistencyTests(unittest.TestCase):
    def test_identical_passes_are_consistent(self):
        self.assertEqual(
            bench.cross_pass_inconsistencies([entry("row", 1), entry("row", 2)]), []
        )

    def test_stdout_mismatch_between_passes_is_reported(self):
        problems = bench.cross_pass_inconsistencies(
            [entry("row", 1, stdout="first"), entry("row", 2, stdout="second")]
        )
        self.assertEqual(len(problems), 1)
        self.assertEqual(problems[0]["row"], "row")

    def test_stderr_mismatch_between_passes_is_reported(self):
        problems = bench.cross_pass_inconsistencies(
            [entry("row", 1, stderr="first"), entry("row", 2, stderr="second")]
        )
        self.assertEqual(len(problems), 1)

    def test_exit_code_mismatch_between_passes_is_reported(self):
        problems = bench.cross_pass_inconsistencies(
            [entry("row", 1, exit_codes=(0,)), entry("row", 2, exit_codes=(1,))]
        )
        self.assertEqual(len(problems), 1)

    def test_single_pass_is_not_a_mismatch(self):
        self.assertEqual(bench.cross_pass_inconsistencies([entry("row", 1)]), [])

    def test_only_the_mismatching_row_is_reported(self):
        problems = bench.cross_pass_inconsistencies(
            [
                entry("stable", 1),
                entry("stable", 2),
                entry("drifting", 1, stdout="first"),
                entry("drifting", 2, stdout="second"),
            ]
        )
        self.assertEqual([problem["row"] for problem in problems], ["drifting"])


class HarnessSourceDigestTests(unittest.TestCase):
    def test_digest_covers_every_harness_source_file(self):
        digest = bench.harness_source_digest()
        self.assertEqual(set(digest["files"]), {"bench/bench.py", "bench/workloads.py"})
        for entry in digest["files"].values():
            self.assertEqual(len(entry["sha256"]), 64)
            self.assertGreater(entry["bytes"], 0)

    def test_digest_is_stable_across_calls(self):
        self.assertEqual(
            bench.harness_source_digest()["combined_sha256"],
            bench.harness_source_digest()["combined_sha256"],
        )


class ProvenanceFieldTests(unittest.TestCase):
    def test_sha256_verification_reports_only_the_digest_check(self):
        # The digest check must not be described as a source-commit proof; that is
        # what --verify-build records separately.
        meta = {
            "binary_sha256_verified": True,
            "binary_source_build_verified": False,
            "binary_source_rebuilt_sha256": None,
        }
        self.assertTrue(meta["binary_sha256_verified"])
        self.assertFalse(meta["binary_source_build_verified"])
        self.assertIsNone(meta["binary_source_rebuilt_sha256"])


if __name__ == "__main__":
    unittest.main()
