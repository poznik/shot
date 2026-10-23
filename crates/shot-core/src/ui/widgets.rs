//! The controls of `docs/design/settings.md`, "Controls", drawn into a window's pixmap (premultiplied
//! BGRA, see [`crate::canvas`]), and the pieces of text both windows use.

use std::io::Cursor;
use std::sync::OnceLock;

use tiny_skia::{FilterQuality, IntSize, Paint, Pixmap, PixmapPaint, Transform};

use super::form::Look;
use crate::canvas::{self, skia_color};
use crate::draw::{self, Area};
use crate::geom::Rect;
use crate::image::{BgrxMut, BgrxView};
use crate::text::{self, TypeStyle};
use crate::theme::{
    Color, Palette, RADIUS_SM, RADIUS_XS, SHADOW_SM, TYPE_BODY, TYPE_BODY_STRONG, TYPE_LABEL,
    TYPE_TITLE, scaled, stroke,
};

pub(crate) const BUTTON_H: f32 = 32.0;
const BUTTON_PAD: f32 = 12.0;
pub(crate) const TOGGLE_W: f32 = 40.0;
pub(crate) const TOGGLE_H: f32 = 20.0;
const THUMB: f32 = 12.0;
const THUMB_HOVER: f32 = 14.0;
/// From the thumb's center to the end of the track it rests at.
const THUMB_INSET: f32 = 10.0;
pub(crate) const SEGMENTED_H: f32 = 32.0;
const TRACK_PAD: f32 = 2.0;
const SEGMENT_PAD: f32 = 12.0;
pub(crate) const FIELD_W: f32 = 184.0;
pub(crate) const FIELD_H: f32 = 32.0;
const FIELD_PAD: f32 = 6.0;
const RING: f32 = 2.0;
const RING_GAP: f32 = 2.0;
const CAP_GAP: f32 = 4.0;

/// The placeholder of a hotkey field that records and no modifier is held yet.
const RECORDING: &str = "Нажмите сочетание";

/// The app icon at the sizes the windows show it, and above, for larger scales.
const APP_ICONS: [(u32, &[u8]); 4] = [
    (
        40,
        include_bytes!("../../../../assets/app/windows/app-40.png"),
    ),
    (
        48,
        include_bytes!("../../../../assets/app/windows/app-48.png"),
    ),
    (
        64,
        include_bytes!("../../../../assets/app/windows/app-64.png"),
    ),
    (
        256,
        include_bytes!("../../../../assets/app/windows/app-256.png"),
    ),
];

pub(crate) fn area(r: Rect) -> Area {
    Area::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32)
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint.anti_alias = true;
    paint
}

