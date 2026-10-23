//! Interface icons from `assets/icons/` (`docs/design/tokens.md`, "Icons"): 20 × 20 SVG strokes,
//! 1.5 wide with round caps and joins. The SVG files are the source; this module reads the few
//! shapes they use (`path` with absolute `M L H V C A Z`, `circle`, `ellipse`, `rect`) and strokes
//! them with tiny-skia at any size.

use std::sync::OnceLock;

use tiny_skia::{LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::canvas::skia_color;
use crate::theme::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Alert,
    Arrow,
    Check,
    Close,
    Copy,
    CounterLetter,
    CounterNumber,
    Edit,
    Ellipse,
    Line,
    Rectangle,
    Redo,
    Save,
    Select,
    Undo,
}

const ALL: [Icon; 15] = [
    Icon::Alert,
    Icon::Arrow,
    Icon::Check,
    Icon::Close,
    Icon::Copy,
    Icon::CounterLetter,
    Icon::CounterNumber,
    Icon::Edit,
    Icon::Ellipse,
    Icon::Line,
    Icon::Rectangle,
    Icon::Redo,
    Icon::Save,
    Icon::Select,
    Icon::Undo,
];

/// The icon grid: an icon is drawn in a 20 × 20 box.
const GRID: f32 = 20.0;
const STROKE: f32 = 1.5;

impl Icon {
    fn svg(self) -> &'static str {
        match self {
            Icon::Alert => include_str!("../../../assets/icons/alert.svg"),
            Icon::Arrow => include_str!("../../../assets/icons/arrow.svg"),
            Icon::Check => include_str!("../../../assets/icons/check.svg"),
            Icon::Close => include_str!("../../../assets/icons/close.svg"),
            Icon::Copy => include_str!("../../../assets/icons/copy.svg"),
            Icon::CounterLetter => include_str!("../../../assets/icons/counter-letter.svg"),
            Icon::CounterNumber => include_str!("../../../assets/icons/counter-number.svg"),
            Icon::Edit => include_str!("../../../assets/icons/edit.svg"),
            Icon::Ellipse => include_str!("../../../assets/icons/ellipse.svg"),
            Icon::Line => include_str!("../../../assets/icons/line.svg"),
            Icon::Rectangle => include_str!("../../../assets/icons/rectangle.svg"),
            Icon::Redo => include_str!("../../../assets/icons/redo.svg"),
            Icon::Save => include_str!("../../../assets/icons/save.svg"),
            Icon::Select => include_str!("../../../assets/icons/select.svg"),
            Icon::Undo => include_str!("../../../assets/icons/undo.svg"),
        }
    }

    /// The icon's outline in grid units, parsed once.
    fn path(self) -> &'static Path {
        static PATHS: OnceLock<Vec<Path>> = OnceLock::new();
        let paths = PATHS.get_or_init(|| {
            ALL.iter()
                .map(|icon| parse_svg(icon.svg()).expect("icon SVG parses"))
                .collect()
        });
        &paths[ALL.iter().position(|i| *i == self).unwrap_or(0)]
    }
}

/// Strokes `icon` into `pixmap` in a `size` × `size` box (physical px) at (`x`, `y`). The stroke is
/// 1.5 logical px at `scale`, also at the 16 px size.
pub fn draw(pixmap: &mut Pixmap, icon: Icon, x: f32, y: f32, size: f32, scale: f32, color: Color) {
    let k = size / GRID;
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint.anti_alias = true;
    let stroke = Stroke {
        width: STROKE * scale / k,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let t = Transform::from_row(k, 0.0, 0.0, k, x, y);
    pixmap.stroke_path(icon.path(), &paint, &stroke, t, None);
}

/// Reads the shapes of one of the icon files.
fn parse_svg(svg: &str) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let body = &svg[svg.find("<svg")? + 4..];
    for element in body.split('<').skip(1) {
        let name: String = element
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        let num = |attr: &str| -> Option<f32> { attribute(element, attr)?.parse().ok() };
        match name.as_str() {
            "path" => add_path_data(&mut pb, attribute(element, "d")?)?,
            "circle" => pb.push_circle(num("cx")?, num("cy")?, num("r")?),
            "ellipse" => {
                let (cx, cy, rx, ry) = (num("cx")?, num("cy")?, num("rx")?, num("ry")?);
                pb.push_oval(Rect::from_xywh(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry)?);
            }
            "rect" => {
                let (x, y, w, h) = (num("x")?, num("y")?, num("width")?, num("height")?);
                let r = num("rx").unwrap_or(0.0);
                push_round_rect(&mut pb, x, y, w, h, r);
            }
            _ => {}
        }
    }
    pb.finish()
}

fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = element.find(&key)? + key.len();
    let len = element[start..].find('"')?;
    Some(&element[start..start + len])
}

/// Appends a rounded rectangle, the corners as quarter-circle cubics.
pub(crate) fn push_round_rect(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    let r = r.min(w / 2.0).min(h / 2.0);
    // The cubic control distance of a quarter circle.
    let c = r * 0.552_284_8;
    let (right, bottom) = (x + w, y + h);
    pb.move_to(x + r, y);
    pb.line_to(right - r, y);
    pb.cubic_to(right - r + c, y, right, y + r - c, right, y + r);
    pb.line_to(right, bottom - r);
    pb.cubic_to(
        right,
        bottom - r + c,
        right - r + c,
        bottom,
        right - r,
        bottom,
    );
    pb.line_to(x + r, bottom);
    pb.cubic_to(x + r - c, bottom, x, bottom - r + c, x, bottom - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - c, x + r - c, y, x + r, y);
    pb.close();
}

/// SVG path data with absolute commands.
pub(crate) fn add_path_data(pb: &mut PathBuilder, d: &str) -> Option<()> {
    let mut tokens = Tokens {
        s: d.as_bytes(),
        i: 0,
    };
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut command = b'M';
    while let Some(next) = tokens.peek() {
        if next.is_ascii_alphabetic() {
            command = next;
            tokens.i += 1;
            if command == b'Z' {
                pb.close();
                (cx, cy) = (sx, sy);
                continue;
            }
        }
        match command {
            b'M' => {
                (cx, cy) = (tokens.num()?, tokens.num()?);
                (sx, sy) = (cx, cy);
                pb.move_to(cx, cy);
                // Further pairs after a move are lines.
                command = b'L';
            }
            b'L' => {
                (cx, cy) = (tokens.num()?, tokens.num()?);
                pb.line_to(cx, cy);
            }
            b'H' => {
                cx = tokens.num()?;
                pb.line_to(cx, cy);
            }
            b'V' => {
                cy = tokens.num()?;
                pb.line_to(cx, cy);
            }
            b'C' => {
                let (x1, y1, x2, y2) = (tokens.num()?, tokens.num()?, tokens.num()?, tokens.num()?);
                (cx, cy) = (tokens.num()?, tokens.num()?);
                pb.cubic_to(x1, y1, x2, y2, cx, cy);
            }
            b'A' => {
                let (rx, ry, rotation) = (tokens.num()?, tokens.num()?, tokens.num()?);
                let (large, sweep) = (tokens.num()? != 0.0, tokens.num()? != 0.0);
                let (x, y) = (tokens.num()?, tokens.num()?);
                arc_to(pb, (cx, cy), rx, ry, rotation, large, sweep, (x, y));
                (cx, cy) = (x, y);
            }
            _ => return None,
        }
    }
    Some(())
}

struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl Tokens<'_> {
    /// The next non-separator byte.
    fn peek(&mut self) -> Option<u8> {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b',' | b'\n' | b'\t') {
            self.i += 1;
        }
        self.s.get(self.i).copied()
    }

    fn num(&mut self) -> Option<f32> {
        self.peek()?;
        let start = self.i;
        let mut seen_dot = false;
        while let Some(&c) = self.s.get(self.i) {
            let sign = (c == b'-' || c == b'+') && self.i == start;
            let dot = c == b'.' && !seen_dot;
            if !(c.is_ascii_digit() || sign || dot) {
                break;
            }
            seen_dot |= c == b'.';
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}

/// An SVG elliptical arc as cubics (SVG 1.1, appendix F.6).
#[allow(clippy::too_many_arguments)]
fn arc_to(
    pb: &mut PathBuilder,
    (x1, y1): (f32, f32),
    rx: f32,
    ry: f32,
    rotation: f32,
    large: bool,
    sweep: bool,
    (x2, y2): (f32, f32),
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        pb.line_to(x2, y2);
        return;
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (x1p, y1p) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut k = (num / den).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cxp, cyp) = (k * rx * y1p / ry, -k * ry * x1p / rx);
    let (cx, cy) = (
        cos * cxp - sin * cyp + (x1 + x2) / 2.0,
        sin * cxp + cos * cyp + (y1 + y2) / 2.0,
    );
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vx + uy * vy) / ((ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt());
        let a = a.clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 { -a } else { a }
    };
    let (ux, uy) = ((x1p - cxp) / rx, (y1p - cyp) / ry);
    let theta = angle(1.0, 0.0, ux, uy);
    let mut delta = angle(ux, uy, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let segments = (delta.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / segments as f32;
    let t = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |a: f32| {
        let (s, c) = a.sin_cos();
        (
            cos * rx * c - sin * ry * s + cx,
            sin * rx * c + cos * ry * s + cy,
        )
    };
    let derivative = |a: f32| {
        let (s, c) = a.sin_cos();
        (-cos * rx * s - sin * ry * c, -sin * rx * s + cos * ry * c)
    };
    let mut a = theta;
    for _ in 0..segments {
        let b = a + step;
        let (p0, p3) = (point(a), point(b));
        let (d0, d3) = (derivative(a), derivative(b));
        pb.cubic_to(
            p0.0 + t * d0.0,
            p0.1 + t * d0.1,
            p3.0 - t * d3.0,
            p3.1 - t * d3.1,
            p3.0,
            p3.1,
        );
        a = b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses_inside_its_grid() {
        for icon in ALL {
            let bounds = icon.path().bounds();
            assert!(
                bounds.left() >= 1.0
                    && bounds.top() >= 1.0
                    && bounds.right() <= 19.0
                    && bounds.bottom() <= 19.0,
                "{icon:?}: {bounds:?}"
            );
        }
    }

    #[test]
    fn arc_corner_lands_on_its_end() {
        // The copy icon's corner: from (13, 4.75) around to (11.5, 3.25).
        let mut pb = PathBuilder::new();
        pb.move_to(13.0, 4.75);
        arc_to(
            &mut pb,
            (13.0, 4.75),
            1.5,
            1.5,
            0.0,
            false,
            false,
            (11.5, 3.25),
        );
        let path = pb.finish().unwrap();
        let b = path.bounds();
        assert!(
            (b.left() - 11.5).abs() < 1e-3 && (b.top() - 3.25).abs() < 1e-3,
            "{b:?}"
        );
        assert!(
            (b.right() - 13.0).abs() < 1e-3 && (b.bottom() - 4.75).abs() < 1e-3,
            "{b:?}"
        );
    }

    #[test]
    fn draws_with_the_color() {
        let mut pixmap = Pixmap::new(16, 16).unwrap();
        draw(
            &mut pixmap,
            Icon::Check,
            0.0,
            0.0,
            16.0,
            1.0,
            Color::hex(0xFF00_00FF),
        );
        let inked = pixmap.pixels().iter().filter(|p| p.alpha() > 128).count();
        assert!(inked > 10, "{inked}");
        // Sprites hold BGRA: red lands in tiny-skia's blue slot.
        assert!(
            pixmap
                .pixels()
                .iter()
                .all(|p| p.blue() == p.alpha() && p.red() == 0)
        );
    }
}
