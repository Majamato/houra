#!/usr/bin/env python3
"""Install or refresh the current user's launcher for a local build.

Defaults to the stable production variant. Pass `--variant devel` to install
only the development launcher and icons from prepared dev assets.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def atomic_write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        try:
            output.write(content)
            output.flush()
            temporary.chmod(0o644)
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)


def desktop_value(value: str) -> str:
    return value.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")


def exec_argument(value: str) -> str:
    # Exec quoting is interpreted after desktop-entry string escaping.
    quoted = re.sub(r'([\\"`$])', r'\\\1', value)
    return desktop_value(f'"{quoted}"').replace("%", "%%")


def refresh_cache(command: str, directory: Path, *options: str) -> None:
    executable = shutil.which(command)
    if executable:
        result = subprocess.run([executable, *options, str(directory)], check=False)
        if result.returncode:
            print(f"Warning: {command} could not refresh {directory}", file=sys.stderr)


def manifest() -> dict:
    return json.loads((ROOT / "data/app-variants.json").read_text())


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description="Install Houra's desktop launcher.")
    parser.add_argument("--variant", choices=["stable", "devel"], default="stable")
    parser.add_argument("--build-dir", type=Path, default=None)
    return parser.parse_args(argv)


def main(argv=None) -> None:
    args = parse_args(argv)
    if args.variant == "devel":
        devel = manifest()["devel"]
        app_id, app_name = devel["app_id"], devel["app_name"]
        build = Path(args.build_dir or ROOT / "target/dev/debug").resolve()
        source_icons = build / "icons/hicolor"
        build_hint = "./scripts/build-dev.sh"
    else:
        stable = manifest()["stable"]
        app_id, app_name = stable["app_id"], stable["app_name"]
        build = Path(
            args.build_dir
            or os.environ.get("HOURA_BUILD_DIR", ROOT / "build-release")
        ).resolve()
        source_icons = ROOT / "data/icons/hicolor"
        build_hint = "./scripts/build-release.sh"
    for name in ["houra", "gschemas.compiled", f"{app_id}.desktop"]:
        if not (build / name).is_file():
            raise ValueError(f"Missing {build / name}. Run {build_hint} first.")

    data = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")).resolve()
    icons = data / "icons/hicolor"
    content = (source_icons / "scalable/apps" / f"{app_id}.svg").read_bytes()
    digest = hashlib.sha256(content).hexdigest()[:16]
    # A changed filename also invalidates GNOME Shell's cached launcher image.
    icon = icons / "scalable/apps" / f"{app_id}-{digest}.svg"
    desktop = (build / f"{app_id}.desktop").read_text()
    if args.variant == "devel":
        # The dev binary finds its schema beside the executable; no
        # GSETTINGS_SCHEMA_DIR wrapper that would leak into child processes.
        command = exec_argument(str(build / "houra"))
    else:
        command = "/usr/bin/env " + " ".join([
            exec_argument(f"GSETTINGS_SCHEMA_DIR={build}"),
            exec_argument(str(build / "houra")),
        ])
    desktop = re.sub(r"^Exec=.*$", lambda _: f"Exec={command}", desktop, flags=re.MULTILINE)
    desktop = re.sub(r"^Icon=.*$", lambda _: f"Icon={desktop_value(str(icon))}", desktop, flags=re.MULTILINE)

    atomic_write(icon, content)
    atomic_write(icons / "scalable/apps" / f"{app_id}.svg", content)
    atomic_write(icons / "symbolic/apps" / f"{app_id}-symbolic.svg",
                 (source_icons / "symbolic/apps" / f"{app_id}-symbolic.svg").read_bytes())
    launcher = data / "applications" / f"{app_id}.desktop"
    atomic_write(launcher, desktop.encode())
    if args.variant == "stable":
        extension = data / f"gnome-shell/extensions/{stable['extension_uuid']}"
        if extension.is_dir():
            atomic_write(extension / "icons/houra-symbolic.svg",
                         (source_icons / "symbolic/apps" / f"{app_id}-symbolic.svg").read_bytes())
            print("Updated the installed top-bar icon. Log out and back in to refresh it.")
    for previous in icon.parent.glob(f"{app_id}-*.svg"):
        if previous != icon and re.fullmatch(rf"{re.escape(app_id)}-[0-9a-f]{{16}}\.svg", previous.name):
            previous.unlink()
    refresh_cache("gtk4-update-icon-cache", icons, "-qtf")
    refresh_cache("update-desktop-database", launcher.parent)
    print(f"Updated the local {app_name} launcher and icon:\n  {launcher}")
    print(f"Quit and reopen {app_name} to use the rebuilt application.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"Cannot install Houra's desktop launcher: {error}", file=sys.stderr)
        sys.exit(1)
