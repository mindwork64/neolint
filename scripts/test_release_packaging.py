from __future__ import annotations

import io
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

from verify_release_archives import REQUIRED_FILES, verify_linux, verify_windows


NOTICES = b"# Third-party notices\n\n## License and copyright texts\n"


def file_payload(name: str) -> bytes:
    if name == "THIRD-PARTY-NOTICES.md":
        return NOTICES
    return f"placeholder content for {name}".encode()


class ReleaseArchiveTests(unittest.TestCase):
    def make_linux_archive(self, path: Path, *, omit: str | None = None) -> None:
        with tarfile.open(path, "w:gz") as archive:
            for name in sorted(REQUIRED_FILES - ({omit} if omit else set())):
                payload = file_payload(name)
                entry = tarfile.TarInfo(name)
                entry.size = len(payload)
                archive.addfile(entry, io.BytesIO(payload))

    def make_windows_archive(self, path: Path, *, omit: str | None = None) -> None:
        with zipfile.ZipFile(path, "w") as archive:
            for name in sorted(REQUIRED_FILES - ({omit} if omit else set())):
                archive_name = (
                    f"{name}.exe" if name in {"neolint", "neolint-lsp"} else name
                )
                archive.writestr(archive_name, file_payload(name))

    def test_linux_archive_has_both_binaries_and_all_notices(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "neolint.tar.gz"
            self.make_linux_archive(archive)
            verify_linux(archive)

    def test_windows_archive_has_both_executables_and_all_notices(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "neolint.zip"
            self.make_windows_archive(archive)
            verify_windows(archive)

    def test_linux_archive_rejects_missing_third_party_notices(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "neolint.tar.gz"
            self.make_linux_archive(archive, omit="THIRD-PARTY-NOTICES.md")
            with self.assertRaisesRegex(ValueError, "THIRD-PARTY-NOTICES.md"):
                verify_linux(archive)

    def test_windows_archive_rejects_empty_binary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "neolint.zip"
            with zipfile.ZipFile(archive, "w") as output:
                for name in sorted(REQUIRED_FILES):
                    archive_name = (
                        f"{name}.exe" if name in {"neolint", "neolint-lsp"} else name
                    )
                    payload = b"" if archive_name == "neolint.exe" else file_payload(name)
                    output.writestr(archive_name, payload)
            with self.assertRaisesRegex(ValueError, "neolint.exe"):
                verify_windows(archive)


if __name__ == "__main__":
    unittest.main()
