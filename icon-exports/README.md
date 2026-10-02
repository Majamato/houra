# Houra icon exports

The selected interval ring icon with H-shaped hands, exported with a transparent
background.

- `houra.svg`: editable vector, scalable to any size.
- `houra-symbolic.svg`: monochrome version for the top bar and symbolic icon themes.
- `houra-WIDTHxHEIGHT.png`: PNGs at 16, 24, 32, 48, 64, 96, 128, 192,
  256, 512 and 1024 pixels square.

The application's source icon is
[`io.github.majamato.Houra.svg`](../data/icons/hicolor/scalable/apps/io.github.majamato.Houra.svg).
These exports are snapshots of that icon for use elsewhere.

To regenerate a PNG with ImageMagick, run from the repository root:

```sh
magick -background none -density 1536 \
  data/icons/hicolor/scalable/apps/io.github.majamato.Houra.svg \
  -resize 512x512 -strip PNG32:icon-exports/houra-512x512.png
```
