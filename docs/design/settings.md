# Settings, first run and tray menu

Forms and behavior of the settings window, the first-run windows and the tray / menu-bar menu.
Colors, type and radii are tokens from [tokens.md](tokens.md); the icons are in
[icons.md](icons.md).

Sizes are logical px at 100% scale, scaled as in [overlay-preview.md](overlay-preview.md):
lengths `round(len × s)`, strokes `max(1, round(w × s))`.

## Controls

Shared by the settings and first-run windows. Every control shows keyboard focus as a 2 px
`focus.ring` outline, offset 2.

### Button

Height 32, padding 12 horizontal, `radius.sm`, `type.body-strong`, text centered.

| State | Primary | Secondary |
|---|---|---|
| Default | `accent.solid`, text `text.on-accent` | Transparent, 1 px `border.strong` inside, `text.primary` |
| Hover | `accent.hover` | `fill.hover` |
| Pressed | `accent.pressed` | `fill.pressed` |
| Disabled | `fill.pressed`, text `text.disabled` | 1 px `border.default` inside, text `text.disabled` |

### Toggle

Track 40 × 20, `radius.full`; thumb 12 (14 on hover), centered vertically, its center 10 from the
track's off-side end.

| State | Track | Thumb |
|---|---|---|
| Off | Transparent, 1 px `icon.secondary` inside | `icon.secondary`, left |
| On | `accent.solid` | `text.on-accent`, right |
| Hover | Off: `fill.hover`; on: `accent.hover` | 14 |
| Disabled | Off: 1 px `text.disabled`; on: `text.disabled` | Off: `text.disabled`; on: `surface.base` |

Click, `Space` or the label's click flips it. The change applies at once.

### Segmented control

Track: height 32, padding 2, `radius.sm`, `surface.sunken`, 1 px `border.default` inside.
Segments: height 28, padding 12 horizontal, `radius.xs`, `type.body`, each as wide as its text;
no gap.

| State | Segment |
|---|---|
| Default | Transparent, `text.secondary` |
| Hover | `fill.hover`, `text.primary` |
| Selected | `surface.raised`, 1 px `border.default` inside, `shadow.sm`, `text.primary` |
| Disabled | All text `text.disabled`; the selected segment keeps its fill, no shadow |

The weight never changes, so nothing moves on selection. `←` `→` move the selection when the
control has focus.

### Key cap

Height 20, padding 6 horizontal, min width 20, `radius.xs`, `surface.base` (`surface.raised` in
the dark theme), 1 px `border.strong` inside, `type.label` `text.primary`, text centered. Caps sit
4 apart.

- Windows names: `Ctrl`, `Alt`, `Shift`, `Win`, then the key: `2`, `S`, `F12`, `PrtSc`, `Space`.
  Order: Ctrl, Alt, Shift, Win, key.
- macOS uses symbols, one cap each, in the system order: `⌃` `⌥` `⇧` `⌘`, then the key. The
  default is `⇧` `⌘` `2`. Inter has none of U+2303, U+2325, U+21E7 and U+2318: `shot_core::text`
  draws them as strokes at the text's size, on the baseline and up to the cap height, as thick as
  the stems of the weight (the outlines of `⇧` and `⌘` at 0.7 of that), in any text.
- Large caps (first-run window): height 28, padding 8, `type.title`.

### Hotkey field

184 × 32, `radius.sm`, padding 6 horizontal, content left-aligned and vertically centered.

| State | Look | Content |
|---|---|---|
| Default | `surface.sunken`, 1 px `border.strong` inside | The hotkey as key caps |
| Hover | Adds `fill.hover` | Same |
| Recording | `surface.base`, 2 px `accent.solid` inside | `Нажмите сочетание` in `type.body` `text.secondary`; held modifiers replace it as caps |
| Error | 1 px `danger` inside | The working hotkey; the error line below the row |

## Hotkey recorder

1. Click, `Enter` or `Space` on the field starts recording. While recording, the global hotkey is
   unregistered, so pressing it does not start a capture.
