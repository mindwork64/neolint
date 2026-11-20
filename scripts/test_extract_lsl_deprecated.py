import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("extract-lsl-deprecated.py")
SPEC = importlib.util.spec_from_file_location("extract_lsl_deprecated", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Cannot load extractor module from {SCRIPT_PATH}")
EXTRACTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXTRACTOR)
extract_deprecated_functions = EXTRACTOR.extract_deprecated_functions


class ExtractDeprecatedFunctionsTests(unittest.TestCase):
    def test_extracts_only_lsl_deprecated_functions(self):
        source = """\\
enums:
  OldEnum:
    deprecated: true
functions:
  llLegacyFlag:
    deprecated: true
    tooltip: old function
  llLegacyAlias:
    deprecated:
      use: ll.NewApi
  llCurrent:
    tooltip: current function
  llSluaOnly:
    slua-deprecated: true
  llNotDeprecated:
    deprecated: false
types:
  integer:
"""
        self.assertEqual(
            extract_deprecated_functions(source),
            ["llLegacyFlag", "llLegacyAlias"],
        )

    def test_rejects_missing_functions_section(self):
        with self.assertRaisesRegex(ValueError, "functions"):
            extract_deprecated_functions("constants:\\n  TRUE: 1\\n")

    def test_extracts_last_function_before_next_top_level_section(self):
        source = """\\
functions:
  llOld:
    deprecated: true
types:
  string:
    deprecated: true
"""
        self.assertEqual(extract_deprecated_functions(source), ["llOld"])


if __name__ == "__main__":
    unittest.main()
