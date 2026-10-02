import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
APP_ID = "io.github.majamato.Houra"


class DesktopInstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-desktop-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.build = self.root / "build with spaces"
        self.build.mkdir()
        (self.build / "houra").write_text("binary fixture")
        (self.build / "gschemas.compiled").write_text("schema fixture")
        (self.build / f"{APP_ID}.desktop").write_text(
            "[Desktop Entry]\nName=Houra\nName[es]=Houra\n"
            f"Exec=houra\nIcon={APP_ID}\nType=Application\n"
        )
        self.data = self.root / "data with spaces"
        self.launcher = self.data / "applications" / f"{APP_ID}.desktop"
        self.launcher.parent.mkdir(parents=True)
        self.launcher.write_text(
            "[Desktop Entry]\nName=Houra\nExec=old-build\n"
            f"Icon={ROOT}/data/icons/hicolor/scalable/apps/{APP_ID}.svg\n"
        )
        tools = self.root / "tools"
        tools.mkdir()
        for tool in ["cargo", "rustc", "cc", "meson", "ninja", "pkg-config",
                     "glib-compile-resources", "glib-compile-schemas", "msgfmt",
                     "xgettext", "gtk4-update-icon-cache", "update-desktop-database"]:
            path = tools / tool
            path.write_text("#!/bin/sh\nexit 0\n")
            path.chmod(0o755)
        self.env = dict(os.environ, XDG_DATA_HOME=str(self.data),
                        HOURA_BUILD_DIR=str(self.build),
                        PATH=f"{tools}:{os.environ['PATH']}")

    def run_script(self, script):
        return subprocess.run([str(ROOT / "scripts" / script)], env=self.env,
                              capture_output=True, text=True)

    def assert_current_launcher(self):
        source = ROOT / "data/icons/hicolor/scalable/apps" / f"{APP_ID}.svg"
        digest = hashlib.sha256(source.read_bytes()).hexdigest()[:16]
        icon = self.data / "icons/hicolor/scalable/apps" / f"{APP_ID}-{digest}.svg"
        text = self.launcher.read_text()
        self.assertIn(f"Icon={icon}\n", text)
        self.assertEqual(icon.read_bytes(), source.read_bytes())
        symbolic = Path("symbolic/apps") / f"{APP_ID}-symbolic.svg"
        self.assertEqual((self.data / "icons/hicolor" / symbolic).read_bytes(),
                         (ROOT / "data/icons/hicolor" / symbolic).read_bytes())
        self.assertIn(f'"{self.build}/houra"', text)
        self.assertIn("Name[es]=Houra", text)

    def test_release_build_refreshes_existing_launcher(self):
        result = self.run_script("build-release.sh")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_current_launcher()

    def test_reinstall_replaces_stale_icon_and_is_repeatable(self):
        icons = self.data / "icons/hicolor/scalable/apps"
        icons.mkdir(parents=True)
        stale = icons / f"{APP_ID}-{'0' * 16}.svg"
        stale.write_text("old icon")
        unrelated = icons / "other-app.svg"
        unrelated.write_text("keep me")
        for _ in range(2):
            result = self.run_script("install-desktop.py")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_current_launcher()
        self.assertFalse(stale.exists())
        self.assertEqual(unrelated.read_text(), "keep me")

    def test_missing_build_keeps_existing_launcher(self):
        previous = self.launcher.read_bytes()
        (self.build / "houra").unlink()
        result = self.run_script("install-desktop.py")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.launcher.read_bytes(), previous)

    def test_first_release_build_creates_launcher(self):
        self.launcher.unlink()
        result = self.run_script("build-release.sh")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_current_launcher()

    def test_packaging_build_does_not_change_launcher(self):
        previous = self.launcher.read_bytes()
        self.env["HOURA_INSTALL_DESKTOP"] = "0"
        result = self.run_script("build-release.sh")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.launcher.read_bytes(), previous)
        self.assertFalse((self.data / "icons").exists())

    def test_failed_build_keeps_existing_launcher(self):
        previous = self.launcher.read_bytes()
        (self.root / "tools/meson").write_text("#!/bin/sh\nexit 1\n")
        result = self.run_script("build-release.sh")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.launcher.read_bytes(), previous)

    def test_installed_extension_icon_is_refreshed_without_replacing_code(self):
        extension = self.data / "gnome-shell/extensions/houra@majamato.github.io"
        (extension / "icons").mkdir(parents=True)
        (extension / "icons/houra-symbolic.svg").write_text("old icon")
        (extension / "extension.js").write_text("keep installed extension code")
        result = self.run_script("install-desktop.py")
        self.assertEqual(result.returncode, 0, result.stderr)
        source = ROOT / "data/icons/hicolor/symbolic/apps" / f"{APP_ID}-symbolic.svg"
        self.assertEqual((extension / "icons/houra-symbolic.svg").read_bytes(), source.read_bytes())
        self.assertEqual((extension / "extension.js").read_text(), "keep installed extension code")

    def test_desktop_install_does_not_create_an_extension(self):
        result = self.run_script("install-desktop.py")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.data / "gnome-shell/extensions").exists())


if __name__ == "__main__":
    unittest.main()
