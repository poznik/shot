# Shot design tokens

The visual language as named values for code. Source of truth for colors and sizes in
`shot-core`.

- Colors are sRGB hex. Alpha is the last byte: `#RRGGBBAA`.
- Sizes are logical px at 100% scale; multiply by the monitor's scale factor. Exception:
  `hairline` is always 1 physical px.
- Names use dots and dashes; in Rust they become `SCREAMING_SNAKE_CASE`, e.g. `text.on-accent` →
  `TEXT_ON_ACCENT`.
- Everything here is drawable with tiny-skia: fills, strokes, rounded rects, alpha. No backdrop
  blur; shadows are precomputed (see [Shadows](#shadows)).

## Theme colors

Two themes with identical names. The theme follows the system setting.

| Token | Light | Dark | Use |
|---|---|---|---|
| `bg.canvas` | `#E6E6EB` | `#111114` | Editor area around the image |
| `surface.base` | `#FFFFFF` | `#232328` | Toolbar, panels, preview card, settings window |
| `surface.raised` | `#FFFFFF` | `#2D2D33` | Popovers, tooltips, menus |
| `surface.sunken` | `#F1F1F4` | `#1A1A1E` | Inputs, segmented-control track |
| `fill.hover` | `#1414190D` | `#FFFFFF12` | Hover over any surface (5% / 7%) |
| `fill.pressed` | `#14141917` | `#FFFFFF1F` | Pressed, selected stroke width (9% / 12%) |
| `border.default` | `#1414191A` | `#FFFFFF1A` | Hairlines, dividers, floating-surface edge (10%) |
| `border.strong` | `#1414192E` | `#FFFFFF2E` | Input outlines, swatch rings (18%) |
| `text.primary` | `#18181C` | `#F2F2F5` | Main text |
| `text.secondary` | `#5E5E69` | `#A3A3AE` | Hints, captions |
| `text.disabled` | `#A2A2AC` | `#5F5F69` | Disabled text |
| `text.on-accent` | `#FFFFFF` | `#FFFFFF` | Text and icons on `accent.solid` |
| `icon.primary` | `#2A2A30` | `#E6E6EA` | Tool icons |
| `icon.secondary` | `#6E6E79` | `#9696A1` | Less important icons |
| `accent.solid` | `#4651F6` | `#5560FA` | Active tool, primary button, toggles |
| `accent.hover` | `#3A44E0` | `#5A64FF` | Hover on `accent.solid` |
| `accent.pressed` | `#3038C4` | `#4A55F2` | Pressed `accent.solid` |
| `accent.subtle` | `#4651F61F` | `#7A83FF2E` | Selected rows, active chips (12% / 18%) |
| `accent.text` | `#3A44E0` | `#8F97FF` | Links, accent glyphs on surfaces |
| `focus.ring` | `#4651F6` | `#7A83FF` | Keyboard focus outline |
| `danger` | `#C8281F` | `#FF6259` | Errors, destructive actions |
| `warning` | `#9E5400` | `#FFB547` | Warnings |
| `success` | `#17803C` | `#4CD07D` | Success notices |
| `scrim` | `#0000004D` | `#00000080` | Behind a modal dialog (30% / 50%) |

Contrast, checked against `surface.base`: `text.primary` and `text.secondary` pass 4.5:1 in both
themes (`text.secondary` ≈ 6.3); `text.on-accent` on `accent.solid` is 5.57 light, 4.73 dark.

## Overlay

Theme-independent: the overlay lies on top of the screenshot, whatever its colors.

| Token | Value | Use |
|---|---|---|
| `overlay.dim` | `#00000073` | Outside the selection (45%); drawn as 4 strips around it |
| `overlay.frame` | `#FFFFFF` | Selection frame: 1 px for an area, 2 px for a hovered window |
| `overlay.frame-halo` | `#00000066` | 1 px outside the frame (40%) |
| `overlay.label-bg` | `#16161AE0` | Size label and key-hint pill (88%) |
| `overlay.label-text` | `#FFFFFF` | Label text |
| `overlay.label-text-dim` | `#FFFFFFA6` | Secondary hint text (65%) |
| `overlay.crosshair` | `#FFFFFF` | Crosshair, 1 px |
| `overlay.crosshair-halo` | `#00000066` | 1 px on each side of the crosshair (40%) |

- The frame is drawn outside the captured rect and never ends up in the image.
- The halo keeps the frame visible on white and on dark content alike.
- A hovered window is not dimmed; everything else is.
- The size label shows physical pixels with tabular digits.

## Selection

Theme-independent, drawn over the image in the editor (see [editor.md](editor.md#hover-and-selection)).

| Token | Value | Use |
|---|---|---|
| `selection` | `#4651F6` | Hover outline, selection box, handle ring |
| `selection.handle-fill` | `#FFFFFF` | Handle fill |

## Annotations

Eight colors, three stroke widths. The default is `annotation.red`, width M.

| Token | Color | Counter glyph | Halo |
|---|---|---|---|
| `annotation.red` | `#E5343E` | `#FFFFFF` | dark |
| `annotation.orange` | `#FF8A1F` | `#16161A` | dark |
| `annotation.yellow` | `#FFD426` | `#16161A` | dark |
| `annotation.green` | `#1A9850` | `#FFFFFF` | dark |
| `annotation.blue` | `#2A72F0` | `#FFFFFF` | dark |
| `annotation.purple` | `#934BF0` | `#FFFFFF` | dark |
| `annotation.black` | `#1A1A1E` | `#FFFFFF` | light |
| `annotation.white` | `#FFFFFF` | `#16161A` | dark |

| Token | Value |
|---|---|
| `annotation.halo-dark` | `#00000059` (35%) |
| `annotation.halo-light` | `#FFFFFF99` (60%) |
| `annotation.halo-width` | 1 px on each side |

The halo is the same shape stroked 2 px wider (1 px per side) beneath the annotation: one extra
stroke pass, no blur. It keeps yellow and white visible on a white document and black visible on
a dark IDE.

| Width | Stroke | Counter diameter | Counter glyph | Arrowhead length |
|---|---|---|---|---|
| `annotation.stroke.s` | 2 | 22 | 12 | 16 |
| `annotation.stroke.m` | 4 | 28 | 15 | 22 |
| `annotation.stroke.l` | 8 | 36 | 19 | 34 |

- Widths are in image pixels at 100%: in the saved image, width = token × the scale factor of the
  monitor the screenshot came from. At 200%, M is 8 px in the file and looks like 4 on screen.
- Counter glyphs are Inter Semi Bold with tabular digits.
- Arrowhead: length `3w + 10`, half-width `length × 0.5`. The exact arrow form is in [editor.md](editor.md#objects).

## Typography

Inter, embedded: Regular 400 and Semi Bold 600, subset to Latin + Cyrillic. Tracking 0.

| Token | Size / line height | Weight | Use |
|---|---|---|---|
| `type.caption` | 11 / 14 | 400 | Secondary text, hints |
| `type.label` | 12 / 16 | 600 | Size label, keys, group labels |
| `type.body` | 13 / 18 | 400 | Main interface text |
| `type.body-strong` | 13 / 18 | 600 | Buttons, field labels, key items |
| `type.title` | 15 / 20 | 600 | Section and dialog titles |
| `type.headline` | 20 / 26 | 600 | Settings window title, first-run screen |

Digits in the size label and in counters are tabular, so the label does not jitter while
dragging. ab_glyph does no OpenType shaping: map the `tnum` glyphs by glyph ID or center each
digit in a fixed advance.

## Spacing

`space.2`, `space.4`, `space.6`, `space.8`, `space.12`, `space.16`, `space.20`, `space.24`,
`space.32`: a 4 px step, plus 2 and 6 for small details.

- Inside a button or field: 6–8 horizontally.
- Between toolbar buttons: 4. Between toolbar groups: 12 plus a divider.
- Window padding: 24. Between settings sections: 32.

## Radii

| Token | Value | Use |
|---|---|---|
| `radius.xs` | 4 | Size label, checkbox, key cap |
| `radius.sm` | 6 | Buttons, inputs, tool buttons |
| `radius.md` | 8 | Toolbar, popovers, preview thumbnail |
| `radius.lg` | 12 | Preview card, dialog, settings window |
| `radius.full` | height / 2 | Counters, color swatches, close button on the preview |

## Shadows

Black, x = 0, no spread. Blur is the Figma/CSS blur radius; the Gaussian σ is blur / 2.

| Token | y | Blur | Light | Dark | Use |
|---|---|---|---|---|---|
| `shadow.sm` | 1 | 3 | `#0000001F` (12%) | `#00000059` (35%) | Tooltip, hint |
| `shadow.md` | 4 | 12 | `#00000024` (14%) | `#00000066` (40%) | Toolbar, color popover, menu |
| `shadow.lg` | 10 | 30 | `#00000038` (22%) | `#00000080` (50%) | Preview card, dialog |

- tiny-skia has no blur. Compute each shadow once per size and scale, store it as a nine-slice
  texture, and blit it on redraw.
- Every floating surface gets its shadow plus a 1 px `border.default` hairline. On the dark theme
  the hairline holds the edge; on the light theme the shadow does.

## Sizes

| Token | Value | Use |
|---|---|---|
| `control.height` | 32 | Button, input, tool button |
| `control.height-sm` | 24 | Segments inside a popover, small buttons |
| `toolbar.height` | 44 | 32 buttons + 6 padding top and bottom |
| `icon.size` | 20 | Icon grid; 16 for the small size, stroke stays 1.5 |
| `swatch.size` | 20 | Color swatch; hit area 28 |
| `hairline` | 1 physical px | Dividers and outlines; never blurred at 125% |
| `focus.ring` | 2, offset 2 | Keyboard focus outline, color `focus.ring` |
| `motion.fade` | 120 ms | The only animation: preview and popover fade in/out |

Selected states on the toolbar: the active tool is `accent.solid` with `text.on-accent` icon; the
selected stroke width is `fill.pressed`; the selected color has a 2 px `accent.solid` ring with a
2 px gap (28 px outer diameter). Every swatch has a 1 px `border.strong` outline inside its edge.

## Icons

Tool icons live in [`assets/icons/`](../../assets/icons/) as SVG:

- 20 × 20 grid, 2 px padding, stroke 1.5, round caps and joins, no fills.
- `stroke="currentColor"`: draw with `icon.primary`, or `text.on-accent` on the active tool.
- At 16 px, scale the geometry and keep the stroke at 1.5.

Editor tools: `select`, `arrow`, `line`, `rectangle`, `ellipse`, `counter-number`,
`counter-letter`, `undo`, `redo`, `copy`, `save`, `close`. Preview status row (at 16 px): `check`,
`alert`, `edit`, plus `copy` and `save`.

The app icon and the tray icon are not tool icons: see [icons.md](icons.md).
