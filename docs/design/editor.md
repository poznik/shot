# Editor

Forms and behavior of the editor window. Colors, type and radii are tokens from
[tokens.md](tokens.md).

Sizes are logical px at 100% scale, scaled as in [overlay-preview.md](overlay-preview.md):
lengths `round(len × s)`, strokes `max(1, round(w × s))`. Annotation sizes are image pixels; see
[tokens.md](tokens.md#annotations).

## Window

- An ordinary OS window with the system frame and title bar. The title is the file name. With
  unsaved edits: a `*` before the name on Windows, `setDocumentEdited` on macOS.
- Opens on the monitor that had the cursor, centered in its work area.
- Content size: the image at fit scale plus the margins below, at most 90% of the work area,
  at least 960 × 600. The toolbar always fits at 960.
- Background `bg.canvas`.

## Image

- Image area: 24 from the left, right and bottom content edges, 68 from the top (12 + toolbar
  44 + 12).
- Fit: the largest scale that fits the image area, never above 100%. The image is centered in
  the image area.
- 100% means one image pixel per physical screen pixel.
- Edge: 1 px `border.default` outside the image, plus `shadow.sm`.
- Sampling: a smoothing filter below 100%; nearest neighbour at 100% and above, so pixels stay
  sharp.
- Zoom: `Ctrl` + wheel (`Cmd` on macOS) and trackpad pinch, around the cursor, through the steps
  10, 25, 33, 50, 67, 100, 150, 200, 300, 400, 800%. `Ctrl+0`: fit. `Ctrl+1`: 100%.
- When the image is larger than the image area: the wheel scrolls vertically, `Shift` + wheel
  horizontally, `Space` + drag pans with a hand cursor. The image scrolls under the toolbar.

### Zoom label

Bottom-right, 12 from the content's right and bottom edges. Height 24, padding 8 horizontal,
`radius.sm`, `surface.base`, 1 px `border.default` inside, `shadow.sm`. Text: the zoom in whole
percent, `type.label`, `text.secondary`, tabular digits. Hover adds `fill.hover`. Click switches
between fit and 100%.

## Toolbar

Floating, centered horizontally, 12 below the content top. 888 × 44, `surface.base`, `radius.md`,
1 px `border.default` inside, `shadow.md`, padding 6. The toolbar never changes its width or
content: switching tools moves nothing under the cursor.

Groups, left to right, separated by a 1 × 20 `border.default` divider with 12 on each side:

| Group | Content | Size |
|---|---|---|
| Tools | `select` V, `arrow` A, `line` L, `rectangle` R, `ellipse` O, `counter-number` N, `counter-letter` N | 7 × 32, gap 4 |
| Colors | The 8 annotation colors | 8 cells 28 × 32, gap 4 |
| Width | S, M, L | 3 × 32, gap 4 |
| History | `undo`, `redo` | 2 × 32, gap 4 |
| Output | `copy`, `save`, `close` | 3 × 32, gap 4 |

- `N` picks the counter; pressed again, it switches between 1, 2, 3 and A, B, C.
- Color cell: a 20 circle with a 1 px `border.strong` outline inside its edge.
- Width button: a 16-long bar, 2 / 4 / 8 high, round ends, `icon.primary`.
- Color and width apply to the next object of the current tool and to the selected object.
  While an object is selected, the toolbar shows its color and width.

### Button states

| State | Tool and action buttons | Color cell | Width button |
|---|---|---|---|
| Default | Transparent, `icon.primary` | Outline only | Transparent |
| Hover | `fill.hover` | 2 gap + 2 ring `border.strong` | `fill.hover` |
| Pressed | `fill.pressed` | — | `fill.pressed` |
| Active / selected | `accent.solid`, icon `text.on-accent` | 2 gap + 2 ring `accent.solid` (28 outer) | `fill.pressed` |
| Active + hover | `accent.hover` | — | — |
| Active + pressed | `accent.pressed` | — | — |
| Disabled | Icon `text.disabled` | — | — |
| Keyboard focus | 2 px `focus.ring`, offset 2 | Same | Same |

The gap around a selected or hovered color cell is `surface.base`. Undo and redo are disabled
when there is nothing to undo or redo.

### Tooltip

Shown after 500 ms of hover, 8 below the toolbar's bottom edge, centered on the button; hidden on
press or when the cursor leaves. Height 24, padding 8 horizontal, `radius.sm`, `surface.raised`,
1 px `border.default` inside, `shadow.sm`. The name in `type.caption` `text.primary`, then 8, then
the shortcut in `text.secondary`: `Стрелка  A`, `Отменить  Ctrl+Z`.

Names: Выделение, Стрелка, Линия, Прямоугольник, Эллипс, Счётчик 1, 2, 3, Счётчик A, B, C;
Отменить, Повторить, Копировать, Сохранить, Закрыть.

## Objects

| | S | M | L |
|---|---|---|---|
| Stroke `w` | 2 | 4 | 8 |
| Arrowhead length `L` | 16 | 22 | 34 |
| Counter diameter `d` | 22 | 28 | 36 |
| Counter glyph, Inter Semi Bold | 12 | 15 | 19 |
| Halo, each side | 1 | 1 | 1 |

- **Line**: from press to release, round caps.
- **Arrow**: the shaft is a line from the tail with a round cap, ending 2 px inside the head. The
  head is a triangle: tip at the end point, length `L = 3w + 10` along the shaft, width `L` across
  it (half-width `L/2`). It is filled and stroked with `w/2` and round joins, which rounds its
  corners.
- **Rectangle**: the drag rect, stroke centered on its edge, round joins, no fill.
- **Ellipse**: inscribed in the drag rect, stroke centered on its edge, no fill.
- **Counter**: centered on the click. A filled circle of diameter `d` in the annotation color,
  the glyph in the counter-glyph color from tokens.md, centered, tabular digits. One or two
  characters sit in the circle; three or more make a capsule of height `d` and width
  `text width + d/2`.
- **Numbering**: the next counter is the largest of its kind on the image plus one; digits and
  letters count separately. Deleting does not renumber. After Z comes AA.
- **Shift** while drawing: lines and arrows snap to 15° steps, a rectangle becomes a square, an
  ellipse a circle.
- **Halo**: every object's halo is the same geometry stroked 2 px wider (filled parts stay
  filled). Draw all halo parts of one object opaque into a separate layer, then blend that layer
  at the halo alpha, so overlapping parts (shaft under head) do not get darker.

