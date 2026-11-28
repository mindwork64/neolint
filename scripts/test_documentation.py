import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DIAGNOSTIC_SOURCES = (
    "src/lexer.rs",
    "src/parser.rs",
    "src/semantic.rs",
    "src/main.rs",
    "src/source/unit.rs",
) + tuple(
    str(path.relative_to(ROOT))
    for directory in ("src/preprocessor", "src/ast", "src/ast_semantic")
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

    def test_language_support_documents_ast_only_unused_analysis(self):
        language_support = (ROOT / "docs/language-support.md").read_text()
        for code in ("SEM024", "SEM025", "SEM026"):
            self.assertIn(code, language_support, f"language-support.md omits {code}")

    def test_readme_documentation_links_exist(self):
        readme = (ROOT / "README.md").read_text()
        for target in re.findall(r"\]\((docs/[^)#]+)(?:#[^)]*)?\)", readme):
            self.assertTrue((ROOT / target).is_file(), f"missing README link: {target}")

    def test_cli_reference_documents_all_include_directory_spellings(self):
        cli = (ROOT / "docs/cli-reference.md").read_text()
        for spelling in ("`-I DIR`", "`-IDIR`", "`--include-dir DIR`", "`--include-dir=DIR`"):
            self.assertIn(spelling, cli, f"cli-reference.md omits {spelling}")

    def test_firestorm_docs_separate_dialect_from_syntax_gates(self):
        firestorm = (ROOT / "docs/firestorm-preprocessor.md").read_text()
        language = (ROOT / "docs/language-support.md").read_text()
        self.assertIn("USE_SWITCHES", firestorm)
        self.assertIn("USE_LAZY_LISTS", firestorm)
        self.assertIn("does **not** turn on switch or lazy-list parsing", firestorm)
        self.assertIn("The preprocessor dialect alone does not enable either extension", language)

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


    def test_ast_default_and_legacy_migration_are_documented(self):
        cli = (ROOT / "docs/cli-reference.md").read_text()
        config = (ROOT / "docs/configuration.md").read_text()
        changelog = (ROOT / "CHANGELOG.md").read_text()
        self.assertIn("`ast` is the default in v1.1.0 and later", cli)
        self.assertIn("## Engine migration (v1.1.0)", config)
        self.assertIn("`engine = legacy`", config)
        self.assertIn("Legacy analyzer remains available", changelog)

    def test_release_version_documentation_matches_package(self):
        cargo = (ROOT / "Cargo.toml").read_text()
        version = re.search(r'^version = "([^"]+)"', cargo, re.MULTILINE).group(1)
        getting_started = (ROOT / "docs/getting-started.md").read_text()
        readme = (ROOT / "README.md").read_text()
        self.assertIn(f"neolint-v{version}-x86_64-unknown-linux-gnu.tar.gz", getting_started)
        self.assertIn(f"neolint-v{version}-x86_64-pc-windows-msvc.zip", getting_started)
        self.assertIn(f"Next release target", readme)
        self.assertIn(f"`{version}` (CLI + LSP + VSIX; not published yet)", readme)
        self.assertIn("Latest published release", readme)

    def test_lsp_initialize_result_uses_protocol_shape(self):
        source = (ROOT / "src/lsp.rs").read_text()
        tests = (ROOT / "tests/lsp_protocol.rs").read_text()
        smoke = (ROOT / "scripts/release-smoke-lsp.py").read_text()
        self.assertIn('"capabilities": capabilities', source)
        self.assertIn('"serverInfo"', source)
        self.assertIn('initialize_result["result"]["capabilities"]', tests)
        self.assertIn('result.get("capabilities", {})', smoke)
        self.assertIn('result["serverInfo"].get("version")', smoke)

    def test_release_artifacts_include_cli_lsp_licenses_and_vsix(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        for required in (
            "--features lsp --bin neolint-lsp",
            "release-smoke-lsp.py",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "LINDEN-RESEARCH-MIT.txt",
            "OPENSIM-NOTICE.txt",
            "THIRD-PARTY-NOTICES.md",
            "generate-third-party-notices.py",
            "neolint-vscode-0.2.0.vsix",
            "sha256sum -c",
        ):
            self.assertIn(required, workflow)
        package = (ROOT / "editors/vscode/package.json").read_text()
        self.assertIn('"version": "0.2.0"', package)
        self.assertIn('"@vscode/vsce": "4.1.1"', package)
        self.assertIn('"package": "npm run compile && vsce package"', package)
        self.assertIn('"minimatch": "11.0.1"', package)
        vscodeignore = (ROOT / "editors/vscode/.vscodeignore").read_text()
        self.assertNotIn("node_modules/**", vscodeignore)
        self.assertTrue((ROOT / "scripts/verify_vsix.py").is_file())
        self.assertTrue((ROOT / "scripts/verify_release_archives.py").is_file())
        self.assertTrue((ROOT / "scripts/generate-third-party-notices.py").is_file())
        self.assertTrue((ROOT / "docs/releasing.md").is_file())
        release_guide = (ROOT / "docs/releasing.md").read_text()
        self.assertIn("publish", release_guide)
        self.assertIn("git push origin vX.Y.Z", release_guide)
        self.assertIn("workflow_dispatch", workflow)
        self.assertIn("GH_TOKEN: ${{ github.token }}", workflow)
        self.assertIn("extension/LICENSE.txt", (ROOT / "scripts/verify_vsix.py").read_text())


if __name__ == "__main__":
    unittest.main()
