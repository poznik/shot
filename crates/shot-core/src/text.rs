//! One-line text in the embedded Inter (`assets/fonts`, built by `tools/fonts.py`).
//!
//! ab_glyph only reads outlines and metrics; tiny-skia fills them. There is no shaping and no
//! kerning: the subset carries no layout tables. Digits are tabular. The modifier signs of macOS,
//! which Inter lacks, are drawn here as strokes (`docs/design/settings.md`, "Hotkey caps").

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, GlyphId, OutlineCurve, Point as FontPoint};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::icons::add_path_data;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    SemiBold,
}

/// A type token: font size and line height in logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    pub weight: Weight,
    pub size: f32,
    pub line_height: f32,
}

fn font(weight: Weight) -> &'static FontRef<'static> {
    static FONTS: OnceLock<[FontRef<'static>; 2]> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let parse = |bytes| FontRef::try_from_slice(bytes).expect("embedded font parses");
        [
            parse(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
            parse(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
        ]
    });
    match weight {
        Weight::Regular => &fonts[0],
        Weight::SemiBold => &fonts[1],
    }
}

/// A sign Inter has no glyph for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sign {
    Control,
    Option,
    Shift,
    Command,
}

const SIGNS: [Sign; 4] = [Sign::Control, Sign::Option, Sign::Shift, Sign::Command];

/// Inter's cap height in font units: the signs stand on the baseline and reach it.
const CAP_HEIGHT: f32 = 1490.0;
/// Space on each side of a sign, in font units.
const SIGN_BEARING: f32 = 100.0;

impl Sign {
    fn of(c: char) -> Option<Sign> {
        match c {
            '\u{2303}' => Some(Sign::Control),
            '\u{2325}' => Some(Sign::Option),
            '\u{21E7}' => Some(Sign::Shift),
            '\u{2318}' => Some(Sign::Command),
            _ => None,
        }
    }

    /// The center line as SVG path data in font units, y down from the cap height, and the width
    /// it takes with the stroke.
    fn shape(self) -> (&'static str, f32) {
        match self {
            Sign::Control => ("M 140 700 L 560 120 L 980 700", 1120.0),
            Sign::Option => (
                "M 140 120 L 510 120 L 880 1370 L 1280 1370 M 910 120 L 1280 120",
                1420.0,
            ),
            Sign::Shift => (
                "M 710 120 L 1280 690 L 1000 690 L 1000 1370 L 420 1370 L 420 690 L 140 690 Z",
                1420.0,
            ),
            // A square with a loop at each corner: lines through the corners, three quarters of
            // a circle around each.
            Sign::Command => (
                "M 584 524 L 584 302 A 222 222 0 1 0 362 524 L 1249 524 A 222 222 0 1 0 1027 302 \
                 L 1027 1189 A 222 222 0 1 0 1249 967 L 362 967 A 222 222 0 1 0 584 1189 Z",
                1611.0,
            ),
        }
    }

    fn advance(self) -> f32 {
        self.shape().1 + 2.0 * SIGN_BEARING
    }

    /// The filled outline in the units of `shape`, stroked as thick as the stems of `weight`;
    /// the outlines of `⇧` and `⌘` thinner, or they fill up.
    fn outline(self, weight: Weight) -> &'static Path {
        static OUTLINES: OnceLock<[Vec<Path>; 2]> = OnceLock::new();
        let outlines = OUTLINES.get_or_init(|| {
            [Weight::Regular, Weight::SemiBold].map(|weight| {
                let font = font(weight);
                let stem = font
                    .outline(font.glyph_id('I'))
                    .map_or(200.0, |o| o.bounds.max.x - o.bounds.min.x);
                SIGNS
                    .iter()
                    .map(|sign| {
                        let mut pb = PathBuilder::new();
                        add_path_data(&mut pb, sign.shape().0).expect("sign path parses");
                        let stroke = Stroke {
                            width: match sign {
                                Sign::Shift | Sign::Command => stem * 0.7,
                                Sign::Control | Sign::Option => stem,
                            },
                            line_cap: LineCap::Round,
                            line_join: LineJoin::Round,
                            ..Stroke::default()
                        };
                        let center = pb.finish().expect("sign path is not empty");
                        center.stroke(&stroke, 1.0).expect("sign strokes")
                    })
                    .collect()
            })
        });
        let i = SIGNS.iter().position(|s| *s == self).unwrap_or(0);
        &outlines[weight as usize][i]
    }
}

