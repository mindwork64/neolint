#!/usr/bin/env python3
"""Regression tests for the pinned OpenSimulator API catalogue generator."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location(
    "generate_opensim_catalogues", ROOT / "scripts" / "generate_opensim_catalogues.py"
)
assert SPEC and SPEC.loader
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


CANONICAL = """\
integer llFoo(integer value)
float llFoo(float value)
key llOwnerKey()
event state_entry()
event attach(key avatar)
event linkset_data(integer action, string name, string value)
const integer TRUE = 1
"""


def write_common_source(directory: Path) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "ILSL_Api.cs").write_text(
        """
        public interface ILSL_Api {
            // An overload must not be dropped.
            public LSL_Integer llFoo(LSL_Integer value);
            public LSL_Float llFoo(LSL_Float value);
            public string llOwnerKey();
            public LSL_String llEcho(string text);
        }
        """,
        encoding="utf-8",
    )
    (directory / "IOSSL_Api.cs").write_text(
        """
        public interface IOSSL_Api {
            public LSL_Integer osApproxEquals(LSL_Float a, LSL_Float b);
            public LSL_Integer osApproxEquals(LSL_Vector a, LSL_Vector b);
        }
        """,
        encoding="utf-8",
    )
    (directory / "LSL_Constants.cs").write_text(
        """
        public static class LSL_Constants {
            public const int OS_APIVERSION = 20;
            public const int TRUE = 1;
            public static readonly LSL_Float PI = 3.141592653589793;
        }
        """,
        encoding="utf-8",
    )


class CatalogueGeneratorTests(unittest.TestCase):
    def test_comment_stripping_preserves_comment_markers_inside_strings(self) -> None:
        source = 'string url = "https://example.test/a/*b*/"; // remove me\n'
        stripped = GENERATOR.strip_comments(source)
        self.assertIn("https://example.test/a/*b*/", stripped)
        self.assertNotIn("remove me", stripped)

    def test_xengine_lexer_parser_intersection_is_used_for_events(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            output = root / "output"
            write_common_source(source)
            (source / "lsl.lexer.cs").write_text(
                """
                public class STATE_ENTRY_EVENT : TOKEN {}
                public class ATTACH_EVENT : TOKEN {}
                public class LINKSET_DATA_EVENT : TOKEN {}
                """,
                encoding="utf-8",
            )
            (source / "lsl.parser.cs").write_text(
                """
                public class VoidArgEvent_1 : VoidArgEvent {
                  public VoidArgEvent_1() : base(((STATE_ENTRY_EVENT)(value))) {}
                }
                public class KeyArgEvent_1 : KeyArgEvent {
                  public KeyArgEvent_1() : base(((ATTACH_EVENT)(value))) {}
                }
                public class IntArgEvent_1 : IntArgEvent {
                  public IntArgEvent_1() : base(((LINKSET_DATA_EVENT)(value))) {}
                }
                """,
                encoding="utf-8",
            )
            canonical = root / "lsl-builtins.txt"
            canonical.write_text(CANONICAL, encoding="utf-8")

            counts = GENERATOR.generate("xengine-api20", source, output, canonical)
            self.assertEqual(counts["events"], 3)
            events = (output / "events.txt").read_text(encoding="utf-8")
            self.assertIn("event state_entry()", events)
            self.assertIn("event attach(key avatar)", events)
            self.assertIn(
                "event linkset_data(integer action, string name, string value)",
                events,
            )

    def test_yengine_event_codes_intersect_with_handler_interface(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            output = root / "output"
            write_common_source(source)
            (source / "MMRScriptEventCode.cs").write_text(
                """
                public enum MMRScriptEventCode {
                    state_entry = 1,
                    attach = 2,
                }
                """,
                encoding="utf-8",
            )
            (source / "MMRIEventHandlers.cs").write_text(
                """
                public interface MMRIEventHandlers {
                    void state_entry();
                    void attach(LSL_String avatar);
                    void region_cross();
                }
                """,
                encoding="utf-8",
            )
            canonical = root / "lsl-builtins.txt"
            canonical.write_text(CANONICAL, encoding="utf-8")

            counts = GENERATOR.generate("yengine-api20", source, output, canonical)
            self.assertEqual(counts["events"], 2)
            events = (output / "events.txt").read_text(encoding="utf-8")
            self.assertIn("event attach(key avatar)", events)
            self.assertNotIn("region_cross", events)

    def test_function_overloads_and_key_string_aliases_are_retained(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            output = root / "output"
            write_common_source(source)
            (source / "lsl.lexer.cs").write_text(
                "public class STATE_ENTRY_EVENT : TOKEN {}",
                encoding="utf-8",
            )
            (source / "lsl.parser.cs").write_text(
                """
                public class VoidArgEvent_1 : VoidArgEvent {
                  public VoidArgEvent_1() : base(((STATE_ENTRY_EVENT)(value))) {}
                }
                """,
                encoding="utf-8",
            )
            canonical = root / "lsl-builtins.txt"
            canonical.write_text(CANONICAL, encoding="utf-8")

            counts = GENERATOR.generate("xengine-api20", source, output, canonical)
            self.assertEqual(counts["function_signatures"], 4)
            functions = (output / "functions.txt").read_text(encoding="utf-8")
            self.assertIn("integer llFoo(integer value)", functions)
            self.assertIn("float llFoo(float value)", functions)
            self.assertIn("key llOwnerKey()", functions)
            self.assertIn("string llEcho(string text)", functions)
            ossl = (output / "ossl-functions.txt").read_text(encoding="utf-8")
            self.assertEqual(ossl.count("osApproxEquals("), 2)

    def test_generated_snapshots_have_stable_interface_counts(self) -> None:
        expected = {
            "xengine-api20": (425, 425, 231, 258, 35, 770),
            "yengine-api20": (425, 425, 231, 258, 36, 770),
            "yengine-api22": (434, 434, 231, 258, 36, 780),
            "yengine-api23": (457, 458, 258, 289, 37, 815),
            "yengine-api24": (473, 474, 265, 296, 37, 888),
        }
        for profile, counts in expected.items():
            directory = ROOT / "data" / "opensim" / profile
            functions_text = (directory / "functions.txt").read_text(encoding="utf-8")
            function_rows = [
                line for line in functions_text.splitlines()
                if " ll" in line and "(" in line
            ]
            self.assertEqual(len(function_rows), counts[1])
            self.assertEqual(
                len({line.split("(", 1)[0].split()[-1] for line in function_rows}),
                counts[0],
            )
            ossl_text = (directory / "ossl-functions.txt").read_text(encoding="utf-8")
            ossl_rows = [
                line for line in ossl_text.splitlines()
                if " os" in line and "(" in line
            ]
            self.assertEqual(len(ossl_rows), counts[3])
            self.assertEqual(
                len({line.split("(", 1)[0].split()[-1] for line in ossl_rows}),
                counts[2],
            )
            events = (directory / "events.txt").read_text(encoding="utf-8")
            self.assertEqual(sum(1 for line in events.splitlines() if line.startswith("event ")), counts[4])
            constants = (directory / "constants.txt").read_text(encoding="utf-8")
            self.assertEqual(sum(1 for line in constants.splitlines() if line.startswith("const ")), counts[5])
            self.assertIn(f"const integer OS_APIVERSION = {20 if profile.endswith('api20') else 22 if profile.endswith('api22') else 23 if profile.endswith('api23') else 24}", constants)


if __name__ == "__main__":
    unittest.main(verbosity=2)
