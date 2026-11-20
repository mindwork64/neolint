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



