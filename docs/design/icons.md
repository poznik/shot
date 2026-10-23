# App and tray icons

The app icon and the tray / menu-bar icon. Tool icons are in [tokens.md](tokens.md#icons).

## The mark

A lightning bolt in a capture frame: a rounded square ring cut through at the top-right and
bottom-left corners, along one diagonal. It comes in two styles:

- **Volumetric**: violet with light and shade. Big icons: the taskbar, the file, Alt+Tab, Finder,
  the settings and first-run windows.
- **Flat**: one color, `#7A3FE7`. Small icons: the title bar, the tray, the menu bar, the
  smallest sizes of the file icon.

| File | What |
|---|---|
| [`assets/app/app.png`](../../assets/app/app.png) | The volumetric source: the designer's picture on transparent, cropped to the glyph |
| [`assets/app/app-flat.svg`](../../assets/app/app-flat.svg) | The flat icon on a 1024 grid, for design tools; the source is the geometry in `tools/icons.py` |

Flat geometry, in units of the frame's outer side: corner radius 0.25, bar 0.132, cut 0.05 wide
along the line from the bottom-left to the top-right corner. The bolt is traced from the
designer's picture.

## App icon

- **Windows**, [`assets/app/windows/`](../../assets/app/windows/):
  - `app-flat-16` … `app-flat-48` (16, 20, 24, 32, 40, 48): the window's small icon (title bar);
  - `app-32` … `app-256` (32, 40, 48, 64, 256): the volumetric glyph, its longer side 92% of the
    icon; the window's big icon (taskbar, Alt+Tab).
  - `shot.exe` carries an icon of flat 16, 20, 24 and volumetric 32, 40, 48, 64, 256; the build
    script of the `shot` crate packs it.
- **macOS**, [`assets/app/macos/Shot.iconset/`](../../assets/app/macos/Shot.iconset/), the 10
  files `iconutil -c icns` expects: flat at 16 and 32 px, volumetric from 64 px, fitted into the
  824 / 1024 body of Apple's icon grid.
- The settings header and the first-run windows draw it from the volumetric PNG nearest to the
  needed physical size (40 and 64 logical px).

## Tray icon

The flat mark. The alert variant puts `!` in place of the bolt (see
[settings.md](settings.md#alert-state)).

Each size is drawn on its own pixel grid, so the frame's straight edges stay sharp:

| Size | Margin | Bar | `!` width |
|---|---|---|---|
| 16 | 1 | 2 | 2 |
| 20 | 1 | 2 | 2 |
| 24 | 1 | 3 | 4 |
| 32 | 2 | 4 | 4 |
| 40 | 2 | 5 | 6 |
| 48 | 3 | 6 | 6 |
| macOS 18 | 1 | 2 | 2 |
| macOS 36 | 2 | 4 | 4 |

- **Windows**: the tray takes 16 at 100%, 20 at 125%, 24 at 150%, 32 at 200%, 40 at 250%, 48 at
  300%. Two PNG sets, chosen by the taskbar theme (`SystemUsesLightTheme` under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`), switched on
  `WM_SETTINGCHANGE`:
  - [`windows/light/`](../../assets/tray/windows/light/): for a light taskbar, `#7A3FE7`;
  - [`windows/dark/`](../../assets/tray/windows/dark/): for a dark taskbar, the lighter `#A07BFF`
    (5:1 on `#202020`; the base violet is under 3:1).
- **macOS**: template images [`macos/trayTemplate.png`](../../assets/tray/macos/) and `@2x`,
  `trayAlertTemplate.png` and `@2x`, black on transparent; `isTemplate = true`, the system tints
  them.

## Regenerating the PNGs

`uv run --no-project --with pillow --with resvg-py python tools/icons.py` writes every PNG above
and `app-flat.svg`. A new volumetric picture goes to `assets/app/app.png` first: transparent
around the glyph (no stray half-transparent pixels) and cropped to it.
