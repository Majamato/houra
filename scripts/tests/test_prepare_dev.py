import importlib.util
import json
import re
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
STABLE_APP_ID = "io.github.majamato.Houra"


def load_prepare_dev():
    spec = importlib.util.spec_from_file_location(
        "prepare_dev", ROOT / "scripts/prepare-dev.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


prepare_dev = load_prepare_dev()
MANIFEST = json.loads((ROOT / "data/app-variants.json").read_text())
DEVEL = MANIFEST["devel"]
DEV_APP_ID = DEVEL["app_id"]


def languages():
    return [line for line in (ROOT / "po/LINGUAS").read_text().splitlines()
            if line and not line.startswith("#")]


def snapshot_sources():
    files = {}
    for directory in ["data", "shell-extension", "po"]:
        for path in sorted((ROOT / directory).rglob("*")):
            if path.is_file():
                files[path] = path.read_bytes()
    return files


class PrepareDevTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="houra-prepare-dev-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.before = snapshot_sources()

    def run_prepare(self, *args):
        return subprocess.run(
            ["python3", "-B", str(ROOT / "scripts/prepare-dev.py"), *args],
            capture_output=True, text=True)

    def assert_sources_unchanged(self):
        self.assertEqual(snapshot_sources(), self.before)

    def test_schemas_only_writes_compiled_schema(self):
        output = self.root / "output with spaces"
        result = self.run_prepare("--output-dir", str(output), "--schemas-only")
        self.assertEqual(result.returncode, 0, result.stderr)
        compiled = output / "gschemas.compiled"
        self.assertTrue(compiled.is_file())
        self.assertGreater(compiled.stat().st_size, 0)
        self.assertEqual(sorted(p.name for p in output.iterdir()), ["gschemas.compiled"])
        self.assert_sources_unchanged()

    def test_dev_schema_changes_only_identity_and_autostart(self):
        stable = ET.fromstring((ROOT / "data/io.github.majamato.Houra.gschema.xml").read_bytes())
        dev = ET.fromstring(prepare_dev.dev_schema_xml(
            (ROOT / "data/io.github.majamato.Houra.gschema.xml").read_text(), DEVEL).encode())
        dev_schema = dev.find("schema")
        self.assertEqual(dev_schema.get("id"), DEV_APP_ID)
        self.assertEqual(dev_schema.get("path"), DEVEL["settings_path"])
        self.assertEqual(dev.get("gettext-domain"), stable.get("gettext-domain"))
        stable_keys = {key.get("name"): key for key in stable.find("schema").findall("key")}
        dev_keys = {key.get("name"): key for key in dev_schema.findall("key")}
        self.assertEqual(set(stable_keys), set(dev_keys))
        def canonical(element):
            return ET.tostring(element) if element is not None else None

        for name, stable_key in stable_keys.items():
            dev_key = dev_keys[name]
            self.assertEqual(dev_key.get("type"), stable_key.get("type"), name)
            self.assertEqual(canonical(dev_key.find("range")), canonical(stable_key.find("range")), name)
            self.assertEqual(
                [choice.get("value") for choice in dev_key.findall("choices/choice")],
                [choice.get("value") for choice in stable_key.findall("choices/choice")], name)
            expected = "false" if name == "launch-at-login" else stable_key.find("default").text
            self.assertEqual(dev_key.find("default").text, expected, name)
        self.assertEqual(stable_keys["launch-at-login"].find("default").text, "true")

    def test_full_tree_matches_the_plan(self):
        output = self.root / "output with spaces"
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = {
            "gschemas.compiled",
            f"{DEV_APP_ID}.desktop",
            f"icons/hicolor/scalable/apps/{DEV_APP_ID}.svg",
            f"icons/hicolor/symbolic/apps/{DEV_APP_ID}-symbolic.svg",
            "shell-extension/metadata.json",
            "shell-extension/identity.js",
            "shell-extension/activeTimer.js",
            "shell-extension/extension.js",
            "shell-extension/indicator.js",
            "shell-extension/format.js",
            "shell-extension/stylesheet-dark.css",
            "shell-extension/stylesheet-light.css",
            "shell-extension/icons/houra-symbolic.svg",
        }
        for language in languages():
            expected.add(f"shell-extension/locale/{language}/LC_MESSAGES/houra-dev.mo")
        actual = {str(path.relative_to(output)) for path in output.rglob("*") if path.is_file()}
        self.assertEqual(actual, expected)
        self.assert_sources_unchanged()

    def test_desktop_entry_changes_name_and_icon_but_keeps_exec(self):
        output = self.root / "output with spaces"
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        desktop = (output / f"{DEV_APP_ID}.desktop").read_text()
        template = (ROOT / "data/io.github.majamato.Houra.desktop.in").read_text()
        self.assertIn(f"Name={DEVEL['app_name']}\n", desktop)
        self.assertIn(f"Icon={DEV_APP_ID}\n", desktop)
        template_exec = next(line for line in template.splitlines() if line.startswith("Exec="))
        self.assertIn(template_exec + "\n", desktop)
        for key in ["Categories=", "Keywords=", "StartupNotify=", "Comment=", "Terminal=", "Type="]:
            template_line = next(line for line in template.splitlines() if line.startswith(key))
            self.assertIn(template_line + "\n", desktop)

    def test_icons_match_the_dev_sources(self):
        output = self.root / "output"
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        for kind, suffix in [("scalable", ".svg"), ("symbolic", "-symbolic.svg")]:
            self.assertEqual(
                (output / "icons/hicolor" / kind / "apps" / f"{DEV_APP_ID}{suffix}").read_bytes(),
                (ROOT / "data/icons/hicolor" / kind / "apps" / f"{DEV_APP_ID}{suffix}").read_bytes())
        self.assertEqual(
            (output / "shell-extension/icons/houra-symbolic.svg").read_bytes(),
            (ROOT / "data/icons/hicolor/symbolic/apps" / f"{DEV_APP_ID}-symbolic.svg").read_bytes())

    def test_metadata_uses_dev_identity_and_stable_compatibility(self):
        output = self.root / "output"
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        metadata = json.loads((output / "shell-extension/metadata.json").read_text())
        stable = json.loads((ROOT / "shell-extension/metadata.json").read_text())
        self.assertEqual(metadata["uuid"], DEVEL["extension_uuid"])
        self.assertEqual(metadata["name"], DEVEL["app_name"])
        self.assertEqual(metadata["gettext-domain"], DEVEL["extension_gettext_domain"])
        self.assertEqual(metadata["shell-version"], stable["shell-version"])
        self.assertEqual(metadata["url"], stable["url"])
        self.assertIn("Houra Dev", metadata["description"])

    def test_identity_generation_keeps_the_stable_shape(self):
        stable_js = (ROOT / "shell-extension/identity.js").read_text()
        generated = prepare_dev.dev_identity_js(stable_js, MANIFEST["stable"], DEVEL)
        for key in ["app_id", "app_name", "extension_gtype_name", "extension_style_prefix"]:
            self.assertIn(f"'{DEVEL[key]}'", generated)
        restored = generated
        for key in ["app_id", "app_name", "extension_gtype_name", "extension_style_prefix"]:
            restored = restored.replace(f"'{DEVEL[key]}'", f"'{MANIFEST['stable'][key]}'")
        self.assertEqual(restored, stable_js)

    def test_implementation_files_are_verbatim_copies(self):
        output = self.root / "output"
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in ["activeTimer.js", "extension.js", "indicator.js", "format.js"]:
            self.assertEqual(
                (output / "shell-extension" / name).read_bytes(),
                (ROOT / "shell-extension" / name).read_bytes(), name)
        published = (ROOT / "data/dbus/io.github.majamato.Houra.ActiveTimer.xml").read_text()
        self.assertIn(
            published.strip(),
            (output / "shell-extension/activeTimer.js").read_text())

    def test_dev_stylesheet_uses_custom_prefixes(self):
        css = ".a-x { color: red; }\n.other-a-x {}\n"
        self.assertEqual(
            prepare_dev.dev_stylesheet(css, "a", "b"),
            ".b-x { color: red; }\n.other-a-x {}\n")

    def test_full_prepare_removes_stale_owned_outputs(self):
        output = self.root / "output"
        stale_code = output / "shell-extension/stale.js"
        stale_code.parent.mkdir(parents=True)
        stale_code.write_text("removed extension file")
        stale_catalog = output / "shell-extension/locale/xx/LC_MESSAGES/houra-dev.mo"
        stale_catalog.parent.mkdir(parents=True)
        stale_catalog.write_text("dropped language")
        stale_desktop = output / f"{DEV_APP_ID}.desktop"
        stale_desktop.write_text("previous launcher")
        binary = output / "houra"
        binary.write_text("cargo build output")
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(stale_code.exists())
        self.assertFalse(stale_catalog.exists())
        self.assertNotEqual(stale_desktop.read_text(), "previous launcher")
        self.assertEqual(binary.read_text(), "cargo build output")

    def test_dev_css_uses_the_dev_prefix(self):
        output = self.root / "output"
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        bare = re.compile(r"\.houra-(?!dev-)")
        for name in ["stylesheet-dark.css", "stylesheet-light.css"]:
            css = (output / "shell-extension" / name).read_text()
            self.assertIn(".houra-dev-indicator", css)
            self.assertIsNone(bare.search(css), f"bare Houra selector in {name}")
            self.assertEqual(
                css.replace(".houra-dev-", ".houra-"),
                (ROOT / "shell-extension" / name).read_text())

    def test_translations_compile_to_dev_catalogs(self):
        output = self.root / "output"
        result = self.run_prepare("--output-dir", str(output))
        self.assertEqual(result.returncode, 0, result.stderr)
        for language in languages():
            catalog = output / "shell-extension/locale" / language / "LC_MESSAGES" / "houra-dev.mo"
            self.assertTrue(catalog.is_file(), language)
            content = catalog.read_bytes()
            self.assertGreater(len(content), 24, language)
            (magic,) = struct.unpack("<I", content[:4])
            self.assertEqual(magic, 0x950412DE, language)


if __name__ == "__main__":
    unittest.main()