/// Fills whole pixels of `r`.
pub(crate) fn fill(pixmap: &mut Pixmap, r: Rect, color: Color) {
    if let Some(rect) =
        tiny_skia::Rect::from_xywh(r.x as f32, r.y as f32, r.width as f32, r.height as f32)
    {
        let mut paint = paint(color);
        paint.anti_alias = false;
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

/// `text` with its line box's top-left corner at (`x`, `top`). Returns where the text ends.
pub(crate) fn label(
    pixmap: &mut Pixmap,
    style: TypeStyle,
    s: f32,
    text: &str,
    x: i32,
    top: i32,
    color: Color,
) -> f32 {
    text::draw(pixmap, style, s, text, x as f32, top as f32, &paint(color))
}

/// `text` centered in `r`.
pub(crate) fn centered(
    pixmap: &mut Pixmap,
    style: TypeStyle,
    s: f32,
    text: &str,
    r: Rect,
    color: Color,
) {
    let w = text::width(style, s, text);
    let x = (r.x as f32 + (r.width as f32 - w) / 2.0).round();
    let top = r.y + (r.height - scaled(style.line_height, s)) / 2;
    text::draw(pixmap, style, s, text, x, top as f32, &paint(color));
}

/// `text`, cut in the middle with `…` until it fits `room` px: `C:\Users\…\Pictures\Shot`.
pub(crate) fn elide_middle(style: TypeStyle, s: f32, text: &str, room: f32) -> String {
    if text::width(style, s, text) <= room {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let head = keep.div_ceil(2);
        let tail = keep / 2;
        let candidate: String = chars[..head]
            .iter()
            .chain(['…'].iter())
            .chain(&chars[chars.len() - tail..])
            .collect();
        if text::width(style, s, &candidate) <= room {
            return candidate;
        }
    }
    "…".to_string()
}

/// `text` broken at spaces into lines no wider than `room`; a word wider than `room` gets a line
/// of its own.
pub(crate) fn wrap(style: TypeStyle, s: f32, text: &str, room: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if line.is_empty() || text::width(style, s, &candidate) <= room {
            line = candidate;
        } else {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        }
    }
    lines.push(line);
    lines
}

/// A 1 px line across `w` from (`x`, `y`).
pub(crate) fn divider(pixmap: &mut Pixmap, x: i32, y: i32, w: i32, palette: &Palette, s: f32) {
    fill(
        pixmap,
        Rect::new(x, y, w, stroke(1.0, s)),
        palette.border_default,
    );
}

/// The 2 px keyboard-focus outline, 2 px outside `r`, whose corners have `radius`.
fn focus_ring(pixmap: &mut Pixmap, r: Rect, radius: f32, palette: &Palette, s: f32) {
    let d = scaled(RING + RING_GAP, s);
    draw::border_inside(
        pixmap,
        area(r.inflate(d)),
        radius + d as f32,
        stroke(RING, s) as f32,
        palette.focus_ring,
    );
}

pub(crate) fn button_width(text: &str, s: f32) -> i32 {
    text::width(TYPE_BODY_STRONG, s, text).ceil() as i32 + 2 * scaled(BUTTON_PAD, s)
}

pub(crate) fn button(
    pixmap: &mut Pixmap,
    r: Rect,
    text: &str,
    primary: bool,
    look: Look,
    palette: &Palette,
    s: f32,
) {
    let a = area(r);
    let radius = RADIUS_SM * s;
    let line = stroke(1.0, s) as f32;
    let hover = look.hover.is_some();
    let pressed = look.pressed.is_some();
    let color = match (primary, look.enabled) {
        (true, true) => {
            let fill = if pressed {
                palette.accent_pressed
            } else if hover {
                palette.accent_hover
            } else {
                palette.accent_solid
            };
            draw::fill_round_rect(pixmap, a, radius, fill);
            palette.text_on_accent
        }
        (true, false) => {
            draw::fill_round_rect(pixmap, a, radius, palette.fill_pressed);
            palette.text_disabled
        }
        (false, true) => {
            if pressed {
                draw::fill_round_rect(pixmap, a, radius, palette.fill_pressed);
            } else if hover {
                draw::fill_round_rect(pixmap, a, radius, palette.fill_hover);
            }
            draw::border_inside(pixmap, a, radius, line, palette.border_strong);
            palette.text_primary
        }
        (false, false) => {
            draw::border_inside(pixmap, a, radius, line, palette.border_default);
            palette.text_disabled
        }
    };
    centered(pixmap, TYPE_BODY_STRONG, s, text, r, color);
    if look.focus {
        focus_ring(pixmap, r, radius, palette, s);
    }
}

/// A toggle whose track is `r`.
pub(crate) fn toggle(
    pixmap: &mut Pixmap,
    r: Rect,
    on: bool,
    look: Look,
    palette: &Palette,
    s: f32,
) {
    let a = area(r);
    let radius = a.h / 2.0;
    let line = stroke(1.0, s) as f32;
    let hover = look.enabled && look.hover.is_some();
    let thumb = match (on, look.enabled) {
        (true, true) => {
            let fill = if hover {
                palette.accent_hover
            } else {
                palette.accent_solid
            };
            draw::fill_round_rect(pixmap, a, radius, fill);
            palette.text_on_accent
        }
        (false, true) => {
            if hover {
                draw::fill_round_rect(pixmap, a, radius, palette.fill_hover);
            }
            draw::border_inside(pixmap, a, radius, line, palette.icon_secondary);
            palette.icon_secondary
        }
        (true, false) => {
            draw::fill_round_rect(pixmap, a, radius, palette.text_disabled);
            palette.surface_base
        }
        (false, false) => {
            draw::border_inside(pixmap, a, radius, line, palette.text_disabled);
            palette.text_disabled
        }
    };
    let d = if hover { THUMB_HOVER } else { THUMB } * s;
    let inset = THUMB_INSET * s;
    let cx = if on { a.x + a.w - inset } else { a.x + inset };
    draw::fill_circle(pixmap, cx, a.y + a.h / 2.0, d / 2.0, thumb);
    if look.focus {
        focus_ring(pixmap, r, radius, palette, s);
    }
}

fn segment_widths(labels: &[&str], s: f32) -> impl Iterator<Item = i32> {
    labels
        .iter()
        .map(move |t| text::width(TYPE_BODY, s, t).ceil() as i32 + 2 * scaled(SEGMENT_PAD, s))
}

pub(crate) fn segmented_width(labels: &[&str], s: f32) -> i32 {
    segment_widths(labels, s).sum::<i32>() + 2 * scaled(TRACK_PAD, s)
}

/// The segments of a segmented control whose track is `r`.
pub(crate) fn segments(r: Rect, labels: &[&str], s: f32) -> Vec<Rect> {
    let pad = scaled(TRACK_PAD, s);
    let mut x = r.x + pad;
    segment_widths(labels, s)
        .map(|w| {
            let part = Rect::new(x, r.y + pad, w, r.height - 2 * pad);
            x += w;
            part
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn segmented(
    pixmap: &mut Pixmap,
    r: Rect,
    parts: &[Rect],
    labels: &[&str],
    selected: Option<usize>,
    look: Look,
    palette: &Palette,
    s: f32,
) {
    let radius = RADIUS_SM * s;
    let line = stroke(1.0, s) as f32;
    draw::fill_round_rect(pixmap, area(r), radius, palette.surface_sunken);
    draw::border_inside(pixmap, area(r), radius, line, palette.border_default);
    let part_radius = RADIUS_XS * s;
    for (i, (&part, text)) in parts.iter().zip(labels).enumerate() {
        let chosen = selected == Some(i);
        let hover = look.enabled && look.hover == Some(i);
        if chosen {
            if look.enabled {
                shadow_under(pixmap, part, part_radius, palette.dark, s);
            }
            draw::fill_round_rect(pixmap, area(part), part_radius, palette.surface_raised);
            draw::border_inside(
                pixmap,
                area(part),
                part_radius,
                line,
                palette.border_default,
            );
        } else if hover {
            let fill = if look.pressed == Some(i) {
                palette.fill_pressed
            } else {
                palette.fill_hover
            };
            draw::fill_round_rect(pixmap, area(part), part_radius, fill);
        }
        let color = if !look.enabled {
            palette.text_disabled
        } else if chosen || hover {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        centered(pixmap, TYPE_BODY, s, text, part, color);
    }
    if look.focus {
        focus_ring(pixmap, r, radius, palette, s);
    }
}

/// `shadow.sm` under the rounded rect `r`.
fn shadow_under(pixmap: &mut Pixmap, r: Rect, radius: f32, dark: bool, s: f32) {
    if let Some((shadow, x, y)) =
        draw::shadow(r.width as f32, r.height as f32, radius, SHADOW_SM, dark, s)
    {
        pixmap.draw_pixmap(
            r.x - x,
            r.y - y,
            shadow.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
}

/// The look of key caps: the small ones of the hotkey field and the welcome's rows, or the large
/// ones of the welcome's hotkey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Caps {
    Small,
    Large,
}

impl Caps {
    /// Height, padding and type.
    fn metrics(self) -> (f32, f32, TypeStyle) {
        match self {
            Caps::Small => (20.0, 6.0, TYPE_LABEL),
            Caps::Large => (28.0, 8.0, TYPE_TITLE),
        }
    }

    pub fn height(self, s: f32) -> i32 {
        scaled(self.metrics().0, s)
    }

    fn cap_width(self, text: &str, s: f32) -> i32 {
        let (h, pad, style) = self.metrics();
        (text::width(style, s, text).ceil() as i32 + 2 * scaled(pad, s)).max(scaled(h, s))
    }
}

/// Key caps 4 apart from `x`, centered on `cy`. Returns where the last one ends.
pub(crate) fn key_caps(
    pixmap: &mut Pixmap,
    x: i32,
    cy: i32,
    caps: &[String],
    size: Caps,
    palette: &Palette,
    s: f32,
) -> i32 {
    let (_, _, style) = size.metrics();
    let h = size.height(s);
    let fill = if palette.dark {
        palette.surface_raised
    } else {
        palette.surface_base
    };
    let radius = RADIUS_XS * s;
    let mut x = x;
    for (i, cap) in caps.iter().enumerate() {
        if i > 0 {
            x += scaled(CAP_GAP, s);
        }
        let r = Rect::new(x, cy - h / 2, size.cap_width(cap, s), h);
        draw::fill_round_rect(pixmap, area(r), radius, fill);
        draw::border_inside(
            pixmap,
            area(r),
            radius,
            stroke(1.0, s) as f32,
            palette.border_strong,
        );
        centered(pixmap, style, s, cap, r, palette.text_primary);
        x = r.right();
    }
    x
}

/// What a hotkey field shows.
pub(crate) enum Field<'a> {
    /// The hotkey's caps.
    Shows(&'a [String]),
    /// Recording, with the caps of the modifiers held.
    Recording(&'a [String]),
    /// The working hotkey's caps, with the error line below.
    Error(&'a [String]),
}

pub(crate) fn hotkey_field(
    pixmap: &mut Pixmap,
    r: Rect,
    field: Field,
    look: Look,
    palette: &Palette,
    s: f32,
) {
    let a = area(r);
    let radius = RADIUS_SM * s;
    let x = r.x + scaled(FIELD_PAD, s);
    let cy = r.y + r.height / 2;
    match field {
        Field::Recording(held) => {
            draw::fill_round_rect(pixmap, a, radius, palette.surface_base);
            draw::border_inside(
                pixmap,
                a,
                radius,
                stroke(2.0, s) as f32,
                palette.accent_solid,
            );
            if held.is_empty() {
                let top = cy - scaled(TYPE_BODY.line_height, s) / 2;
                label(
                    pixmap,
                    TYPE_BODY,
                    s,
                    RECORDING,
                    x,
                    top,
                    palette.text_secondary,
                );
            } else {
                key_caps(pixmap, x, cy, held, Caps::Small, palette, s);
            }
        }
        Field::Shows(caps) | Field::Error(caps) => {
            draw::fill_round_rect(pixmap, a, radius, palette.surface_sunken);
            if look.hover.is_some() {
                draw::fill_round_rect(pixmap, a, radius, palette.fill_hover);
            }
            let border = match field {
                Field::Error(_) => palette.danger,
                _ => palette.border_strong,
            };
            draw::border_inside(pixmap, a, radius, stroke(1.0, s) as f32, border);
            key_caps(pixmap, x, cy, caps, Caps::Small, palette, s);
        }
    }
    if look.focus {
        focus_ring(pixmap, r, radius, palette, s);
    }
}

/// A `width` × `height` pixmap filled with `surface.base`, reusing `cache` when it has that size.
pub(crate) fn canvas(
    cache: &mut Option<Pixmap>,
    width: u32,
    height: u32,
    palette: &Palette,
) -> Option<Pixmap> {
    let mut pixmap = match cache.take() {
        Some(p) if (p.width(), p.height()) == (width, height) => p,
        _ => Pixmap::new(width, height)?,
    };
    pixmap.fill(skia_color(palette.surface_base));
    Some(pixmap)
}

/// Puts a window's content on its client area `dst`, filling what the content does not cover.
pub(crate) fn present(dst: &mut BgrxMut, pixmap: &Pixmap, palette: &Palette) {
    let view = BgrxView {
        width: pixmap.width(),
        height: pixmap.height(),
        stride: pixmap.width() as usize * 4,
        data: pixmap.data(),
    };
    let all = Rect::new(0, 0, dst.width as i32, dst.height as i32);
    let content = Rect::new(0, 0, view.width as i32, view.height as i32);
    if let Some(r) = content.intersect(&all) {
        canvas::copy(dst, &view, r);
    }
    for rest in all.subtract(&content) {
        canvas::fill(dst, rest, palette.surface_base);
    }
}

/// Draws `icon` with its top-left corner at (`x`, `y`).
pub(crate) fn sprite(pixmap: &mut Pixmap, icon: &Pixmap, x: i32, y: i32) {
    pixmap.draw_pixmap(
        x,
        y,
        icon.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

/// The app icon `size` px wide: the embedded PNG closest above it, scaled down.
pub(crate) fn app_icon(size: u32) -> Option<Pixmap> {
    static DECODED: OnceLock<Vec<Option<Pixmap>>> = OnceLock::new();
    let decoded = DECODED.get_or_init(|| APP_ICONS.iter().map(|(_, png)| decode(png)).collect());
    let i = APP_ICONS
        .iter()
        .position(|(px, _)| *px >= size)
        .unwrap_or(APP_ICONS.len() - 1);
    let source = decoded[i].as_ref()?;
    if source.width() == size {
        return Some(source.clone());
    }
    let mut icon = Pixmap::new(size, size)?;
    let k = size as f32 / source.width() as f32;
    icon.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bicubic,
            ..PixmapPaint::default()
        },
        Transform::from_scale(k, k),
        None,
    );
    Some(icon)
}

/// An RGBA PNG as a premultiplied BGRA pixmap.
fn decode(png: &[u8]) -> Option<Pixmap> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let channels = info.color_type.samples();
    let mut data = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for px in buf[..info.buffer_size()].chunks_exact(channels) {
        let (r, g, b, a) = match *px {
            [r, g, b, a] => (r, g, b, a),
            [r, g, b] => (r, g, b, 255),
            [l, a] => (l, l, l, a),
            [l] => (l, l, l, 255),
            _ => return None,
        };
        let pre = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
        data.extend_from_slice(&[pre(b), pre(g), pre(r), a]);
    }
    Pixmap::from_vec(data, IntSize::from_wh(info.width, info.height)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elides_in_the_middle() {
        let path = r"C:\Users\Someone\Pictures\Shot";
        assert_eq!(elide_middle(TYPE_BODY, 1.0, path, 1000.0), path);
        let short = elide_middle(TYPE_BODY, 1.0, path, 120.0);
        assert!(short.starts_with("C:") && short.ends_with("ot") && short.contains('…'));
        assert!(text::width(TYPE_BODY, 1.0, &short) <= 120.0);
    }

    #[test]
    fn app_icon_decodes_at_every_size() {
        for size in [40, 50, 60, 64, 80, 96] {
            let icon = app_icon(size).unwrap();
            assert_eq!(icon.width(), size);
            // Opaque in the middle, transparent in the corner.
            let mid = icon.pixel(size / 2, size / 2).unwrap();
            assert_eq!(mid.alpha(), 255);
            assert_eq!(icon.pixel(0, 0).unwrap().alpha(), 0, "{size}");
        }
    }
}
