import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
VERSIONED_FILES = [
    "meson.build",
    "Cargo.toml",
    "packaging/fedora/houra.spec",
    "data/io.github.majamato.Houra.metainfo.xml.in",
    "packaging/debian/changelog",
    "packaging/arch/PKGBUILD",
]


class ReleaseTarballVersionTests(unittest.TestCase):
    """The version checks run before any crate is vendored, so no network is used."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-release-")
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / "repo"
        for name in VERSIONED_FILES + ["scripts/make-release-tarballs.sh"]:
            target = self.repo / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, target)
        self.env = dict(os.environ, GIT_AUTHOR_NAME="Test", GIT_AUTHOR_EMAIL="test@example.com",
                        GIT_COMMITTER_NAME="Test", GIT_COMMITTER_EMAIL="test@example.com",
                        GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1")
        self.git("init", "-q")

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.repo, env=self.env, check=True,
                       capture_output=True)

    def make_tarballs(self):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "release")
        return subprocess.run(
            [str(self.repo / "scripts/make-release-tarballs.sh"), str(self.repo / "out"), "HEAD"],
            cwd=self.repo, env=self.env, capture_output=True, text=True)

    def replace(self, name, old, new):
        path = self.repo / name
        text = path.read_text()
        self.assertIn(old, text)
        path.write_text(text.replace(old, new, 1))

    def current_version(self):
        text = (ROOT / "packaging/arch/PKGBUILD").read_text()
        return next(line.split("=", 1)[1] for line in text.splitlines()
                    if line.startswith("pkgver="))

    def test_debian_changelog_version_must_match(self):
        version = self.current_version()
        self.replace("packaging/debian/changelog", f"houra ({version}-1)", "houra (9.9.9-1)")
        result = self.make_tarballs()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("packaging/debian/changelog", result.stderr)
        self.assertIn("9.9.9", result.stderr)

    def test_debian_changelog_must_target_unstable(self):
        self.replace("packaging/debian/changelog", ") unstable;", ") resolute;")
        result = self.make_tarballs()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("packaging/debian/changelog", result.stderr)

    def test_pkgbuild_version_must_match(self):
        version = self.current_version()
        self.replace("packaging/arch/PKGBUILD", f"pkgver={version}", "pkgver=9.9.9")
        result = self.make_tarballs()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("packaging/arch/PKGBUILD", result.stderr)


if __name__ == "__main__":
    unittest.main()