2. Held modifiers show as caps at once.
3. A non-modifier key ends the combination:
   - Without a required modifier, recording goes on and the hint line (below the row,
     `type.body` `text.secondary`) says `Нужен Ctrl или Alt` (Windows) / `Нужен ⌘ или ⌃` (macOS).
     `Shift` and `Win` alone are not enough on Windows: `Shift+2` types text.
   - Otherwise Shot registers it. Success: the field shows it, `settings.toml` is written, no
     other message. Failure: the previous hotkey is registered again, the field shows it, and the
     error line names both.
4. `Esc`, a click outside the field or the window losing focus cancels recording; the previous
   hotkey stays.

Error line: below the row's label and control, 6 below them, a 16 `alert` icon in `danger`, 8,
the text in `type.body` `danger`.

| Case | Text |
|---|---|
| The OS refused the new hotkey | `Ctrl+Alt+S занято другой программой. Работает прежнее: Ctrl+Shift+2.` |
| The saved hotkey failed at startup | `Ctrl+Shift+2 занято другой программой. Запишите другое сочетание.` |
| macOS: `⇧⌘3`, `⇧⌘4`, `⇧⌘5` | `⇧⌘4 — системный снимок экрана macOS. Выберите другое сочетание.` |

The startup case also puts the tray icon into the alert state (see [Tray menu](#tray-menu)).
In text, a hotkey is written with `+` on Windows and as joined symbols on macOS.

## Settings window

An ordinary OS window with the system frame, not resizable, titled `Настройки Shot`. One instance:
opening it again brings it to the front. It opens centered on the monitor with the cursor.
Content 560 wide, its height follows the rows (530 with no error line). Background
`surface.base`. Every change applies and is saved at once; there is no Save button. `Esc` closes
the window unless a hotkey is being recorded.

Layout, top to bottom, padding 24:

1. Header, 40 high: the app icon at 40, 12, then `Shot` in `type.headline` over `Версия 1.0.0` in
   `type.caption` `text.secondary`.
2. 16, then seven rows. Each row has a 1 px `border.default` divider above it and is 56 high: on
   the left the label in `type.body-strong` `text.primary`, 2, the hint in `type.caption`
   `text.secondary`; on the right the controls, right-aligned, 12 apart, vertically centered. An
   error or hint line adds 24 to its row.
3. A divider, 12, then `Изменения сохраняются сразу.` in `type.caption` `text.secondary`.

| Row | Hint | Controls | Default |
|---|---|---|---|
| Хоткей | Снять область, окно или экран | Hotkey field | `Ctrl+Shift+2` / `⇧⌘2` |
| После снимка | Куда отправить снимок | Segmented: Буфер · Файл · Буфер и файл | Буфер и файл |
| Папка | The folder path, `type.caption`, elided in the middle to fit | Secondary button `Изменить…` | `%USERPROFILE%\Pictures\Shot` / `~/Pictures/Shot` |
| Превью | Карточка в углу после снимка | Segmented: 3 с · 5 с · 10 с, then a toggle | On, 5 с |
| Угол превью | Нижний угол экрана | Segmented: Слева · Справа | Слева |
| Автозапуск | Запускать Shot при входе в систему | Toggle | Set in the first-run window |
| Тема | Светлая или тёмная | Segmented: Как в системе · Светлая · Тёмная | Как в системе |

- `Изменить…` opens the native folder dialog: `IFileOpenDialog` with `FOS_PICKFOLDERS` on
  Windows, `NSOpenPanel` with `canChooseDirectories` on macOS. Title `Папка для снимков`.
- The preview duration is disabled while the preview toggle is off. The corner stays enabled:
  a failed save shows its card even with the preview off.
- The folder stays enabled with `Буфер`: the editor's `Ctrl+S` still saves there.
- Tab order follows the rows top to bottom, left to right.

## First run

Shown once, then `settings.toml` records it. Both windows are ordinary OS windows titled `Shot`,
not resizable, centered on the monitor with the cursor, content 480 wide, padding 24,
`surface.base`. macOS shows the Screen Recording window first and the welcome after it closes;
Windows shows only the welcome.

### Screen Recording (macOS)

Top to bottom:

1. The app icon at 64. 16.
2. `Разрешите Shot снимать экран`, `type.headline`. 8.
3. `type.body` `text.secondary`: `macOS показывает экран только программам с разрешением «Запись
   экрана». Shot снимает только по хоткею и ничего не отправляет в сеть.` 20.
4. Three steps, 10 apart: a 20 circle in `accent.subtle` with the number in `type.label`
   `accent.text`, 12, the text in `type.body`:
   1. `Нажмите «Открыть Системные настройки».`
   2. `Включите Shot в разделе «Запись экрана».`
   3. `Если macOS попросит перезапустить Shot, согласитесь.`
5. 20, the status: a 16 icon, 8, `type.body`. No permission: `alert` and the text
   `Разрешения пока нет` in `warning`. Granted: `check` in `success` and `Разрешение есть` in
   `text.primary`.
6. 24, buttons right-aligned, 8 apart: secondary `Позже`, primary `Открыть Системные
   настройки`. Once granted, they become a single primary `Готово`.

- `Открыть Системные настройки` calls `CGRequestScreenCaptureAccess()` once, so Shot appears in
  the list, then opens
  `x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture`.
- The status is checked again when Shot becomes active (`NSApplicationDidBecomeActive`); no
  timer.
- `Позже` closes the window. Without the permission the menu-bar icon is in the alert state, the
  hotkey opens this window instead of a capture, and so does the menu's first item.
- macOS 15 renamed the section (Screen & System Audio Recording): in Russian it is `Запись экрана
  и системного звука` on macOS 15.8, `Запись экрана` before. Step 2 names the start of both.

### Welcome

Top to bottom:

1. The app icon at 64. 16.
2. `Shot работает в фоне`, `type.headline`. 8.
3. `type.body` `text.secondary`. Windows: `Значок Shot — в трее у часов. Если его не видно, он
   под стрелкой ^.` macOS: `Значок Shot — в меню-баре справа.` 20.
4. `Снять экран:` in `type.body-strong`, 12, the hotkey in large key caps. 16.
5. Four rows, 24 high: on the left, 112 wide, a word in `type.body-strong` or a key cap; on the
   right `type.body` `text.secondary`.

   | Left | Right |
   |---|---|
   | Клик | окно под курсором |
   | Протянуть | область |
   | `Space` cap | весь экран |
   | `Esc` cap | отмена |

6. 20, a divider, 16, `Запускать Shot при входе в систему` in `type.body` with a toggle on the
   right, on by default.
7. 24, buttons: secondary `Настройки…` on the left, primary `Готово` on the right. `Enter`:
   Готово. `Esc` closes like Готово.

## Tray menu

Native menus: `TrackPopupMenu` on Windows, `NSMenu` on macOS. Shot only sets the items and the
icon.

| Windows | macOS | Action |
|---|---|---|
| **Снять**, `Ctrl+Shift+2` on the right | Снять `⇧⌘2` | Start a capture |
| — | — | |
| Настройки… | Настройки… `⌘,` | Open the settings window |
| Открыть папку | Открыть папку | Open the save folder |
| Автозапуск, check mark when on | Автозапуск, check mark when on | Flip autostart |
| — | — | |
| Выход | Завершить Shot `⌘Q` | Quit |

- Windows: `Снять` is the menu's default item (bold), and a left click on the icon runs it. A
  right click opens the menu. Tooltip: `Shot — Ctrl+Shift+2`.
- macOS: a click opens the menu. The shortcuts are shown with `keyEquivalent`; they act only
  while the menu is open, as on any status item.
- The hotkey shown is the current one.

### Alert state

When the hotkey failed to register, or on macOS the Screen Recording permission is missing, the
icon switches to its alert variant and the menu gets a first item and a divider:

| Problem | First item | Opens |
|---|---|---|
| Hotkey | `Хоткей не работает — изменить…` | The settings window |
| Permission (macOS) | `Нет разрешения на запись экрана…` | The Screen Recording window |

`Снять` loses its shortcut while the hotkey is broken and is disabled while the permission is
missing. Windows tooltip: `Shot — хоткей не работает`.
