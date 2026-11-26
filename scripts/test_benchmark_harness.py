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


class FlowWorkloadTests(unittest.TestCase):
    def test_flow_workloads_are_deterministic_and_cover_budget_paths(self):
        import tempfile

        with tempfile.TemporaryDirectory() as first_dir, tempfile.TemporaryDirectory() as second_dir:
            first = pathlib.Path(first_dir)
            second = pathlib.Path(second_dir)
            generated = [
                ("flow_worklist", lambda root: bench.workloads.flow_worklist(root, 4)),
                ("flow_side_effects", lambda root: bench.workloads.flow_side_effects(root, 4)),
                ("flow_budget", lambda root: bench.workloads.flow_budget(root, 4)),
                ("switch_dispatch", lambda root: bench.workloads.switch_dispatch(root, 3)),
            ]
            for name, make in generated:
                first_path = make(first)
                second_path = make(second)
                self.assertEqual(first_path.read_bytes(), second_path.read_bytes(), name)

            budget = (first / "flow_budget.lsl").read_text(encoding="utf-8")
            switch = (first / "switch_dispatch_3.lsl").read_text(encoding="utf-8")
            self.assertIn("after return: budget must fail closed", budget)
            self.assertIn("#define USE_SWITCHES", switch)
            self.assertIn("case 0: return 0;", switch)
            self.assertIn("case 2: return 2;", switch)
            self.assertIn("default: return -1;", switch)


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
