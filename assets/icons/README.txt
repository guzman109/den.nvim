Den app icon - "firelit floor" (5a)

Four theme variants, each in its own folder:
  ember          dark, default
  ember-soft     dark, lifted
  ember-light    light, warm paper
  ember-lighter  light, neutral

Per variant:
  den-<variant>.svg         scalable master, full detail (64px and up)
  den-<variant>-small.svg   scalable, simplified for small sizes
  den-<variant>-<n>.png     16, 24, 32, 48, 64, 128, 256, 512, 1024 px

Detail drops as size falls: 16-24px is silhouette only, 32-48px adds muzzle
and inner ears, 64px and up carries eyes, nose and mouth.
All artwork sits on a 512 grid with a 112 corner radius (22%), transparent
outside the rounded square.

macOS .icns
  mkdir den.iconset
  for s in 16 32 128 256 512; do cp den-ember-$s.png den.iconset/icon_${s}x${s}.png; done
  iconutil -c icns den.iconset

Linux
  install den-ember-<n>.png to hicolor/<n>x<n>/apps/den.png

Windows .ico
  magick den-ember-16.png den-ember-32.png den-ember-48.png den-ember-256.png den.ico
