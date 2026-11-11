import tempfile
from pathlib import Path
import unittest

from release_metadata import release_metadata


class ReleaseMetadataTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "Cargo.toml").write_text('[package]\nversion = "3.0.0"\n', encoding="utf-8")
        (self.root / "CHANGELOG.md").write_text("# Changelog\n\n## 3.0.0 - 2026-11-11\n\nNew fork release.\n\n## 2.1.2\n\nOld notes.\n", encoding="utf-8")

    def test_short_and_full_version_tags_extract_only_current_notes(self):
        for tag in ("v3.0", "v3.0.0"):
            self.assertEqual(release_metadata(self.root, tag), ("3.0.0", "New fork release.\n"))

    def test_wrong_version_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "does not match"):
            release_metadata(self.root, "v2.1.2")

    def test_branch_name_and_malformed_tag_are_rejected(self):
        for tag in ("master", "3.0", "v03.0", "v3.0\nother=value"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release_metadata(self.root, tag)

    def test_missing_release_notes_are_rejected(self):
        (self.root / "CHANGELOG.md").write_text("# Changelog\n## 2.1.2\nOld notes.\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "no release notes"):
            release_metadata(self.root, "v3.0")


if __name__ == "__main__":
    unittest.main()
