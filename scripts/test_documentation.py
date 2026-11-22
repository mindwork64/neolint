import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DIAGNOSTIC_SOURCES = (
    "src/lexer.rs",
    "src/parser.rs",
    "src/semantic.rs",
) + tuple(
    str(path.relative_to(ROOT))
    for directory in ("src/preprocessor", "src/ast")
    for path in sorted((ROOT / directory).rglob("*.rs"))
)


class DocumentationTests(unittest.TestCase):
    def test_diagnostic_reference_covers_every_emitted_code(self):
        source_codes = set()
        for relative in DIAGNOSTIC_SOURCES:
            source_codes.update(
                re.findall(
                    r'"(?:LEX|PAR|SEM|PRE|AST)\d{3}"',
                    (ROOT / relative).read_text(),
                )
            )
        documented = (ROOT / "docs/diagnostics.md").read_text()
        missing = sorted(code.strip('"') for code in source_codes if code.strip('"') not in documented)
        self.assertEqual(missing, [], f"missing diagnostic docs: {missing}")

    def test_readme_documentation_links_exist(self):
        readme = (ROOT / "README.md").read_text()
        for target in re.findall(r"\]\((docs/[^)#]+)(?:#[^)]*)?\)", readme):
            self.assertTrue((ROOT / target).is_file(), f"missing README link: {target}")

    def test_release_workflow_names_both_supported_targets(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("x86_64-unknown-linux-gnu", workflow)
        self.assertIn("x86_64-pc-windows-msvc", workflow)
        self.assertIn("workflow_dispatch", workflow)
        self.assertIn("sha256", workflow.lower())


    def test_project_dual_license_metadata_and_files(self):
        cargo = (ROOT / "Cargo.toml").read_text()
        self.assertIn('license = "MIT OR Apache-2.0"', cargo)
        mit = (ROOT / "LICENSE-MIT").read_text()
        apache = (ROOT / "LICENSE-APACHE").read_text()
        self.assertIn("Copyright (c) 2026 mindwork64", mit)
        self.assertIn("Permission is hereby granted", mit)
        self.assertIn("Apache License", apache)
        self.assertIn("Version 2.0", apache)

    def test_project_license_docs_and_third_party_notice(self):
        readme = (ROOT / "README.md").read_text()
        self.assertIn("MIT OR Apache-2.0", readme)
        self.assertIn("LICENSE-MIT", readme)
        self.assertIn("LICENSE-APACHE", readme)
        self.assertIn("LICENSES/LINDEN-RESEARCH-MIT.txt", readme)
        self.assertNotIn("License selection is pending", readme)
        self.assertTrue((ROOT / "LICENSES/LINDEN-RESEARCH-MIT.txt").is_file())
        self.assertTrue((ROOT / "data/README.md").is_file())


    def test_release_version_documentation_matches_package(self):
        cargo = (ROOT / "Cargo.toml").read_text()
        version = re.search(r'^version = "([^"]+)"', cargo, re.MULTILINE).group(1)
        getting_started = (ROOT / "docs/getting-started.md").read_text()
        readme = (ROOT / "README.md").read_text()
        self.assertIn(f"neolint-v{version}-x86_64-unknown-linux-gnu.tar.gz", getting_started)
        self.assertIn(f"neolint-v{version}-x86_64-pc-windows-msvc.zip", getting_started)
        self.assertIn(f"Current version: `{version}`", readme)


if __name__ == "__main__":
    unittest.main()
