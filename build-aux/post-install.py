#!/usr/bin/env python3
"""Refresh caches for a direct install; packagers run their own scriptlets."""

import os
import subprocess


def run_if_native_root(command: list[str]) -> None:
    if not os.environ.get("DESTDIR"):
        subprocess.run(command, check=False)


prefix = os.environ.get("MESON_INSTALL_PREFIX", "/usr/local")
run_if_native_root(["glib-compile-schemas", os.path.join(prefix, "share/glib-2.0/schemas")])
run_if_native_root(["gtk4-update-icon-cache", "-qtf", os.path.join(prefix, "share/icons/hicolor")])