/// What a character is drawn with: Inter's glyph, or one of the signs where Inter has none.
enum Glyph {
    Font(GlyphId),
    Sign(Sign),
}

fn glyph(font: &FontRef, c: char) -> Glyph {
    let id = font.glyph_id(c);
    match Sign::of(c) {
        Some(sign) if id.0 == 0 => Glyph::Sign(sign),
        _ => Glyph::Font(id),
    }
}

/// Advance width of `text` in font units.
fn advance_units(font: &FontRef, text: &str) -> f32 {
    text.chars()
        .map(|c| match glyph(font, c) {
            Glyph::Font(id) => font.h_advance_unscaled(id),
            Glyph::Sign(sign) => sign.advance(),
        })
        .sum()
}

/// Physical px per font unit.
fn units(font: &FontRef, style: TypeStyle, scale: f32) -> f32 {
    style.size * scale / font.units_per_em().unwrap_or(2048.0)
}

/// Advance width of `text` in physical px.
pub fn width(style: TypeStyle, scale: f32, text: &str) -> f32 {
    let font = font(style.weight);
    advance_units(font, text) * units(font, style, scale)
}

/// Fills `text` into `pixmap` with its line box's top-left corner at (`x`, `top`), the baseline
/// snapped to a whole pixel. Returns the pen position after the last glyph.
pub fn draw(
    pixmap: &mut Pixmap,
    style: TypeStyle,
    scale: f32,
    text: &str,
    x: f32,
    top: f32,
    paint: &Paint,
) -> f32 {
    let font = font(style.weight);
    let k = units(font, style, scale);
    let ascent = font.ascent_unscaled() * k;
    let descent = font.descent_unscaled() * k;
    let half_leading = (style.line_height * scale - (ascent - descent)) / 2.0;
    let baseline = (top + half_leading + ascent).round();

    let mut path = PathBuilder::new();
    let pen = push_text(&mut path, style.weight, k, text, x, baseline);
    if let Some(path) = path.finish() {
        pixmap.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
    }
    pen
}

/// Appends the outlines of `text`, `k` px per font unit, its baseline starting at (`x`,
/// `baseline`). Returns the pen position after the last glyph.
fn push_text(
    path: &mut PathBuilder,
    weight: Weight,
    k: f32,
    text: &str,
    x: f32,
    baseline: f32,
) -> f32 {
    let font = font(weight);
    let mut pen = x;
    for c in text.chars() {
        match glyph(font, c) {
            Glyph::Font(id) => {
                if let Some(outline) = font.outline(id) {
                    let to_px = |p: FontPoint| (pen + p.x * k, baseline - p.y * k);
                    add_outline(path, &outline.curves, to_px);
                }
                pen += font.h_advance_unscaled(id) * k;
            }
            Glyph::Sign(sign) => {
                let t = Transform::from_row(
                    k,
                    0.0,
                    0.0,
                    k,
                    pen + SIGN_BEARING * k,
                    baseline - CAP_HEIGHT * k,
                );
                if let Some(outline) = sign.outline(weight).clone().transform(t) {
                    path.push_path(&outline);
                }
                pen += sign.advance() * k;
            }
        }
    }
    pen
}

/// Advance width of `text` at `size` physical px.
pub fn advance(weight: Weight, size: f32, text: &str) -> f32 {
    let font = font(weight);
    advance_units(font, text) * size / font.units_per_em().unwrap_or(2048.0)
}

/// Height of capitals and digits above the baseline at `size` px: what centers a label
/// vertically.
pub fn cap_height(weight: Weight, size: f32) -> f32 {
    let font = font(weight);
    let k = size / font.units_per_em().unwrap_or(2048.0);
    // ab_glyph puts the top of the outline in `min.y`: its bounds are y-down, its curves y-up.
    font.outline(font.glyph_id('H'))
        .map_or(size * 0.73, |o| o.bounds.min.y.max(o.bounds.max.y) * k)
}

/// The outline of `text` at `size` px, its baseline starting at (`x`, `baseline`) and not snapped
/// to pixels: for drawing through a transform, as annotations are.
pub fn outline(weight: Weight, size: f32, text: &str, x: f32, baseline: f32) -> Option<Path> {
    let font = font(weight);
    let k = size / font.units_per_em().unwrap_or(2048.0);
    let mut path = PathBuilder::new();
    push_text(&mut path, weight, k, text, x, baseline);
    path.finish()
}

