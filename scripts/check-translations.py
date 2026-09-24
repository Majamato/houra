#!/usr/bin/env python3
"""Check catalog coverage and named placeholders before shipping translations."""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLACEHOLDER = re.compile(r"\{[a-z_]+\}")


def field(lines: list[str], name: str) -> str | None:
    for index, line in enumerate(lines):
        if line.startswith(name + " "):
            parts = [line[len(name) + 1 :]]
            for continuation in lines[index + 1 :]:
                if not continuation.startswith('"'):
                    break
                parts.append(continuation)
            return "".join(json.loads(part) for part in parts)
    return None


def catalog(path: Path) -> dict[str, tuple[str | None, list[str]]]:
    entries = {}
    for block in path.read_text().split("\n\n"):
        lines = block.splitlines()
        source = field(lines, "msgid")
        if source is None or not source:
            continue
        plural = field(lines, "msgid_plural")
        translations = []
        if plural is None:
            translations.append(field(lines, "msgstr") or "")
        else:
            for index in range(3):
                value = field(lines, f"msgstr[{index}]")
                if value is None:
                    break
                translations.append(value)
        entries[source] = plural, translations
    return entries


def main() -> int:
    expected = catalog(ROOT / "po/houra.pot")
    failures = []
    for language in (ROOT / "po/LINGUAS").read_text().splitlines():
        if not language or language.startswith("#"):
            continue
        path = ROOT / f"po/{language}.po"
        if not path.exists():
            failures.append(f"{language}: missing catalog")
            continue
        actual = catalog(path)
        if set(actual) != set(expected):
            failures.append(f"{language}: catalog differs from houra.pot")
        for source, (plural, values) in actual.items():
            if not values or any(not value for value in values):
                failures.append(f"{language}: untranslated {source!r}")
            placeholders = set(PLACEHOLDER.findall(source))
            if plural:
                placeholders.update(PLACEHOLDER.findall(plural))
            for value in values:
                if set(PLACEHOLDER.findall(value)) != placeholders:
                    failures.append(f"{language}: placeholders differ for {source!r}")
        result = subprocess.run(
            ["msgfmt", "--check", "--check-format", "--output-file", "/dev/null", str(path)],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode:
            failures.append(f"{language}: {result.stderr.strip()}")
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print(f"Checked {len(expected)} messages in all catalogs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
