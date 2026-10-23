# KetchUp branding

An extruded K with cream faces on a tomato-red (`#D94836`) badge, inspired by the geometry of 3D modeling tools.

- `logo.svg`: scalable application badge; also used for the favicon and loading screen.
- `mark.svg`: transparent red mark for use outside the badge.
- `icon-*.png`: raster badges from 16 to 1024 pixels; the native window uses `icon-256.png`.
- `favicon.ico`: Windows/browser icon containing 16, 32, 48, 64, 128, and 256 pixel images.
- `ketchup.icns`: macOS icon asset for application packaging.
- `apple-touch-icon.png`: opaque 180 pixel Apple touch icon.
- `icon-maskable-512.png`: opaque badge with extra padding for launcher masks.
- `site.webmanifest`: browser application metadata and launcher icons; paths support hosting in a subdirectory.

The native toolbar uses generated egui vector geometry, so it stays sharp at any display scale without a new runtime dependency. The desktop executable sets its window icon; the ICO and ICNS assets are also available for future installers/application bundles.

GNOME on Wayland resolves the icon from the `io.github.Ketchup3D.Ketchup` application ID, its matching desktop entry, and the installed icon theme assets. Run `python3 scripts/install_linux_desktop.py` after compiling to install the launcher and icons for your user. The desktop entry's `StartupWMClass=ketchup-app` also associates X11 windows with the launcher.

To change the mark, edit `scripts/generate_branding.py`, then run `python3 scripts/generate_branding.py` from the repository root. This regenerates the SVG, PNG, ICO, ICNS, and Rust geometry together. It requires Python 3, Pillow, and `rustfmt`. Update the UI accent, HTML theme color, and manifest theme color if changing the brand red.