/// Appends glyph contours. ab_glyph gives a flat list of curves; a curve that does not start
/// where the previous one ended begins a new contour.
fn add_outline(
    path: &mut PathBuilder,
    curves: &[OutlineCurve],
    to_px: impl Fn(FontPoint) -> (f32, f32),
) {
    let mut last: Option<FontPoint> = None;
    for curve in curves {
        let (OutlineCurve::Line(start, _)
        | OutlineCurve::Quad(start, ..)
        | OutlineCurve::Cubic(start, ..)) = *curve;
        if last != Some(start) {
            if last.is_some() {
                path.close();
            }
            let (x, y) = to_px(start);
            path.move_to(x, y);
        }
        last = Some(match *curve {
            OutlineCurve::Line(_, end) => {
                let (x, y) = to_px(end);
                path.line_to(x, y);
                end
            }
            OutlineCurve::Quad(_, c, end) => {
                let ((cx, cy), (x, y)) = (to_px(c), to_px(end));
                path.quad_to(cx, cy, x, y);
                end
            }
            OutlineCurve::Cubic(_, c1, c2, end) => {
                let ((ax, ay), (bx, by), (x, y)) = (to_px(c1), to_px(c2), to_px(end));
                path.cubic_to(ax, ay, bx, by, x, y);
                end
            }
        });
    }
    if last.is_some() {
        path.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::{Key, Os, modifier_caps};
    use crate::theme::TYPE_LABEL;

    #[test]
    fn every_character_of_a_hotkey_is_drawn() {
        for os in [Os::Windows, Os::MacOs] {
            let mut text = modifier_caps([true; 4], os).concat();
            for key in Key::all() {
                text.push_str(&key.name());
            }
            for weight in [Weight::Regular, Weight::SemiBold] {
                let f = font(weight);
                for c in text.chars() {
                    assert!(
                        f.glyph_id(c).0 != 0 || Sign::of(c).is_some(),
                        "{c:?} is missing"
                    );
                }
            }
        }
    }

    #[test]
    fn signs_stay_in_their_box_and_ink() {
        for weight in [Weight::Regular, Weight::SemiBold] {
            for sign in SIGNS {
                let b = sign.outline(weight).bounds();
                let width = sign.shape().1;
                assert!(
                    b.left() >= 0.0
                        && b.right() <= width
                        && b.top() >= -40.0
                        && b.bottom() <= CAP_HEIGHT + 40.0,
                    "{sign:?} {weight:?}: {b:?}"
                );
            }
        }
        for c in ['\u{2303}', '\u{2325}', '\u{21E7}', '\u{2318}'] {
            let text = c.to_string();
            let mut pixmap = Pixmap::new(40, 40).unwrap();
            let mut paint = Paint::default();
            paint.set_color_rgba8(0, 0, 0, 255);
            let pen = draw(&mut pixmap, TYPE_LABEL, 2.0, &text, 0.0, 0.0, &paint);
            assert_eq!(pen, width(TYPE_LABEL, 2.0, &text));
            let inked = pixmap.pixels().iter().filter(|p| p.alpha() > 128).count();
            assert!(inked > 20, "{c:?}: {inked}");
        }
    }

    #[test]
    fn capitals_are_about_three_quarters_of_the_size() {
        let cap = cap_height(Weight::SemiBold, 100.0);
        assert!((70.0..76.0).contains(&cap), "{cap}");
    }

    #[test]
    fn digits_are_tabular() {
        let w = width(TYPE_LABEL, 1.0, "0");
        for d in "123456789".chars() {
            assert_eq!(width(TYPE_LABEL, 1.0, &d.to_string()), w);
        }
        assert_eq!(width(TYPE_LABEL, 2.0, "1920"), 8.0 * w);
    }

    #[test]
    fn latin_cyrillic_and_signs_have_glyphs() {
        for weight in [Weight::Regular, Weight::SemiBold] {
            let f = font(weight);
            for c in "Space Esc Клик Протянуть область весь экран отмена 0123456789 × ·".chars()
            {
                assert_ne!(f.glyph_id(c).0, 0, "{c:?} is missing");
            }
        }
    }
}
