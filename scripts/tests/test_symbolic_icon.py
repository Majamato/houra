"""Check the icon under GNOME Shell's fill-only symbolic recoloring."""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
STABLE = ROOT / "data/icons/hicolor/symbolic/apps/io.github.majamato.Houra-symbolic.svg"
DEVEL = ROOT / "data/icons/hicolor/symbolic/apps/io.github.majamato.Houra.Devel-symbolic.svg"


@unittest.skipUnless(shutil.which("magick"), "ImageMagick is required for SVG rendering")
class SymbolicIconTests(unittest.TestCase):
    def render(self, svg):
        with tempfile.TemporaryDirectory(prefix="houra-symbolic-") as directory:
            path = Path(directory) / "icon.svg"
            path.write_text(svg)
            return subprocess.run(
                ["magick", "-background", "none", f"RSVG:{path}",
                 "-depth", "8", "rgba:-"],
                check=True, capture_output=True,
            ).stdout

    def test_shell_recoloring_preserves_the_icon_shape(self):
        for source, variant in [(STABLE, "stable"), (DEVEL, "devel")]:
            with self.subTest(variant=variant):
                svg = source.read_text()
                for color in ["#ffffff", "#222226", "#3584e4"]:
                    for opacity in [1, 140 / 255]:
                        with self.subTest(color=color, opacity=opacity):
                            # StIconTheme overrides shape fills, but leaves strokes alone.
                            css = ("<style>rect,path,ellipse,circle,polygon {"
                                   f"fill: {color} !important;" + "}</style>")
                            opening = svg.index(">") + 1
                            themed = svg[:opening] + css + svg[opening:]
                            expected = svg.replace("#2e3436", color)
                            # Shell dims the whole rendered icon when the timer is paused.
                            def dim(image):
                                start = image.index(">") + 1
                                return (image[:start] + f'<g opacity="{opacity}">' +
                                        image[start:].replace("</svg>", "</g></svg>"))
                            self.assertEqual(self.render(dim(themed)), self.render(dim(expected)),
                                             "Shell recoloring changes the symbolic icon's shape or color")

    def test_export_matches_the_installed_source(self):
        self.assertEqual(STABLE.read_bytes(),
                         (ROOT / "icon-exports/houra-symbolic.svg").read_bytes())


class DevelIconTests(unittest.TestCase):
    def test_dev_icon_identifies_the_development_variant(self):
        self.assertIn("<title>Houra Dev</title>", DEVEL.read_text())


if __name__ == "__main__":
    unittest.main()
