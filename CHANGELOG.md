# Changelog

## 2.1.0

- Windows installer: `Shot-2.1.0-windows-x64-setup.exe` installs Shot for the current user with
  no administrator rights, adds it to the Start menu and to Settings → Apps, and updates the 1.0
  single file in `%LOCALAPPDATA%\Programs\Shot` in place, start at login included. Settings →
  Apps uninstalls it.
- macOS comes as a disk image, `Shot-2.1.0-macos.dmg`: drag Shot onto Applications.
- The preview can show in the bottom-left or the bottom-right corner (Settings → Угол превью).
- Editor: the arrow is one tapered shape with a notched head.
- Editor: the select tool picks up a rectangle or an ellipse by its inside, not only by its
  outline; a selected one shows its handles without an extra box.
- A new app and tray icon.

## 1.0.0

The first release, for Windows 10 22H2 and 11 (x64) and macOS 14 and later (Apple silicon and
Intel).

- Capture: `Ctrl+Shift+2` on Windows, `⇧⌘2` on macOS, freezes every monitor at once. Pick a
  window, drag an area, or press `Space` for the whole screen.
- The shot goes to the clipboard, to a PNG in `Pictures/Shot`, or both, and shows a preview in
  the corner.
- Editor: arrow, line, rectangle, ellipse and numbered or lettered counters; every mark stays
  movable and editable, with undo and redo.
- Settings window: hotkey recording, destination, folder, preview, start at login, light or dark
  theme.
- Runs in the tray or the menu bar; no network access.
