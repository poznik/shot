# App and tray icons

The app icon and the monochrome tray / menu-bar icon. Tool icons are in
[tokens.md](tokens.md#icons).

## App icon

A viewfinder with a numbered counter: four white corner brackets on an indigo rounded square, a
yellow `annotation.yellow` counter with `1` in the middle. It names both halves of Shot: the
capture and the annotation.

| File | Use |
|---|---|
| [`assets/app/app.svg`](../../assets/app/app.svg) | Windows, 40 px and up: the body fills the frame with a 4% margin |
| [`assets/app/app-small.svg`](../../assets/app/app-small.svg) | Windows, 32 px and down: no digit, thicker brackets |
| [`assets/app/app-macos.svg`](../../assets/app/app-macos.svg) | macOS, every size: the 824 / 1024 body of Apple's icon grid with its drop shadow |

- Body: a vertical gradient `#5A65FF` → `#3A44E0` (the `accent` family). Brackets: white, round
  caps, stroke 64 / 1024. Counter: `#FFD426`, digit `#16161A`, stroke 44 / 1024.
- PNGs:
  - Windows: [`assets/app/windows/`](../../assets/app/windows/) at 16, 20, 24, 32, 40, 48, 64
    and 256. The `.ico` for the executable packs all of them.
  - macOS: [`assets/app/macos/Shot.iconset/`](../../assets/app/macos/Shot.iconset/), the 10 files
    `iconutil -c icns Shot.iconset` expects.
- The settings header and the first-run windows draw it from the PNG nearest to the needed
  physical size (40 and 64 logical px).

## Tray icon

The same viewfinder, monochrome: four corner brackets and a dot. The alert variant puts `!` in
place of the dot (see [settings.md](settings.md#alert-state)).

The glyph is drawn on the pixel grid of each size, so the corners stay sharp: one SVG per size in
[`assets/tray/`](../../assets/tray/), all black on transparent.

| Size | Margin | Stroke | Arm | Dot | `!` width |
|---|---|---|---|---|---|
| 16 | 1 | 2 | 5 | 4 | 2 |
| 20 | 1 | 2 | 6 | 6 | 2 |
| 24 | 2 | 3 | 7 | 6 | 2 |
| 32 | 2 | 4 | 10 | 8 | 4 |
| 40 | 2 | 4 | 12 | 12 | 4 |
| 48 | 4 | 6 | 14 | 12 | 6 |
| macOS 18 | 1 | 2 | 6 | 4 | 2 |
| macOS 36 | 2 | 3 | 11 | 8 | 4 |

- **Windows**: the tray takes 16 at 100%, 20 at 125%, 24 at 150%, 32 at 200%, 40 at 250%, 48 at
  300%. Two PNG sets, chosen by the taskbar theme (`SystemUsesLightTheme` under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`), switched on
  `WM_SETTINGCHANGE`:
  - [`windows/light/`](../../assets/tray/windows/light/): for a light taskbar, black at 89%
    (`#000000E4`);
  - [`windows/dark/`](../../assets/tray/windows/dark/): for a dark taskbar, white.
- **macOS**: template images [`macos/trayTemplate.png`](../../assets/tray/macos/) and `@2x`,
  `trayAlertTemplate.png` and `@2x`; `isTemplate = true`, the system tints them.

## Regenerating the PNGs

The SVGs are the source. Render each PNG at its exact pixel size with resvg (the `resvg` CLI or
the `resvg-py` package), e.g. `resvg -w 16 -h 16 assets/tray/tray-16.svg out.png`. The Windows tray
PNGs are the black render with its color replaced and the alpha kept (× 0.89 for the light set).
