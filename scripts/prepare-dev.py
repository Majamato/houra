#!/usr/bin/env python3
"""Prepare Houra's development artifacts from the production sources.

Generates an isolated settings schema, desktop launcher, icons, and GNOME
Shell extension for the development variant, without touching any production
source file. Build code imports the pure helpers; the CLI writes artifacts.
"""

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "data/app-variants.json"
STABLE_SCHEMA = ROOT / "data/io.github.majamato.Houra.gschema.xml"
STABLE_DESKTOP_TEMPLATE = ROOT / "data/io.github.majamato.Houra.desktop.in"
STABLE_METADATA = ROOT / "shell-extension/metadata.json"
STABLE_IDENTITY_JS = ROOT / "shell-extension/identity.js"
EXTENSION_SOURCES = ROOT / "shell-extension"


def dev_description(app_name: str) -> str:
    return (
        f"Shows {app_name}'s active timer in the top bar, with a button to pause or "
        f"resume it. Click it to open {app_name}. Installed and enabled together "
        f"with the {app_name} app."
    )


def load_manifest() -> dict:
    return json.loads(MANIFEST.read_text())


def dev_schema_xml(stable_xml: str, dev: dict) -> str:
    """Copies the production key definitions into a dev schema document.

    Changes the schema ID and path from the manifest and the dev
    `launch-at-login` default to false, preserving key types, ranges,
    choices, and every other default.
    """
    parser = ET.XMLParser(target=ET.TreeBuilder(insert_comments=True))
    root = ET.fromstring(stable_xml.encode(), parser=parser)
    schemas = root.findall("schema")
    if len(schemas) != 1:
        raise ValueError("the production schema file must define exactly one schema")
    schema = schemas[0]
    schema.set("id", dev["app_id"])
    schema.set("path", dev["settings_path"])
    launch = None
    for key in schema.findall("key"):
        if key.get("name") == "launch-at-login":
            launch = key
    if launch is None:
        raise ValueError("the production schema must define launch-at-login")
    default = launch.find("default")
    if default is None:
        raise ValueError("launch-at-login must declare a default")
    default.text = "false"
    ET.indent(root)
    return ET.tostring(root, encoding="unicode", xml_declaration=True)


def compile_schemas(source_dir: Path, output_dir: Path) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        ["glib-compile-schemas", "--strict", f"--targetdir={output_dir}", str(source_dir)],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(f"glib-compile-schemas failed: {result.stderr.strip()}")


def prepare_schemas(output_dir: Path) -> Path:
    """Writes `gschemas.compiled` for the dev schema into the output directory."""
    dev = load_manifest()["devel"]
    document = dev_schema_xml(STABLE_SCHEMA.read_text(), dev)
    with tempfile.TemporaryDirectory(prefix="houra-dev-schemas-") as directory:
        source = Path(directory) / f"{dev['app_id']}.gschema.xml"
        source.write_text(document + "\n")
        compile_schemas(Path(directory), output_dir)
    return output_dir / "gschemas.compiled"


def dev_desktop_entry(template: str, dev: dict) -> str:
    """Generates the dev launcher, changing name and icon.

    Leaves `Exec=` from the template unchanged: install-desktop.py rewrites it
    with the real build path and proper quoting.
    """
    lines = []
    for line in template.splitlines():
        key, separator, _ = line.partition("=")
        if separator and key == "Name":
            lines.append(f"Name={dev['app_name']}")
        elif separator and key == "Icon":
            lines.append(f"Icon={dev['app_id']}")
        else:
            lines.append(line)
    return "\n".join(lines) + "\n"


def dev_identity_js(stable_js: str, stable: dict, dev: dict) -> str:
    """Generates the dev identity module, keeping the stable module's shape."""
    generated = stable_js
    for key in ["app_id", "app_name", "extension_gtype_name", "extension_style_prefix"]:
        old = f"'{stable[key]}'"
        if generated.count(old) != 1:
            raise ValueError(f"stable identity.js must define {old} exactly once")
        generated = generated.replace(old, f"'{dev[key]}'")
    return generated


def dev_metadata(stable_metadata: dict, dev: dict) -> dict:
    """Generates the dev extension metadata from the stable metadata."""
    return {
        "uuid": dev["extension_uuid"],
        "name": dev["app_name"],
        "description": dev_description(dev["app_name"]),
        "shell-version": stable_metadata["shell-version"],
        "url": stable_metadata["url"],
        "gettext-domain": dev["extension_gettext_domain"],
    }


def dev_stylesheet(stable_css: str, stable_prefix: str, dev_prefix: str) -> str:
    """Retargets the extension stylesheet at the dev CSS classes."""
    return stable_css.replace(f".{stable_prefix}-", f".{dev_prefix}-")


def languages() -> list:
    return [
        line
        for line in (ROOT / "po/LINGUAS").read_text().splitlines()
        if line and not line.startswith("#")
    ]


def prepare_all(output_dir: Path) -> None:
    """Prepares every development artifact under the output directory."""
    manifest = load_manifest()
    stable, dev = manifest["stable"], manifest["devel"]
    output_dir.mkdir(parents=True, exist_ok=True)
    for owned in ("shell-extension", "icons"):
        shutil.rmtree(output_dir / owned, ignore_errors=True)
    (output_dir / f"{dev['app_id']}.desktop").unlink(missing_ok=True)
    prepare_schemas(output_dir)
    (output_dir / f"{dev['app_id']}.desktop").write_text(
        dev_desktop_entry(STABLE_DESKTOP_TEMPLATE.read_text(), dev)
    )
    for kind, suffix in [("scalable", ".svg"), ("symbolic", "-symbolic.svg")]:
        target = output_dir / "icons/hicolor" / kind / "apps" / f"{dev['app_id']}{suffix}"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(
            ROOT / "data/icons/hicolor" / kind / "apps" / f"{dev['app_id']}{suffix}",
            target,
        )
    extension = output_dir / "shell-extension"
    extension.mkdir(parents=True, exist_ok=True)
    (extension / "metadata.json").write_text(
        json.dumps(dev_metadata(json.loads(STABLE_METADATA.read_text()), dev), indent=2)
        + "\n"
    )
    (extension / "identity.js").write_text(
        dev_identity_js(STABLE_IDENTITY_JS.read_text(), stable, dev)
    )
    for name in ["activeTimer.js", "extension.js", "indicator.js", "format.js"]:
        shutil.copyfile(EXTENSION_SOURCES / name, extension / name)
    for name in ["stylesheet-dark.css", "stylesheet-light.css"]:
        (extension / name).write_text(dev_stylesheet(
            (EXTENSION_SOURCES / name).read_text(),
            stable["extension_style_prefix"],
            dev["extension_style_prefix"],
        ))
    icons = extension / "icons"
    icons.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(
        ROOT / "data/icons/hicolor/symbolic/apps" / f"{dev['app_id']}-symbolic.svg",
        icons / "houra-symbolic.svg",
    )
    for language in languages():
        target = extension / "locale" / language / "LC_MESSAGES" / f"{dev['extension_gettext_domain']}.mo"
        target.parent.mkdir(parents=True, exist_ok=True)
        result = subprocess.run(
            ["msgfmt", "--check", "--output-file", str(target),
             str(ROOT / "po" / f"{language}.po")],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode:
            raise RuntimeError(f"msgfmt failed for {language}: {result.stderr.strip()}")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="Prepare Houra's development artifacts.")
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--schemas-only", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.schemas_only:
            prepare_schemas(args.output_dir)
        else:
            prepare_all(args.output_dir)
    except (OSError, ValueError, RuntimeError) as error:
        print(f"Cannot prepare development artifacts: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
