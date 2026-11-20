import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from extract_lsl_deprecated import extract_deprecated_functions


class ExtractDeprecatedFunctionsTests(unittest.TestCase):
    def test_extracts_only_lsl_deprecated_functions(self):
        source = """\
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
            extract_deprecated_functions("constants:\n  TRUE: 1\n")

    def test_extracts_last_function_before_next_top_level_section(self):
        source = """\
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
