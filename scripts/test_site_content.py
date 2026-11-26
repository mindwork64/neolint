import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD_SCRIPT = ROOT / "scripts/build_site_content.mjs"
CATALOG = ROOT / "site/src/data/diagnostics.json"
GENERATED_DOCS = ROOT / "site/src/content/docs/docs"


class SiteContentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run(
            ["node", str(BUILD_SCRIPT)],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        cls.rules = json.loads(CATALOG.read_text(encoding="utf-8"))
        cls.by_code = {rule["code"]: rule for rule in cls.rules}

    def test_catalog_is_nonempty_and_codes_are_unique(self):
        codes = [rule["code"] for rule in self.rules]
        self.assertGreater(len(codes), 50)
        self.assertEqual(len(codes), len(set(codes)))
        self.assertIn("SEM047", codes)
        self.assertIn("SEM048", codes)
        self.assertIn("SEM049", codes)
        self.assertIn("SEM050", codes)

    def test_catalog_covers_every_diagnostic_source_code(self):
        import re

        source_files = [
            ROOT / "src/lexer.rs",
            ROOT / "src/parser.rs",
            ROOT / "src/semantic.rs",
        ]
        for directory in ("src/preprocessor", "src/ast", "src/ast_semantic"):
            source_files.extend((ROOT / directory).rglob("*.rs"))

        emitted = set()
        pattern = re.compile(r'"((?:LEX|PAR|SEM|PRE|AST)\d{3})"')
        for path in source_files:
            if path.is_file():
                emitted.update(pattern.findall(path.read_text(encoding="utf-8")))

        self.assertEqual(sorted(emitted - self.by_code.keys()), [])

    def test_known_severity_and_engine_metadata(self):
        self.assertEqual(self.by_code["PRE037"]["severity"], "info")
        self.assertEqual(self.by_code["SEM024"]["severity"], "warning")
        self.assertEqual(self.by_code["SEM024"]["engine"], "AST only")
        self.assertEqual(self.by_code["LEX001"]["engine"], "AST + Legacy")

    def test_site_docs_keep_canonical_body_titles_and_edit_links(self):
        import re

        source_files = sorted(path.relative_to(ROOT / "docs") for path in (ROOT / "docs").rglob("*.md"))
        copied_files = sorted(path.relative_to(GENERATED_DOCS) for path in GENERATED_DOCS.rglob("*.md"))
        self.assertEqual(source_files, copied_files)
        for relative in source_files:
            original = (ROOT / "docs" / relative).read_text(encoding="utf-8")
            generated = (GENERATED_DOCS / relative).read_text(encoding="utf-8")
            original_frontmatter = re.match(r"^---\r?\n[\s\S]*?\r?\n---\r?\n?", original)
            generated_frontmatter = re.match(r"^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n){1,2}", generated)
            self.assertIsNotNone(generated_frontmatter, f"generated page has no frontmatter: {relative}")

            original_body = original[original_frontmatter.end():] if original_frontmatter else original
            generated_body = generated[generated_frontmatter.end():]
            self.assertEqual(
                generated_body,
                original_body,
                f"generated page changed canonical Markdown content: {relative}",
            )

            metadata = generated_frontmatter.group(1)
            self.assertRegex(metadata, r"(?m)^title: .+$")
            expected_edit_url = "https://github.com/mindwork64/neolint/edit/main/docs/" + relative.as_posix()
            self.assertIn(f'editUrl: "{expected_edit_url}"', metadata)


    def test_production_routes_and_github_pages_base_path(self):
        import re

        site_dist = ROOT / "site/dist"
        expected_pages = (
            "index.html",
            "diagnostics/index.html",
            "docs/index.html",
            "docs/diagnostics/index.html",
            "404.html",
        )
        for relative in expected_pages:
            self.assertTrue((site_dist / relative).is_file(), f"missing production page: {relative}")

        broken_urls = []
        for page in site_dist.rglob("*.html"):
            html = page.read_text(encoding="utf-8")
            for url in re.findall(r'(?:href|src)="([^"]+)"', html):
                if url.startswith("/") and url not in ("/neolint", "/neolint/") and not url.startswith("/neolint/"):
                    broken_urls.append((page.relative_to(site_dist), url))

        self.assertEqual(broken_urls, [])
        homepage = (site_dist / "index.html").read_text(encoding="utf-8")
        self.assertIn('src="/neolint/assets/neolint-logo.png"', homepage)
        self.assertIn('src="/neolint/assets/neolint-icon.png"', homepage)
        catalog = (site_dist / "diagnostics/index.html").read_text(encoding="utf-8")
        self.assertIn('href="/neolint/docs/diagnostics/"', catalog)
        getting_started = (site_dist / "docs/getting-started/index.html").read_text(encoding="utf-8")
        self.assertIn(
            'href="https://github.com/mindwork64/neolint/edit/main/docs/getting-started.md"',
            getting_started,
        )
        self.assertNotIn("src/content/docs", getting_started)
        not_found = (site_dist / "404.html").read_text(encoding="utf-8")
        self.assertIn("This page went", not_found)

    def test_diagnostic_catalog_has_search_filters_and_every_rule(self):
        catalog = (ROOT / "site/dist/diagnostics/index.html").read_text(encoding="utf-8")
        self.assertEqual(catalog.count('class="rule-card"'), len(self.rules))
        self.assertIn('data-search', catalog)
        self.assertIn('data-category', catalog)
        self.assertIn('data-severity', catalog)
        self.assertIn('data-engine', catalog)
        self.assertIn('data-reset', catalog)
        self.assertIn("SEM043", catalog)
        self.assertIn("SEM045", catalog)
        self.assertIn("SEM046", catalog)

        component = (ROOT / "site/src/components/DiagnosticCatalog.astro").read_text(encoding="utf-8")
        self.assertIn("event.key === '/'", component)
        self.assertIn("card.hidden = !matches", component)
        self.assertIn("search?.focus()", component)

    def test_lsl_fenced_examples_use_the_custom_shiki_grammar(self):
        import re

        diagnostics_page = (ROOT / "site/dist/docs/diagnostics/index.html").read_text(encoding="utf-8")
        fences = re.findall(r'<pre data-language="lsl"><code>(.*?)</code></pre>', diagnostics_page, re.DOTALL)
        self.assertGreater(len(fences), 0)
        highlighted = "\n".join(fences)
        for token in ("default", "state_entry", "integer", "llOwnerSay"):
            self.assertRegex(
                highlighted,
                rf'<span style="[^"]*">{re.escape(token)}</span>',
                f"LSL token is not syntax-highlighted: {token}",
            )

if __name__ == "__main__":
    unittest.main()
