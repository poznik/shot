# Shot

A fast screenshot tool for Windows and macOS, with the same look and the same keys on both.

![Shot: drag an area, then mark it up in the editor](media/demo.gif)

- **Fast.** The hotkey freezes the screen in about 35 ms. Idle in the background, Shot takes
  about 2 MB of memory on Windows and under 10 MB on macOS. On Windows it is one file of under
  2 MB.
- **Frozen screen.** Shot captures every monitor the moment you press the hotkey, so open menus
  and tooltips stay put while you choose what to keep.
- **Window, area or whole screen**, at full resolution on high-DPI and Retina displays.
- **An editor for quick marks**: arrows, lines, rectangles, ellipses and numbered steps. Every
  mark stays an object you can move, resize, recolor or delete.
- **Private.** No network access, no telemetry, no account.

The interface is in Russian for now.

## Install

Download the latest build from [Releases](https://github.com/poznik/shot/releases). The builds
are not signed by Microsoft or Apple, so each system warns you once before the first start.

### Windows 10 22H2 or 11

1. Download `Shot-<version>-windows-x64.exe` and put it in a folder where it will stay, such as
   `%LOCALAPPDATA%\Programs\Shot`. Shot is one file with no installer; start at login points to
   that file.
2. Run it. If SmartScreen says "Windows protected your PC", click **More info**, then **Run
   anyway**.
3. Shot lives in the notification area. The first start offers to start Shot at login.

### macOS 14 or later

1. Download `Shot-<version>-macos.zip`, open it and move **Shot.app** to **Applications**.
2. Open Shot. macOS says it cannot verify the app: click **Done**, open **System Settings →
   Privacy & Security**, scroll down and click **Open Anyway** next to Shot.
3. Shot needs the **Screen Recording** permission: its window has a button that opens the right
   page of System Settings. Turn Shot on there.
4. Shot lives in the menu bar and has no Dock icon.

## Use

Press **Ctrl+Shift+2** on Windows or **⇧⌘2** on macOS. The screen freezes, then:

| Do | To |
|---|---|
| Click a highlighted window | Capture the window |
| Drag | Capture an area |
| `Space` | Capture each monitor to its own file; the clipboard gets the one under the cursor |
| `Esc` or right click | Cancel |

The shot goes to the clipboard and to `Pictures/Shot` as `Shot 2026-10-22 17-03-15.png`, and a
preview shows in the corner for a few seconds. Click the preview to open the editor.

### Editor

`Ctrl` is `Cmd` on macOS.

| Key | Action |
|---|---|
| `A` `L` `R` `O` `N` | Arrow, line, rectangle, ellipse, counter; `N` again switches 1, 2, 3 and A, B, C |
| `V` | Select: move, resize with the handles, `Del` to delete |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo, redo (also `Ctrl+Y` on Windows) |
| `Ctrl+C` | Copy the result |
| `Ctrl+S` | Save over the same file and copy |
| `Enter` | Save and close |
| `Esc` | Close; with unsaved edits, Shot asks first |
| `Ctrl` + wheel, `Ctrl+0`, `Ctrl+1` | Zoom, fit, 100% |

### Settings

Open **Настройки…** (Settings) from the tray or menu bar icon to change the hotkey, where shots go
(clipboard, file or both), the folder, the preview, start at login and the theme. Starting Shot
again while it runs opens the same window. The settings are a plain file:
`%APPDATA%\Shot\settings.toml` on Windows, `~/Library/Application Support/Shot/settings.toml` on
macOS.

## Uninstall

Turn off **Автозапуск** (start at login) in the tray or menu bar menu, quit Shot, and delete the program. Its
settings are in `%APPDATA%\Shot` (Windows) or `~/Library/Application Support/Shot` (macOS).

## Build from source

Shot is written in Rust (stable, edition 2024) and draws its own interface with
[tiny-skia](https://github.com/linebender/tiny-skia); it uses no web view and no GPU context.

```sh
cargo build --release
```

That gives `target/release/shot.exe` on Windows and `target/release/shot` on macOS. To make
`Shot.app` on a Mac, run `tools/bundle-mac.sh`; add `--universal` for Apple silicon and Intel in
one app (needs both Rust targets). macOS ties the Screen Recording permission to the app's
signature: sign your builds with one certificate from `tools/mac-sign-cert.sh` to keep the
permission across rebuilds.

The forms and behavior of every window, and the design tokens the code draws with, are in
[docs/design](docs/design).

## License

[MIT](LICENSE)
