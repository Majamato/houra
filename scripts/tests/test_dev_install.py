import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
STABLE_APP_ID = "io.github.majamato.Houra"
STABLE_UUID = "houra@majamato.github.io"
MANIFEST = json.loads((ROOT / "data/app-variants.json").read_text())
DEVEL = MANIFEST["devel"]
DEV_APP_ID = DEVEL["app_id"]
DEV_UUID = DEVEL["extension_uuid"]


def load_script(name):
    spec = importlib.util.spec_from_file_location(
        name.replace("-", "_"), ROOT / "scripts" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


install_desktop = load_script("install-desktop")


def fake_tools(directory):
    tools = directory / "tools"
    tools.mkdir()
    for tool in ["cargo", "rustc", "cc", "meson", "ninja", "pkg-config",
                 "glib-compile-resources", "glib-compile-schemas", "msgfmt",
                 "xgettext", "gtk4-update-icon-cache", "update-desktop-database"]:
        path = tools / tool
        path.write_text("#!/bin/sh\nexit 0\n")
        path.chmod(0o755)
    return tools


def write_production_sentinels(data, config):
    sentinels = {}
    production = [
        data / "applications" / f"{STABLE_APP_ID}.desktop",
        data / "icons/hicolor/scalable/apps" / f"{STABLE_APP_ID}.svg",
        data / "icons/hicolor/scalable/apps" / f"{STABLE_APP_ID}-{'1' * 16}.svg",
        data / "icons/hicolor/symbolic/apps" / f"{STABLE_APP_ID}-symbolic.svg",
        data / "gnome-shell/extensions" / STABLE_UUID / "icons/houra-symbolic.svg",
        data / "gnome-shell/extensions" / STABLE_UUID / "extension.js",
        config / "autostart" / f"{STABLE_APP_ID}.desktop",
    ]
    for path in production:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"production {path.name}")
        sentinels[path] = path.read_bytes()
    return sentinels


def check_sentinels_intact(test_case, sentinels):
    for path, content in sentinels.items():
        test_case.assertEqual(path.read_bytes(), content, str(path))


class DevInstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-dev-install-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.build = self.root / "build with spaces"
        result = subprocess.run(
            ["python3", "-B", str(ROOT / "scripts/prepare-dev.py"),
             "--output-dir", str(self.build)],
            capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        (self.build / "houra").write_text("binary fixture")
        self.data = self.root / "data with spaces"
        self.config = self.root / "config with spaces"
        self.env = dict(os.environ, XDG_DATA_HOME=str(self.data),
                        XDG_CONFIG_HOME=str(self.config),
                        PATH=f"{fake_tools(self.root)}:{os.environ['PATH']}")
        self.env.pop("HOURA_BUILD_DIR", None)

    def run_install(self, *args):
        return subprocess.run(
            [str(ROOT / "scripts/install-desktop.py"), "--variant", "devel",
             "--build-dir", str(self.build), *args],
            env=self.env, capture_output=True, text=True)

    def write_sentinels(self):
        return write_production_sentinels(self.data, self.config)

    def assert_sentinels_intact(self, sentinels):
        check_sentinels_intact(self, sentinels)

    def assert_current_dev_launcher(self):
        source = ROOT / "data/icons/hicolor/scalable/apps" / f"{DEV_APP_ID}.svg"
        digest = hashlib.sha256(source.read_bytes()).hexdigest()[:16]
        icon = self.data / "icons/hicolor/scalable/apps" / f"{DEV_APP_ID}-{digest}.svg"
        launcher = self.data / "applications" / f"{DEV_APP_ID}.desktop"
        text = launcher.read_text()
        self.assertIn(f"Icon={icon}\n", text)
        self.assertEqual(icon.read_bytes(), source.read_bytes())
        symbolic = Path("symbolic/apps") / f"{DEV_APP_ID}-symbolic.svg"
        self.assertEqual((self.data / "icons/hicolor" / symbolic).read_bytes(),
                         (ROOT / "data/icons/hicolor" / symbolic).read_bytes())
        self.assertIn(f'Exec="{self.build}/houra"\n', text)
        self.assertNotIn("GSETTINGS_SCHEMA_DIR", text)
        self.assertIn("Name=Houra Dev\n", text)

    def test_dev_install_leaves_production_files_unchanged(self):
        sentinels = self.write_sentinels()
        result = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_current_dev_launcher()
        self.assert_sentinels_intact(sentinels)
        self.assertFalse((self.data / "gnome-shell/extensions" / DEV_UUID).exists())

    def test_reinstall_is_repeatable(self):
        for _ in range(2):
            result = self.run_install()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_current_dev_launcher()

    def test_missing_artifacts_fail_before_replacing(self):
        launcher = self.data / "applications" / f"{DEV_APP_ID}.desktop"
        for missing in ["houra", "gschemas.compiled", f"{DEV_APP_ID}.desktop"]:
            with self.subTest(missing=missing):
                launcher.parent.mkdir(parents=True, exist_ok=True)
                launcher.write_text("previous dev launcher")
                path = self.build / missing
                content = path.read_bytes()
                path.unlink()
                try:
                    result = self.run_install()
                finally:
                    path.write_bytes(content)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(launcher.read_text(), "previous dev launcher")

    def test_stale_dev_icons_are_removed_without_touching_production(self):
        icons = self.data / "icons/hicolor/scalable/apps"
        icons.mkdir(parents=True)
        stale = icons / f"{DEV_APP_ID}-{'0' * 16}.svg"
        stale.write_text("old dev icon")
        production = icons / f"{STABLE_APP_ID}-{'0' * 16}.svg"
        production.write_text("production icon")
        result = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_current_dev_launcher()
        self.assertFalse(stale.exists())
        self.assertEqual(production.read_text(), "production icon")

    def test_exec_escaping_handles_spaces_and_special_characters(self):
        result = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        launcher = self.data / "applications" / f"{DEV_APP_ID}.desktop"
        self.assertIn(f'"{self.build}/houra"', launcher.read_text())
        self.assertEqual(
            install_desktop.exec_argument("/tmp/my app/houra"), '"/tmp/my app/houra"')
        self.assertEqual(install_desktop.exec_argument("a$b"), '"a\\\\$b"')
        self.assertEqual(install_desktop.exec_argument("100%"), '"100%%"')
        self.assertEqual(
            install_desktop.desktop_value("a\\b\nc\rd\te"), "a\\\\b\\nc\\rd\\te")

    def test_explicit_build_dir_wins_over_production_environment(self):
        self.env["HOURA_BUILD_DIR"] = str(self.root / "release-build")
        result = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_current_dev_launcher()

    def test_explicit_stable_variant_keeps_production_behavior(self):
        build = self.root / "stable-build"
        build.mkdir()
        (build / "houra").write_text("binary fixture")
        (build / "gschemas.compiled").write_text("schema fixture")
        (build / f"{STABLE_APP_ID}.desktop").write_text(
            "[Desktop Entry]\nName=Houra\nExec=houra\n"
            f"Icon={STABLE_APP_ID}\nType=Application\n")
        result = subprocess.run(
            [str(ROOT / "scripts/install-desktop.py"), "--variant", "stable",
             "--build-dir", str(build)],
            env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        launcher = self.data / "applications" / f"{STABLE_APP_ID}.desktop"
        self.assertIn("Name=Houra\n", launcher.read_text())
        self.assertFalse((self.data / "applications" / f"{DEV_APP_ID}.desktop").exists())


class ShellExtensionInstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-dev-extension-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.build = self.root / "build with spaces"
        result = subprocess.run(
            ["python3", "-B", str(ROOT / "scripts/prepare-dev.py"),
             "--output-dir", str(self.build)],
            capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.data = self.root / "data with spaces"
        self.env = dict(os.environ, XDG_DATA_HOME=str(self.data),
                        PATH=f"{fake_tools(self.root)}:{os.environ['PATH']}")
        self.env.pop("HOURA_BUILD_DIR", None)

    def run_install(self, *args):
        return subprocess.run(
            [str(ROOT / "scripts/install-shell-extension.sh"), *args,
             "--build-dir", str(self.build)],
            env=self.env, capture_output=True, text=True)

    def dev_extension(self):
        return self.data / "gnome-shell/extensions" / DEV_UUID

    def test_default_mode_installs_the_prepared_dev_extension(self):
        stable = self.data / "gnome-shell/extensions" / STABLE_UUID / "extension.js"
        stable.parent.mkdir(parents=True)
        stable.write_text("stable extension")
        result = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        installed = self.dev_extension()
        for name in ["metadata.json", "identity.js", "activeTimer.js", "extension.js",
                     "indicator.js", "format.js", "stylesheet-dark.css",
                     "stylesheet-light.css", "icons/houra-symbolic.svg"]:
            self.assertEqual((installed / name).read_bytes(),
                             (self.build / "shell-extension" / name).read_bytes(), name)
        self.assertTrue((installed / "locale/es/LC_MESSAGES/houra-dev.mo").is_file())
        metadata = json.loads((installed / "metadata.json").read_text())
        self.assertEqual(metadata["uuid"], DEV_UUID)
        self.assertEqual(stable.read_text(), "stable extension")

    def test_reinstall_is_repeatable(self):
        for _ in range(2):
            result = self.run_install("dev")
            self.assertEqual(result.returncode, 0, result.stderr)
        installed = self.dev_extension()
        self.assertEqual((installed / "metadata.json").read_text(),
                         (self.build / "shell-extension/metadata.json").read_text())

    def test_identical_reinstall_reports_unchanged(self):
        result = self.run_install("dev")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Installed the top-bar extension", result.stdout)
        result = self.run_install("dev")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("unchanged", result.stdout)
        self.assertNotIn("Log out and back in", result.stdout)

    def test_uuid_mismatch_fails_before_replacing(self):
        sentinel = self.dev_extension() / "extension.js"
        sentinel.parent.mkdir(parents=True)
        sentinel.write_text("previous dev extension")
        metadata = self.build / "shell-extension/metadata.json"
        content = metadata.read_text()
        metadata.write_text(content.replace(DEV_UUID, STABLE_UUID))
        try:
            result = self.run_install("dev")
        finally:
            metadata.write_text(content)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(sentinel.read_text(), "previous dev extension")

    def test_missing_inputs_fail_before_replacing(self):
        sentinel = self.dev_extension() / "extension.js"
        sentinel.parent.mkdir(parents=True)
        sentinel.write_text("previous dev extension")
        missing = self.build / "shell-extension/identity.js"
        content = missing.read_bytes()
        missing.unlink()
        try:
            result = self.run_install("dev")
        finally:
            missing.write_bytes(content)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(sentinel.read_text(), "previous dev extension")

    def test_release_mode_installs_the_stable_sources(self):
        result = self.run_install("release")
        self.assertEqual(result.returncode, 0, result.stderr)
        installed = self.data / "gnome-shell/extensions" / STABLE_UUID
        self.assertEqual((installed / "identity.js").read_bytes(),
                         (ROOT / "shell-extension/identity.js").read_bytes())
        self.assertEqual((installed / "icons/houra-symbolic.svg").read_bytes(),
                         (ROOT / "data/icons/hicolor/symbolic/apps"
                          / f"{STABLE_APP_ID}-symbolic.svg").read_bytes())
        self.assertFalse(self.dev_extension().exists())


class UninstallDevTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-uninstall-dev-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.data = self.root / "data with spaces"
        self.config = self.root / "config with spaces"
        tools = fake_tools(self.root)
        self.calls = self.root / "calls.log"
        gsettings = tools / "gsettings"
        gsettings.write_text(
            "#!/bin/sh\n"
            f'echo "gsettings $@" >> {self.calls}\n'
            'if [ "$1" = "get" ]; then\n'
            f'  echo "[\'{STABLE_UUID}\', \'{DEV_UUID}\']"\n'
            "fi\n"
            "exit 0\n")
        gsettings.chmod(0o755)
        dconf = tools / "dconf"
        dconf.write_text(
            "#!/bin/sh\n"
            f'echo "dconf $@" >> {self.calls}\n'
            "exit 0\n")
        dconf.chmod(0o755)
        self.env = dict(os.environ, XDG_DATA_HOME=str(self.data),
                        XDG_CONFIG_HOME=str(self.config),
                        PATH=f"{tools}:{os.environ['PATH']}")
        self.env.pop("HOURA_BUILD_DIR", None)

    def write_dev_files(self):
        dev_files = [
            self.data / "applications" / f"{DEV_APP_ID}.desktop",
            self.data / "icons/hicolor/scalable/apps" / f"{DEV_APP_ID}.svg",
            self.data / "icons/hicolor/scalable/apps" / f"{DEV_APP_ID}-{'a' * 16}.svg",
            self.data / "icons/hicolor/symbolic/apps" / f"{DEV_APP_ID}-symbolic.svg",
            self.data / "gnome-shell/extensions" / DEV_UUID / "extension.js",
            self.data / "gnome-shell/extensions" / DEV_UUID / "metadata.json",
            self.config / "autostart" / f"{DEV_APP_ID}.desktop",
        ]
        for path in dev_files:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"dev {path.name}")
        data_dir = self.data / DEVEL["data_subdir"]
        database = data_dir / "houra.sqlite3"
        database.parent.mkdir(parents=True, exist_ok=True)
        database.write_text("dev database")
        return dev_files, data_dir

    def run_uninstall(self, *args):
        return subprocess.run(
            [str(ROOT / "scripts/uninstall-dev.sh"), *args],
            env=self.env, capture_output=True, text=True)

    def test_uninstall_removes_dev_files_and_keeps_production(self):
        sentinels = write_production_sentinels(self.data, self.config)
        dev_files, data_dir = self.write_dev_files()
        result = self.run_uninstall()
        self.assertEqual(result.returncode, 0, result.stderr)
        for path in dev_files:
            self.assertFalse(path.exists(), str(path))
        check_sentinels_intact(self, sentinels)
        calls = self.calls.read_text()
        self.assertIn(f"dconf reset -f {DEVEL['settings_path']}", calls)
        set_calls = [line for line in calls.splitlines()
                     if line.startswith("gsettings set ")]
        self.assertEqual(len(set_calls), 1)
        self.assertIn(STABLE_UUID, set_calls[0])
        self.assertNotIn(DEV_UUID, set_calls[0])
        self.assertTrue((data_dir / "houra.sqlite3").is_file())

    def test_purge_data_removes_the_dev_database(self):
        _, data_dir = self.write_dev_files()
        result = self.run_uninstall("--purge-data")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(data_dir.exists())

    def test_uninstall_is_repeatable(self):
        self.write_dev_files()
        for _ in range(2):
            result = self.run_uninstall()
            self.assertEqual(result.returncode, 0, result.stderr)


class DevWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-dev-workflow-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def write_fake_tool(self, path, body):
        path.write_text(body)
        path.chmod(0o755)

    def fixture_repo(self, build_exit=0, name="fixture"):
        """A fixture project with the real runner and fake builders."""
        fixture = self.root / name
        scripts = fixture / "scripts"
        scripts.mkdir(parents=True)
        shutil.copyfile(ROOT / "scripts/build-and-run.sh", scripts / "build-and-run.sh")
        (scripts / "build-and-run.sh").chmod(0o755)
        log = fixture / "calls.log"
        self.write_fake_tool(
            scripts / "build-dev.sh",
            f"#!/bin/sh\necho \"build-dev $@\" >> {log}\nexit {build_exit}\n")
        self.write_fake_tool(
            scripts / "install-dev.sh",
            f"#!/bin/sh\necho \"install-dev $@\" >> {log}\nexit 0\n")
        self.write_fake_tool(
            scripts / "build-release.sh",
            f"#!/bin/sh\necho \"build-release $@\" >> {log}\nexit 0\n")
        for profile in ["debug", "release"]:
            binary = fixture / "target/dev" / profile / "houra"
            binary.parent.mkdir(parents=True)
            self.write_fake_tool(
                binary, f"#!/bin/sh\necho \"run-{profile}\" >> {log}\n")
        release = fixture / "build-release/houra"
        release.parent.mkdir(parents=True)
        self.write_fake_tool(release, f"#!/bin/sh\necho run-release >> {log}\n")
        return fixture, log

    def run_runner(self, fixture, *args, xdg_data_dirs=None):
        if xdg_data_dirs is None:
            empty = fixture / "empty-data-dirs"
            empty.mkdir(exist_ok=True)
            xdg_data_dirs = str(empty)
        env = dict(os.environ, HOURA_BUILD_DIR=str(fixture / "build-release"),
                   XDG_DATA_DIRS=xdg_data_dirs)
        return subprocess.run([str(fixture / "scripts/build-and-run.sh"), *args],
                              env=env, capture_output=True, text=True)

    def write_packaged_launcher(self):
        packaged = self.root / "packaged"
        launcher = packaged / "applications/io.github.majamato.Houra.desktop"
        launcher.parent.mkdir(parents=True, exist_ok=True)
        launcher.write_text("[Desktop Entry]\nName=Houra\n")
        return str(packaged)

    def test_default_run_path_selects_dev(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log.read_text(), "build-dev \ninstall-dev \nrun-debug\n")

    def test_dev_run_path_selects_dev(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture, "dev")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log.read_text(), "build-dev \ninstall-dev \nrun-debug\n")

    def test_dev_release_builds_installs_and_runs_the_release_binary(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture, "dev", "--release")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            log.read_text(), "build-dev --release\ninstall-dev --release\nrun-release\n")

    def test_only_explicit_release_selects_production(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture, "release")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log.read_text(), "build-release \nrun-release\n")

    def test_release_refuses_to_replace_a_packaged_app(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture, "release",
                                 xdg_data_dirs=self.write_packaged_launcher())
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(log.exists())
        self.assertIn(
            "shares the packaged app's database, settings, autostart entry and bus name",
            result.stderr)
        self.assertIn("would lock the packaged app out", result.stderr)

    def test_release_replace_packaged_runs_production(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture, "release", "--replace-packaged",
                                 xdg_data_dirs=self.write_packaged_launcher())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log.read_text(), "build-release \nrun-release\n")

    def test_release_rejects_unknown_arguments(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture, "release", "--bogus")
        self.assertEqual(result.returncode, 2)
        self.assertFalse(log.exists())

    def test_dev_forwards_offline_in_either_position(self):
        cases = [
            (["dev", "--offline"], "build-dev --offline\ninstall-dev \nrun-debug\n"),
            (["dev", "--release", "--offline"],
             "build-dev --release --offline\ninstall-dev --release\nrun-release\n"),
            (["dev", "--offline", "--release"],
             "build-dev --release --offline\ninstall-dev --release\nrun-release\n"),
        ]
        for index, (args, expected) in enumerate(cases):
            with self.subTest(args=args):
                fixture, log = self.fixture_repo(name=f"fixture-{index}")
                result = self.run_runner(fixture, *args)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(log.read_text(), expected)

    def test_unknown_mode_is_rejected(self):
        fixture, log = self.fixture_repo()
        result = self.run_runner(fixture, "staging")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(log.exists())

    def test_build_failure_prevents_installation_and_execution(self):
        fixture, log = self.fixture_repo(build_exit=1)
        result = self.run_runner(fixture, "dev")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(log.read_text(), "build-dev \n")

    def test_dev_build_selects_the_feature_and_target_dir(self):
        fixture = self.root / "build-fixture"
        scripts = fixture / "scripts"
        scripts.mkdir(parents=True)
        shutil.copyfile(ROOT / "scripts/build-dev.sh", scripts / "build-dev.sh")
        (scripts / "build-dev.sh").chmod(0o755)
        log = fixture / "calls.log"
        tools = fixture / "tools"
        tools.mkdir()
        for tool in ["cargo", "rustc", "cc", "pkg-config", "glib-compile-resources",
                     "glib-compile-schemas", "msgfmt", "python3"]:
            self.write_fake_tool(
                tools / tool, f"#!/bin/sh\necho \"{tool} $@\" >> {log}\nexit 0\n")
        env = dict(os.environ, HOURA_BUILD_DIR="/tmp/production-build",
                   PATH=f"{tools}:{os.environ['PATH']}")
        for args, profile in [(["--release"], "release"), ([], "debug")]:
            log.unlink(missing_ok=True)
            result = subprocess.run([str(scripts / "build-dev.sh"), *args],
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = log.read_text()
            cargo_calls = [line for line in calls.splitlines() if line.startswith("cargo ")]
            self.assertEqual(len(cargo_calls), 1)
            self.assertIn(
                f"cargo build --workspace --locked --features native-ui,dev-app "
                f"--target-dir {fixture}/target/dev", cargo_calls[0])
            if profile == "release":
                self.assertIn("--release", cargo_calls[0])
            else:
                self.assertNotIn("--release", cargo_calls[0])
            self.assertIn(
                f"python3 -B {scripts}/prepare-dev.py --output-dir "
                f"{fixture}/target/dev/{profile}", calls)
            self.assertNotIn("/tmp/production-build", calls)
        result = subprocess.run([str(scripts / "build-dev.sh"), "--target-dir", "/tmp/x"],
                                env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertFalse((fixture / "target").exists())


if __name__ == "__main__":
    unittest.main()
