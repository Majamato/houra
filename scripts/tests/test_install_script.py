import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
APP_ID = "io.github.majamato.Houra"
UUID = "houra@majamato.github.io"
EXTENSION = f"share/gnome-shell/extensions/{UUID}"

CURL_STUB = """#!/bin/sh
# Serves file:// URLs, and the latest-release redirect from HOURA_TEST_LATEST.
output=''
url=''
effective=0
while [ $# -gt 0 ]; do
    case $1 in
        -o) output=$2; shift ;;
        -w) effective=1; shift ;;
        -*) ;;
        *) url=$1 ;;
    esac
    shift
done
if [ $effective = 1 ]; then
    printf '%s' "$HOURA_TEST_LATEST"
    exit 0
fi
cp "${url#file://}" "$output"
"""


def release_files(version, *, extra_locale=True):
    """Files of a binary tarball, relative to its top directory."""
    files = {
        "install.sh": ((ROOT / "scripts/install.sh").read_bytes(), 0o755),
        "bin/houra": (f"#!/bin/sh\necho 'houra {version}'\n".encode(), 0o755),
        f"share/applications/{APP_ID}.desktop": (
            b"[Desktop Entry]\nName=Houra\nExec=houra\nIcon=io.github.majamato.Houra\n", 0o644),
        f"share/metainfo/{APP_ID}.metainfo.xml": (b"<component/>\n", 0o644),
        f"share/glib-2.0/schemas/{APP_ID}.gschema.xml": (b"<schemalist/>\n", 0o644),
        f"share/icons/hicolor/scalable/apps/{APP_ID}.svg": (b"<svg/>\n", 0o644),
        f"share/icons/hicolor/symbolic/apps/{APP_ID}-symbolic.svg": (b"<svg/>\n", 0o644),
        f"{EXTENSION}/metadata.json": (
            b'{\n  "uuid": "houra@majamato.github.io",\n  "shell-version": ["49", "50", "51"]\n}\n',
            0o644),
        f"{EXTENSION}/extension.js": (b"export default class {}\n", 0o644),
        f"{EXTENSION}/icons/houra-symbolic.svg": (b"<svg/>\n", 0o644),
        "share/locale/es/LC_MESSAGES/houra.mo": (b"es", 0o644),
        "share/licenses/houra/LICENSE": (b"GPL", 0o644),
    }
    if extra_locale:
        files["share/locale/fr/LC_MESSAGES/houra.mo"] = (b"fr", 0o644)
    return files


def write_tarball(path, version, files, links=None, top=None):
    top = top or f"houra-{version}-x86_64-linux"
    links = {f"{EXTENSION}/locale": "../../../locale"} if links is None else links
    with tarfile.open(path, "w:xz") as archive:
        directories = {top}
        for name in list(files) + list(links):
            parts = name.split("/")[:-1]
            for index in range(1, len(parts) + 1):
                directories.add(f"{top}/{'/'.join(parts[:index])}")
        for directory in sorted(directories):
            info = tarfile.TarInfo(directory)
            info.type = tarfile.DIRTYPE
            info.mode = 0o755
            archive.addfile(info)
        for name, (content, mode) in files.items():
            info = tarfile.TarInfo(f"{top}/{name}")
            info.size = len(content)
            info.mode = mode
            archive.addfile(info, io.BytesIO(content))
        for name, target in links.items():
            info = tarfile.TarInfo(f"{top}/{name}")
            info.type = tarfile.SYMTYPE
            info.linkname = target
            archive.addfile(info)
    return path


class InstallScriptTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-install-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "home with space"
        self.home.mkdir()
        self.prefix = self.home / ".local"
        self.system = self.root / "system prefix"
        self.distro = self.root / "distro"
        self.os_release = self.root / "os-release"
        self.os_release.write_text('ID=fedora\nVERSION_ID=44\n')
        self.releases = self.root / "releases"
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.stub("ldd", 'printf "%s\\n" "${HOURA_TEST_LDD:-}"\n')
        self.stub("gnome-shell", 'echo "GNOME Shell ${HOURA_TEST_SHELL:-50.1}"\n')
        self.stub("glib-compile-schemas", 'echo "$1" >> "$HOURA_TEST_LOG"\n'
                  ': > "$1/gschemas.compiled"\n')
        self.stub("curl", CURL_STUB.split("\n", 1)[1])
        self.log = self.root / "schemas.log"
        self.env = {
            "PATH": f"{self.tools}:/usr/bin:/bin",
            "HOME": str(self.home),
            "HOURA_OS_RELEASE": str(self.os_release),
            "HOURA_SYSTEM_PREFIX": str(self.system),
            "HOURA_DISTRO_PREFIX": str(self.distro),
            "HOURA_RELEASES_URL": f"file://{self.releases}",
            "HOURA_TEST_LOG": str(self.log),
            "HOURA_ALLOW_ROOT": "1",
            "LC_ALL": "C",
        }

    def stub(self, name, body):
        path = self.tools / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def release(self, version, **options):
        """Publishes a release under the fake releases URL and returns its tarball."""
        directory = self.releases / "download" / f"v{version}"
        directory.mkdir(parents=True, exist_ok=True)
        name = f"houra-{version}-x86_64-linux.tar.xz"
        files = options.pop("files", None) or release_files(version, **options)
        tarball = write_tarball(directory / name, version, files)
        digest = hashlib.sha256(tarball.read_bytes()).hexdigest()
        (directory / "SHA256SUMS").write_text(f"{digest}  {name}\n")
        return tarball

    def run_script(self, *args, script=ROOT / "scripts/install.sh", **env):
        return subprocess.run(["bash", str(script), *args], env={**self.env, **env},
                              capture_output=True, text=True)

    def assert_ok(self, result):
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

    def installed_files(self, prefix):
        return sorted(str(path.relative_to(prefix)) for path in prefix.rglob("*")
                      if path.is_file() or path.is_symlink())

    def test_per_user_install(self):
        tarball = self.release("1.2.3")
        self.assert_ok(self.run_script("--from-file", str(tarball)))

        binary = self.prefix / "bin/houra"
        self.assertEqual(subprocess.run([binary], capture_output=True, text=True).stdout,
                         "houra 1.2.3\n")
        desktop = (self.prefix / "share/applications" / f"{APP_ID}.desktop").read_text()
        self.assertIn(f'Exec="{binary}"\n', desktop)
        self.assertNotIn("Exec=houra", desktop)
        locale = self.prefix / EXTENSION / "locale"
        self.assertEqual(os.readlink(locale), "../../../locale")
        self.assertEqual(locale.resolve(), (self.prefix / "share/locale").resolve())
        self.assertEqual((self.prefix / "share/houra-installer/version").read_text(), "1.2.3\n")
        manifest = (self.prefix / "share/houra-installer/manifest").read_text().splitlines()
        self.assertIn("bin/houra", manifest)
        self.assertNotIn("install.sh", manifest)
        self.assertTrue((self.prefix / "share/houra-installer/install.sh").is_file())
        self.assertEqual(self.log.read_text().splitlines(),
                         [str(self.prefix / "share/glib-2.0/schemas")])
        self.assertFalse((self.prefix / "share/icons/hicolor/icon-theme.cache").exists())

    def test_exec_quoting_matches_desktop_spec(self):
        self.home = self.root / 'odd "$home" `x` 100%'
        self.home.mkdir()
        self.prefix = self.home / ".local"
        self.assert_ok(self.run_script("--from-file", str(self.release("1.2.3")),
                                       HOME=str(self.home)))
        desktop = (self.prefix / "share/applications" / f"{APP_ID}.desktop").read_text()
        quoted = (str(self.prefix / "bin/houra")
                  .replace("\\", "\\\\").replace('"', '\\"').replace("`", "\\`")
                  .replace("$", "\\$"))
        expected = f'"{quoted}"'.replace("\\", "\\\\").replace("%", "%%")
        self.assertIn(f"Exec={expected}\n", desktop)

    def test_latest_release_is_downloaded_and_verified(self):
        self.release("1.2.3")
        result = self.run_script(HOURA_TEST_LATEST="https://example.test/releases/tag/v1.2.3")
        self.assert_ok(result)
        self.assertEqual((self.prefix / "share/houra-installer/version").read_text(), "1.2.3\n")

    def test_requested_version_is_downloaded(self):
        self.release("1.2.3")
        self.release("2.0.0")
        self.assert_ok(self.run_script("--version", "1.2.3"))
        self.assertEqual((self.prefix / "share/houra-installer/version").read_text(), "1.2.3\n")

    def test_checksum_mismatch_installs_nothing(self):
        tarball = self.release("1.2.3")
        sums = tarball.parent / "SHA256SUMS"
        sums.write_text(f"{'0' * 64}  {tarball.name}\n")
        result = self.run_script("--version", "1.2.3")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_same_version_is_left_alone_unless_reinstalling(self):
        tarball = self.release("1.2.3")
        self.assert_ok(self.run_script("--from-file", str(tarball)))
        self.log.unlink()
        result = self.run_script("--from-file", str(tarball))
        self.assert_ok(result)
        self.assertIn("already installed", result.stdout)
        self.assertFalse(self.log.exists())
        self.assert_ok(self.run_script("--from-file", str(tarball), "--reinstall"))
        self.assertTrue(self.log.exists())

    def test_update_removes_files_the_new_version_dropped(self):
        self.assert_ok(self.run_script("--from-file", str(self.release("1.2.3"))))
        french = self.prefix / "share/locale/fr/LC_MESSAGES/houra.mo"
        self.assertTrue(french.is_file())
        result = self.run_script("--from-file", str(self.release("1.3.0", extra_locale=False)))
        self.assert_ok(result)
        self.assertIn("Updated Houra from 1.2.3 to 1.3.0", result.stdout)
        self.assertFalse(french.exists())
        self.assertTrue((self.prefix / "share/locale/es/LC_MESSAGES/houra.mo").is_file())
        self.assertNotIn("share/locale/fr/LC_MESSAGES/houra.mo",
                         (self.prefix / "share/houra-installer/manifest").read_text())

    def test_uninstall_keeps_data_and_unrelated_files(self):
        self.assert_ok(self.run_script("--from-file", str(self.release("1.2.3"))))
        data = self.prefix / "share/houra/houra.sqlite3"
        data.parent.mkdir(parents=True)
        data.write_text("hours")
        other_schema = self.prefix / "share/glib-2.0/schemas/org.example.Other.gschema.xml"
        other_schema.write_text("<schemalist/>")
        other_mo = self.prefix / "share/locale/es/LC_MESSAGES/other.mo"
        other_mo.write_text("other")
        autostart = self.home / ".config/autostart" / f"{APP_ID}.desktop"
        autostart.parent.mkdir(parents=True)
        escaped = str(self.prefix / "bin/houra").replace(" ", "\\ ")
        autostart.write_text(f"[Desktop Entry]\nExec={escaped}\n")

        installed = self.prefix / "share/houra-installer/install.sh"
        result = self.run_script("--uninstall", script=installed)
        self.assert_ok(result)
        self.assertIn("was kept", result.stdout)
        self.assertEqual(data.read_text(), "hours")
        self.assertTrue(other_schema.exists())
        self.assertTrue(other_mo.exists())
        self.assertTrue((self.prefix / "share/glib-2.0/schemas/gschemas.compiled").exists())
        self.assertFalse(autostart.exists())
        self.assertFalse((self.prefix / EXTENSION).exists())
        self.assertFalse((self.prefix / "share/houra-installer").exists())
        self.assertEqual(self.installed_files(self.prefix), sorted([
            "share/glib-2.0/schemas/gschemas.compiled",
            "share/glib-2.0/schemas/org.example.Other.gschema.xml",
            "share/houra/houra.sqlite3",
            "share/locale/es/LC_MESSAGES/other.mo",
        ]))

    def test_uninstall_keeps_an_autostart_entry_for_another_binary(self):
        self.assert_ok(self.run_script("--from-file", str(self.release("1.2.3"))))
        autostart = self.home / ".config/autostart" / f"{APP_ID}.desktop"
        autostart.parent.mkdir(parents=True)
        autostart.write_text("[Desktop Entry]\nExec=/usr/bin/houra\n")
        self.assert_ok(self.run_script("--uninstall"))
        self.assertTrue(autostart.exists())

    def test_forged_manifest_entries_are_rejected(self):
        self.assert_ok(self.run_script("--from-file", str(self.release("1.2.3"))))
        data = self.prefix / "share/houra/houra.sqlite3"
        data.parent.mkdir(parents=True)
        data.write_text("hours")
        manifest = self.prefix / "share/houra-installer/manifest"
        original = manifest.read_text()
        for entry in ["share/houra/houra.sqlite3", "../outside",
                      f"{EXTENSION}/../../../houra/houra.sqlite3", "/etc/passwd"]:
            with self.subTest(entry=entry):
                manifest.write_text(original + entry + "\n")
                result = self.run_script("--uninstall")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("not a Houra file", result.stderr)
                self.assertTrue(data.exists())
                self.assertTrue((self.prefix / "bin/houra").exists())

    def test_distribution_package_blocks_the_install(self):
        (self.distro / "bin").mkdir(parents=True)
        (self.distro / "bin/houra").write_text("packaged")
        result = self.run_script("--from-file", str(self.release("1.2.3")))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already installed", result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_system_install_blocks_a_per_user_install(self):
        (self.system / "share/houra-installer").mkdir(parents=True)
        (self.system / "share/houra-installer/manifest").write_text("bin/houra\n")
        result = self.run_script("--from-file", str(self.release("1.2.3")))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--system", result.stderr)

    def test_system_install_into_its_prefix(self):
        tarball = self.release("1.2.3")
        self.assert_ok(self.run_script("--system", "--from-file", str(tarball)))
        self.assertTrue((self.system / "bin/houra").is_file())
        self.assertFalse(self.prefix.exists())
        self.assertIn(f'Exec="{self.system}/bin/houra"',
                      (self.system / "share/applications" / f"{APP_ID}.desktop").read_text())
        self.assert_ok(self.run_script("--system", "--uninstall"))
        self.assertFalse((self.system / "bin/houra").exists())

    @unittest.skipIf(os.geteuid() == 0, "root can write anywhere")
    def test_system_install_without_root_prints_the_sudo_command(self):
        self.system.mkdir()
        self.system.chmod(0o555)
        self.addCleanup(self.system.chmod, 0o755)
        result = self.run_script("--system", "--from-file", str(self.release("1.2.3")))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("| sudo bash -s -- --system", result.stderr)

    def test_missing_libraries_print_the_distribution_command(self):
        tarball = self.release("1.2.3")
        ldd = "\tlibadwaita-1.so.0 => not found\n\tlibgtk-4.so.1 => not found"
        cases = [
            ("ID=ubuntu\n", "sudo apt install libadwaita-1-0 libgtk-4-1"),
            ('ID=pop\nID_LIKE="ubuntu debian"\n', "sudo apt install libadwaita-1-0 libgtk-4-1"),
            ('ID="opensuse-tumbleweed"\nID_LIKE="opensuse suse"\n',
             "sudo zypper install libadwaita-1-0 libgtk-4-1"),
            ("ID=endeavouros\nID_LIKE=arch\n", "sudo pacman -S --needed libadwaita gtk4"),
            ("ID=fedora\n", "sudo dnf install libadwaita gtk4"),
            ("ID=gentoo\n", "Install the packages that provide: libadwaita-1.so.0 libgtk-4.so.1"),
        ]
        for os_release, hint in cases:
            with self.subTest(os_release=os_release):
                self.os_release.write_text(os_release)
                result = self.run_script("--from-file", str(tarball), HOURA_TEST_LDD=ldd)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(hint, result.stderr)
                self.assertFalse(self.prefix.exists())

    def test_old_glibc_is_explained(self):
        ldd = "./houra: /lib64/libc.so.6: version `GLIBC_2.41' not found"
        result = self.run_script("--from-file", str(self.release("1.2.3")), HOURA_TEST_LDD=ldd)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("glibc 2.41", result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_unsupported_shell_version_warns_but_installs(self):
        result = self.run_script("--from-file", str(self.release("1.2.3")),
                                 HOURA_TEST_SHELL="48.2")
        self.assert_ok(result)
        self.assertIn("this is GNOME Shell 48.2", result.stderr)
        self.assertIn("GNOME Shell 49, 50 or 51", result.stderr)

    def test_unexpected_archive_contents_are_rejected(self):
        files = release_files("1.2.3")
        cases = {
            "outside path": dict(files={**files, "../evil": (b"x", 0o644)}),
            "unknown file": dict(files={**files, "share/applications/evil.desktop": (b"x", 0o644)}),
            "other link": dict(files=files, links={"bin/sh": "/bin/sh"}),
            "escaping link": dict(files=files, links={f"{EXTENSION}/locale": "/etc"}),
        }
        for label, contents in cases.items():
            with self.subTest(label):
                tarball = write_tarball(self.root / f"{label}.tar.xz", "1.2.3", **contents)
                result = self.run_script("--from-file", str(tarball))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unexpected", result.stderr)
                self.assertFalse(self.prefix.exists())

    def test_root_needs_system_for_an_install(self):
        env = dict(self.env)
        del env["HOURA_ALLOW_ROOT"]
        if os.geteuid() != 0:
            self.skipTest("needs root")
        result = subprocess.run(["bash", str(ROOT / "scripts/install.sh"), "--from-file",
                                 str(self.release("1.2.3"))], env=env,
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--system", result.stderr)

    def test_custom_data_home_is_refused(self):
        result = self.run_script("--from-file", str(self.release("1.2.3")),
                                 XDG_DATA_HOME=str(self.root / "data"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("XDG_DATA_HOME", result.stderr)

    def test_dev_extension_copy_is_replaced(self):
        stale = self.prefix / EXTENSION / "stale.js"
        stale.parent.mkdir(parents=True)
        stale.write_text("old")
        result = self.run_script("--from-file", str(self.release("1.2.3")))
        self.assert_ok(result)
        self.assertIn("Replacing", result.stdout)
        self.assertFalse(stale.exists())
        self.assertTrue((self.prefix / EXTENSION / "metadata.json").exists())


if __name__ == "__main__":
    unittest.main()