## Hover and selection

Colors are the theme-independent `selection` and `selection.handle-fill` from
[tokens.md](tokens.md#selection).

- **Hover** (select tool): the object's halo is replaced by a 1 px `selection` outline (the same
  halo geometry, opaque).
- **Selected line or arrow**: the outline plus two handles at the end points.
- **Selected rectangle or ellipse**: the halo stays; a 1 px `selection` box on the drag rect and
  8 handles at its corners and edge midpoints.
- **Selected counter**: the outline only; its size comes from the width.
- **Handle**: a circle of diameter 10, `selection.handle-fill` with a 2 px `selection` ring
  inside its edge. Hit area 20 × 20. Handles keep their size at any zoom.
- A drawing tool always draws; the new object is selected at once. The selected object's handles
  work under any tool. To select another object, use the select tool.
- Cursors: crosshair while drawing; move over an object with the select tool; a resize cursor in
  the handle's direction over a handle; hand while panning.

## Keys

`Ctrl` is `Cmd` on macOS.

| Input | Action |
|---|---|
| `V` `A` `L` `R` `O` `N` | Tools; `N` again switches the counter mode |
| `Del`, `Backspace` | Delete the selected object |
| Arrow keys | Move the selected object by 1 px; with `Shift`, by 10 |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z`, and `Ctrl+Y` on Windows | Redo |
| `Ctrl+C` | Copy the result |
| `Ctrl+S` | Save over the same file and copy the result |
| `Enter` | Save per the destination setting and close |
| `Esc` | Close; with unsaved edits, ask first |
| `Ctrl+0`, `Ctrl+1` | Fit, 100% |

## Notice

One notice at a time; a new one replaces the old. Bottom center, 16 above the content's bottom
edge. Height 32, padding 12 horizontal, `radius.md`, `surface.raised`, 1 px `border.default`
inside, `shadow.md`. A 16 icon, 8 gap, text in `type.body`. Shown for 2 s (an error for 5 s), with
the 120 ms fade.

| After | Icon | Text | Text color |
|---|---|---|---|
| `Ctrl+C` | `check`, `success` | Скопировано | `text.primary` |
| `Ctrl+S` | `check`, `success` | Сохранено и скопировано | `text.primary` |
| Save failed | `alert`, `danger` | Не сохранено: нет доступа к папке | `danger` |

## Unsaved edits

`Esc` or the window's close button with unsaved edits opens a dialog inside the editor window.

- Scrim over the whole content: `scrim` (`#0000004D` light, `#00000080` dark).
- Dialog: width 400, padding 24, `radius.lg`, `surface.base`, 1 px `border.default` inside,
  `shadow.lg`, centered.
- Title `type.title` `text.primary`: Сохранить правки? 8 below it, `type.body` `text.secondary`:
  В «<file name>» есть несохранённые правки.
- Buttons 24 below the text, height 32, padding 12, `radius.sm`, `type.body-strong`: Не сохранять
  on the left; Отмена and Сохранить on the right, 8 apart. Secondary buttons: transparent, 1 px
  `border.strong` inside, `text.primary`. Сохранить: `accent.solid`, `text.on-accent`.
- `Enter`: Сохранить (save over the file, copy, close). `Esc`: Отмена.
