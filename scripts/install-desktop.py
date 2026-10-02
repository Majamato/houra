#!/usr/bin/env python3
"""Install or refresh the current user's launcher for a local release build."""

import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.majamato.Houra"


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


def main() -> None:
    build = Path(os.environ.get("HOURA_BUILD_DIR", ROOT / "build-release")).resolve()
    for name in ["houra", "gschemas.compiled", f"{APP_ID}.desktop"]:
        if not (build / name).is_file():
            raise ValueError(f"Missing {build / name}. Run ./scripts/build-release.sh first.")

    data = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")).resolve()
    icons = data / "icons/hicolor"
    source_icons = ROOT / "data/icons/hicolor"
    content = (source_icons / "scalable/apps" / f"{APP_ID}.svg").read_bytes()
    digest = hashlib.sha256(content).hexdigest()[:16]
    # A changed filename also invalidates GNOME Shell's cached launcher image.
    icon = icons / "scalable/apps" / f"{APP_ID}-{digest}.svg"
    desktop = (build / f"{APP_ID}.desktop").read_text()
    command = "/usr/bin/env " + " ".join([
        exec_argument(f"GSETTINGS_SCHEMA_DIR={build}"),
        exec_argument(str(build / "houra")),
    ])
    desktop = re.sub(r"^Exec=.*$", lambda _: f"Exec={command}", desktop, flags=re.MULTILINE)
    desktop = re.sub(r"^Icon=.*$", lambda _: f"Icon={desktop_value(str(icon))}", desktop, flags=re.MULTILINE)

    atomic_write(icon, content)
    atomic_write(icons / "scalable/apps" / f"{APP_ID}.svg", content)
    atomic_write(icons / "symbolic/apps" / f"{APP_ID}-symbolic.svg",
                 (source_icons / "symbolic/apps" / f"{APP_ID}-symbolic.svg").read_bytes())
    launcher = data / "applications" / f"{APP_ID}.desktop"
    atomic_write(launcher, desktop.encode())
    extension = data / "gnome-shell/extensions/houra@majamato.github.io"
    if extension.is_dir():
        atomic_write(extension / "icons/houra-symbolic.svg",
                     (source_icons / "symbolic/apps" / f"{APP_ID}-symbolic.svg").read_bytes())
        print("Updated the installed top-bar icon. Log out and back in to refresh it.")
    for previous in icon.parent.glob(f"{APP_ID}-*.svg"):
        if previous != icon and re.fullmatch(rf"{re.escape(APP_ID)}-[0-9a-f]{{16}}\.svg", previous.name):
            previous.unlink()
    refresh_cache("gtk4-update-icon-cache", icons, "-qtf")
    refresh_cache("update-desktop-database", launcher.parent)
    print(f"Updated the local Houra launcher and icon:\n  {launcher}")
    print("Quit and reopen Houra to use the rebuilt application.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"Cannot install Houra's desktop launcher: {error}", file=sys.stderr)
        sys.exit(1)
