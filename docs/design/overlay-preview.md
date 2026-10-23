# Overlay and preview

Forms and behavior of the capture overlay and the preview card. Colors, type and radii are
tokens from [tokens.md](tokens.md).

Sizes are logical px at 100% scale. On a monitor with scale `s`, every length is `round(len × s)`
and every stroke width is `max(1, round(w × s))`. Sizes shown to the user (the size label) are
physical px.

## Overlay

### Layers, bottom to top

1. Frozen frame: the monitor image taken at the hotkey. Nothing on screen moves while choosing.
2. Dim: `overlay.dim` over everything outside the target rect, drawn as 4 strips.
3. Frame and halo, outside the target rect.
4. Key hint bar.
5. Size label.
6. Crosshair. The system cursor is hidden over the overlay.

The target rect is the hovered window (state A) or the dragged area (state B). There is always a
target: over the desktop it is the whole monitor. The overlay never ends up in the image: the
image is cut from the frozen frame.

### State A: hover a window

- The window under the cursor is not dimmed. Frame 2 px plus halo 1 px, outside the window rect.
- The window rect comes from the window list taken at the hotkey, clipped to the monitor.
- Click captures that window. Over the desktop, click captures the whole monitor.
- The size label shows the window size.

### State B: drag an area

- A drag starts once the cursor moves past the system drag threshold with the button down
  (`SM_CXDRAG` / `SM_CYDRAG` on Windows, 4 px on macOS). A release before that is a click.
- The window highlight goes away. The area is not dimmed. Frame 1 px plus halo 1 px, outside the
  area.
- The area spans from the press point to the hot spot, inclusive of both.
- The crosshair sits at the corner being dragged. The size label shows the area size.
- Release captures at once; there is no adjust step.
- The area is clamped to the monitor where the drag started: when the cursor goes past that
  monitor's edge, the area stops at the edge.

### Keys

| Input | Action |
|---|---|
| Click | Capture the hovered window |
| Drag | Capture the area |
| `Space` | Capture every monitor, one file each; the clipboard gets the monitor under the cursor |
| `Esc`, right click | Cancel |

### Geometry

| Element | Value |
|---|---|
| Area frame | 1 px `overlay.frame`, directly outside the area |
| Window frame | 2 px `overlay.frame`, directly outside the window |
| Frame halo | 1 px `overlay.frame-halo`, directly outside the frame |
| Crosshair | 4 arms of 8 × 1 px `overlay.crosshair`, starting 3 px from the hot spot; the hot spot pixel and 2 px around it stay uncovered |
| Crosshair halo | 1 px `overlay.crosshair-halo` around each arm (each arm is drawn as 10 × 3 halo, then 8 × 1 white) |
| Size label | Height 20, padding 6 horizontal and 2 vertical, `radius.xs`, `overlay.label-bg` |
| Size label text | `type.label`, tabular digits, `overlay.label-text`; text `W × H`, the `×` in `overlay.label-text-dim`, spaces around it |
| Key hint bar | Height 32, padding 14 horizontal and 8 vertical, `radius.md`, `overlay.label-bg` |
| Key hint pairs | Key in `type.label` `overlay.label-text`, action in `type.caption` `overlay.label-text-dim`; 6 between key and action, 16 between pairs |

Key hint content, in order: `Клик` окно · `Протянуть` область · `Space` весь экран · `Esc` отмена.

### Placement

- Size label: top-left at hot spot + (16, 16). If its right edge would come closer than 8 px to
  the monitor's right edge, place it at `x = hot.x − 16 − width`. Same for the bottom edge:
  `y = hot.y − 16 − height`.
- Key hint: centered horizontally in the cursor monitor's work area, bottom edge 24 px above the
  work-area bottom (above the taskbar or Dock).
- When the hot spot comes within 96 px of the hint bar, the bar moves to 24 px below the
  work-area top. It moves back once the hot spot is farther than 96 px from the bottom position.
- The key hint is shown for the first 5 captures, then never again (a counter in
  `settings.toml`).

### Several monitors

- Every monitor gets the dim, and the frame where the target rect crosses it.
- The crosshair, size label and key hint appear only on the monitor with the cursor.

## Preview

### Card

| Part | Value |
|---|---|
| Card | `surface.base`, `radius.lg`, 1 px `border.default` inside, `shadow.lg`. Width 248. Padding 4 at top, left and right |
| Thumbnail box | 240 × `clamp(round(h × min(240 / w, 1)), 64, 240)`: the image's height at 1 : 1 or narrowed to 240, `radius.md`, `surface.sunken` backing, 1 px `border.default` inside |
| Image in the box | Scaled to fit the box (contain), never above 1 : 1, centered |
| Status row | Height 28 below the thumbnail: 16 icon, 6 gap, `type.caption` text; 8 from the card's left edge |
| Close button | 24 circle, `overlay.label-bg`, `close` icon 16 in white; 8 from the card's top and right edges; hit area 32 |

A 16:9 capture gives a 248 × 167 card.

Examples of the thumbnail rule:

| Capture | Box | Image |
|---|---|---|
| 1920 × 1080 | 240 × 135 | 240 × 135 |
| 1080 × 1920 | 240 × 240 | 135 × 240 |
| 3440 × 600 | 240 × 64 | 240 × 42 |
| 90 × 40 | 240 × 64 | 90 × 40, 1 : 1 |

### States

| State | Icon | Text | Color | Close button |
|---|---|---|---|---|
| Copied and saved | `check`, `success` | Скопировано и сохранено | `text.secondary` | On hover |
| Clipboard only | `copy`, `icon.secondary` | Скопировано в буфер | `text.secondary` | On hover |
| File only | `save`, `icon.secondary` | Сохранено в Pictures\Shot | `text.secondary` | On hover |
| Save failed | `alert`, `danger` | Не сохранено: нет доступа к папке | `danger` | Always |
| Hover | `edit`, `accent.text` | Открыть в редакторе | `accent.text` | Yes |

- Hover: the card border becomes `border.strong`, the cursor becomes a hand, and the hide timer
  pauses.
- Click on the card opens the editor and removes the card. Click on the close button removes the
  card.
- The file-only text names the actual folder, shortened from the left if it does not fit.

### Placement and stack

No mockup shows the stack; this section is its only description.

- The card appears on the monitor where the cursor was at the hotkey, in a bottom corner of its
  work area: the left one, or the right one when `preview_side = "right"` (settings, `Угол
  превью`). 16 px from that side edge and 16 px above the bottom edge (above the taskbar or
  Dock).
- Several cards stack upward with an 8 px gap. The newest takes the bottom slot; older cards move
  up one slot.
- When the next card would cross 16 px below the work-area top, the oldest card is removed.
- A card keeps the corner it appeared in. When the corner changes while cards are shown, the
  new cards start a stack of their own in the other corner.
- A full-screen capture of several monitors adds one card per file.

### Timing

- Each card hides 5 s after it appears (`preview_seconds` in `settings.toml`). A save error does
  not hide by itself.
- While the cursor is over any card, every card's timer pauses. Each resumes with its remaining
  time when the cursor leaves the stack.
- Cards appear and disappear with a 120 ms fade (`motion.fade`). Nothing slides.
- When a card goes away, the cards above it move down at once, but not while the cursor is over
  the stack: the stack is re-laid out when the cursor leaves it.
- The card never takes focus from the window the user works in.
