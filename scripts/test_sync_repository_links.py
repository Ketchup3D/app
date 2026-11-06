import tempfile
from pathlib import Path
import unittest
from sync_repository_links import synchronize


class RepositoryLinksTests(unittest.TestCase):
    def test_fork_and_second_fork_are_idempotent(self):
        source = Path(__file__).resolve().parents[1] / "README.md"
        with tempfile.TemporaryDirectory() as folder:
            readme = Path(folder) / "README.md"
            readme.write_text(source.read_text())
            for repository, base in [("alice/editor", "https://alice.github.io/editor/"),
                                     ("bob/bob.github.io", "https://bob.github.io/")]:
                synchronize(readme, repository)
                text = readme.read_text()
                self.assertIn(f"](https://github.com/{repository}/releases/latest)", text)
                self.assertIn(f"]({base}play/)", text)
                self.assertIn("https://docs.velopack.io/packaging/signing", text)
                synchronize(readme, repository)
                self.assertEqual(readme.read_text(), text)
            synchronize(readme, "bob/bob.github.io", "https://editor.example.org")
            self.assertIn("](https://editor.example.org/play/)", readme.read_text())


if __name__ == "__main__":
    unittest.main()
