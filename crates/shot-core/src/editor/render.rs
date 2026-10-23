//! Drawing annotations (`docs/design/editor.md`, "Objects", "Hover and selection") into a sprite
//! through a transform from image pixels: the editor draws them zoomed over the image, the result
//! at 1 : 1 into it.

use tiny_skia::{
    FillRule, IntRect, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

use super::objects::{HANDLE_FILL, Object, Pt, SELECTION, Shape, arrow_outline};
use crate::canvas::skia_color;
use crate::text::{self, Weight};
use crate::theme::{Color, stroke};

/// Handle diameter and ring, logical px: handles keep their size at any zoom.
const HANDLE: f32 = 10.0;
const HANDLE_RING: f32 = 2.0;

/// One thing an object paints.
enum Part {
    Stroke {
        path: Path,
        width: f32,
        round_cap: bool,
    },
    Fill(Path),
}

/// Draws `o` through `t`, which maps image pixels to the sprite's and only scales and moves.
/// `hover`: the halo becomes a 1 px `selection` outline.
pub fn draw_object(pixmap: &mut Pixmap, o: &Object, k: f32, t: Transform, hover: bool) {
    let swatch = o.swatch();
    let (body, halo) = parts(o, k);
    if hover {
        for part in &halo {
            draw_part(pixmap, part, SELECTION, t);
        }
    } else {
        for part in &halo {
            draw_part(pixmap, part, swatch.halo, t);
        }
    }
    for part in &body {
        draw_part(pixmap, part, swatch.color, t);
    }
    if let Some((label, _, _)) = o.counter_body(k)
        && let Shape::Counter { at, .. } = o.shape
    {
        let size = o.sizes(k).glyph;
        let x = at.x - text::advance(Weight::SemiBold, size, &label) / 2.0;
        let baseline = at.y + text::cap_height(Weight::SemiBold, size) / 2.0;
        if let Some(path) = text::outline(Weight::SemiBold, size, &label, x, baseline) {
            draw_part(pixmap, &Part::Fill(path), swatch.glyph, t);
        }
    }
}

/// What `o` paints and its halo: the same geometry stroked 2 halo widths wider, filled parts
/// still filled.
fn parts(o: &Object, k: f32) -> (Vec<Part>, Vec<Part>) {
    let s = o.sizes(k);
    let (w, h) = (s.stroke, s.halo);
    let stroke = |path: &Path, width: f32, round_cap: bool| Part::Stroke {
        path: path.clone(),
        width,
        round_cap,
    };
    match o.shape {
        Shape::Line { a, b } => {
            let Some(path) = segment(a, b) else {
                return (Vec::new(), Vec::new());
            };
            (
                vec![stroke(&path, w, true)],
                vec![stroke(&path, w + 2.0 * h, true)],
            )
        }
        Shape::Arrow { a, b } => {
            let Some(outline) = arrow_outline(a, b, w, s.head) else {
                return (Vec::new(), Vec::new());
            };
            // One path each, filled once: the halo does not get darker where parts would overlap.
            let grown = |r: f32| {
                let mut pb = PathBuilder::new();
                push_grown(&mut pb, &outline, r);
                pb.finish().map(Part::Fill)
            };
            (
                grown(w / 4.0).into_iter().collect(),
                grown(w / 4.0 + h).into_iter().collect(),
            )
        }
        Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
            let Some(rect) =
                Rect::from_ltrb(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
            else {
                return (Vec::new(), Vec::new());
            };
            let path = if matches!(o.shape, Shape::Rect { .. }) {
                PathBuilder::from_rect(rect)
            } else {
                match PathBuilder::from_oval(rect) {
                    Some(path) => path,
                    None => return (Vec::new(), Vec::new()),
                }
            };
            (
                vec![stroke(&path, w, false)],
                vec![stroke(&path, w + 2.0 * h, false)],
            )
        }
        Shape::Counter { at, .. } => {
            let Some((_, width, height)) = o.counter_body(k) else {
                return (Vec::new(), Vec::new());
            };
            let pill = |grow: f32| {
                let (w, h) = (width + 2.0 * grow, height + 2.0 * grow);
                let mut pb = PathBuilder::new();
                crate::icons::push_round_rect(
                    &mut pb,
                    at.x - w / 2.0,
                    at.y - h / 2.0,
                    w,
                    h,
                    h / 2.0,
                );
                pb.finish().map(Part::Fill)
            };
            (
                pill(0.0).into_iter().collect(),
                pill(h).into_iter().collect(),
            )
        }
    }
}

fn segment(a: Pt, b: Pt) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    // A zero-length segment still paints its round caps: a dot.
    let b = if a == b { Pt::new(b.x + 0.01, b.y) } else { b };
    pb.line_to(b.x, b.y);
    pb.finish()
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint.anti_alias = true;
    paint
}

fn draw_part(pixmap: &mut Pixmap, part: &Part, color: Color, t: Transform) {
    match part {
        Part::Fill(path) => pixmap.fill_path(path, &paint(color), FillRule::Winding, t, None),
        Part::Stroke {
            path,
            width,
            round_cap,
        } => {
            let stroke = Stroke {
                width: *width,
                line_cap: if *round_cap {
                    LineCap::Round
                } else {
                    LineCap::Butt
                },
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(path, &paint(color), &stroke, t, None);
        }
    }
}

/// The simple polygon `points` grown by `r`: round at its convex corners, sharp at its concave
/// ones, where the grown edges meet. Wound so the shoelace area is positive.
fn push_grown(pb: &mut PathBuilder, points: &[Pt], r: f32) {
    let n = points.len();
    let area: f32 = (0..n)
        .map(|i| {
            let (p, q) = (points[i], points[(i + 1) % n]);
            p.x * q.y - q.x * p.y
        })
        .sum();
    let mut p = points.to_vec();
    if area < 0.0 {
        p.reverse();
    }
    // Each edge's outward normal.
    let mut normals = Vec::with_capacity(n);
    for i in 0..n {
        let e = p[(i + 1) % n] - p[i];
        let len = e.len();
        if len < 1e-4 {
            return;
        }
        normals.push(Pt::new(e.y / len, -e.x / len));
    }
    // Each corner from where the previous grown edge ends; the close runs along the last edge.
    for i in 0..n {
        let from = normals[(i + n - 1) % n];
        let to = normals[i];
        let a0 = from.y.atan2(from.x);
        let sweep = (to.y.atan2(to.x) - a0).rem_euclid(std::f32::consts::TAU);
        let concave = sweep > std::f32::consts::PI;
        let start = if concave {
            p[i] + (from + to) * (r / (1.0 + from.x * to.x + from.y * to.y))
        } else {
            p[i] + from * r
        };
        if i == 0 {
            pb.move_to(start.x, start.y);
        } else {
            pb.line_to(start.x, start.y);
        }
        if !concave {
            push_arc(pb, p[i], r, a0, sweep);
        }
    }
    pb.close();
}

/// An arc around `c` of radius `r` from angle `a0` through `sweep`, as cubics of at most 90°;
/// the pen is at its start.
fn push_arc(pb: &mut PathBuilder, c: Pt, r: f32, a0: f32, sweep: f32) {
    if sweep < 1e-4 {
        return;
    }
    let parts = (sweep / std::f32::consts::FRAC_PI_2).ceil().max(1.0);
    let step = sweep / parts;
    let h = 4.0 / 3.0 * (step / 4.0).tan() * r;
    let mut a = a0;
    for _ in 0..parts as usize {
        let b = a + step;
        let (sa, ca) = a.sin_cos();
        let (sb, cb) = b.sin_cos();
        let p0 = c + Pt::new(ca, sa) * r;
        let p3 = c + Pt::new(cb, sb) * r;
        let p1 = p0 + Pt::new(-sa, ca) * h;
        let p2 = p3 - Pt::new(-sb, cb) * h;
        pb.cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y);
        a = b;
    }
}

fn map(t: Transform, p: Pt) -> (f32, f32) {
    (p.x * t.sx + t.tx, p.y * t.sy + t.ty)
}

/// Handles at points of the sprite: white circles with a `selection` ring inside their edge.
pub fn draw_handles(pixmap: &mut Pixmap, points: &[(f32, f32)], scale: f32) {
    let r = HANDLE * scale / 2.0;
    let ring = stroke(HANDLE_RING, scale) as f32;
    for &(x, y) in points {
        if let Some(path) = PathBuilder::from_circle(x, y, r) {
            pixmap.fill_path(
                &path,
                &paint(SELECTION),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        if let Some(path) = PathBuilder::from_circle(x, y, r - ring) {
            pixmap.fill_path(
                &path,
                &paint(HANDLE_FILL),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

/// The handle hit area's half side, in px of a monitor at `scale`.
pub fn handle_reach(scale: f32) -> f32 {
    HANDLE * scale
}

/// `image` with `objects` drawn into it at 1 : 1: what is copied and saved.
pub fn flatten(image: &Pixmap, objects: &[Object], k: f32) -> Pixmap {
    let mut out = image.clone();
    for o in objects {
        draw_object(&mut out, o, k, Transform::identity(), false);
    }
    out
}

/// The part of the sprite `t` maps the bounds of `o` to, rounded out to whole pixels.
pub fn object_rect(o: &Object, k: f32, t: Transform) -> Option<IntRect> {
    let b = o.bounds(k);
    let (x0, y0) = map(t, b.min);
    let (x1, y1) = map(t, b.max);
    IntRect::from_ltrb(
        x0.floor() as i32 - 1,
        y0.floor() as i32 - 1,
        x1.ceil() as i32 + 1,
        y1.ceil() as i32 + 1,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::editor::objects::{Counting, SWATCHES, Width};
    use crate::image::{BgrxView, Compression, encode_png};

    /// A scene of every kind of object in each color, over a light and a dark half, so the halos
    /// show.
    fn scene(width: Width, k: f32, hover: bool) -> Pixmap {
        let (w, h) = ((480.0 * k) as u32, (200.0 * k) as u32);
        let mut pixmap = Pixmap::new(w, h).unwrap();
        pixmap.fill(skia_color(Color::hex(0xF4F4_F4FF)));
        let dark = Rect::from_xywh(0.0, h as f32 / 2.0, w as f32, h as f32 / 2.0).unwrap();
        pixmap.fill_rect(
            dark,
            &paint(Color::hex(0x2020_24FF)),
            Transform::identity(),
            None,
        );
        let p = |x: f32, y: f32| Pt::new(x * k, y * k);
        let mut objects = Vec::new();
        for (i, row) in [20.0, 120.0].into_iter().enumerate() {
            let shapes = [
                Shape::Line {
                    a: p(16.0, row),
                    b: p(80.0, row + 56.0),
                },
                Shape::Arrow {
                    a: p(96.0, row + 56.0),
                    b: p(170.0, row + 4.0),
                },
                Shape::Rect {
                    a: p(186.0, row),
                    b: p(246.0, row + 56.0),
                },
                Shape::Ellipse {
                    a: p(262.0, row),
                    b: p(330.0, row + 56.0),
                },
                Shape::Counter {
                    at: p(362.0, row + 28.0),
                    counting: Counting::Numbers,
                    n: 7,
                },
                Shape::Counter {
                    at: p(430.0, row + 28.0),
                    counting: Counting::Letters,
                    n: 26 * 27 + 1,
                },
            ];
            for (j, shape) in shapes.into_iter().enumerate() {
                objects.push(Object {
                    shape,
                    color: (i * 6 + j) % SWATCHES.len(),
                    width,
                });
            }
        }
        for o in &objects {
            draw_object(&mut pixmap, o, k, Transform::identity(), hover);
        }
        pixmap
    }

    fn reference_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("reference")
    }

    /// Compares `pixmap` with the reference PNG `name`, or writes it with `SHOT_BLESS=1`. Small
    /// differences pass: tiny-skia's vector code differs a little between CPUs.
    fn check(name: &str, pixmap: &Pixmap) {
        let path = reference_dir().join(name);
        let view = BgrxView {
            width: pixmap.width(),
            height: pixmap.height(),
            stride: pixmap.width() as usize * 4,
            data: pixmap.data(),
        };
        if std::env::var_os("SHOT_BLESS").is_some() {
            std::fs::create_dir_all(reference_dir()).unwrap();
            let file = std::fs::File::create(&path).unwrap();
            encode_png(
                view,
                std::io::BufWriter::new(file),
                Compression::Balanced,
                None,
            )
            .unwrap();
            return;
        }
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run with SHOT_BLESS=1", path.display()));
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut rgb = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut rgb).unwrap();
        let info = reader.info();
        assert_eq!(
            (info.width, info.height),
            (pixmap.width(), pixmap.height()),
            "{name}: size"
        );
        let mut off = 0;
        let mut worst = 0;
        for (want, got) in rgb
            .as_chunks::<3>()
            .0
            .iter()
            .zip(pixmap.data().as_chunks::<4>().0)
        {
            let got = [got[2], got[1], got[0]];
            let d = (0..3).map(|c| want[c].abs_diff(got[c])).max().unwrap_or(0);
            worst = worst.max(d);
            if d > 2 {
                off += 1;
            }
        }
        let limit = (pixmap.width() * pixmap.height()) as usize / 500;
        assert!(
            off <= limit && worst <= 48,
            "{name}: {off} pixels differ (limit {limit}), worst by {worst}; \
             look at the picture and rerun with SHOT_BLESS=1 if it is right"
        );
    }

    #[test]
    fn objects_match_the_references() {
        check("objects-s.png", &scene(Width::S, 1.0, false));
        check("objects-m.png", &scene(Width::M, 1.0, false));
        check("objects-l.png", &scene(Width::L, 1.0, false));
        check("objects-m-hover.png", &scene(Width::M, 1.0, true));
        check("objects-m-200.png", &scene(Width::M, 2.0, false));
    }

    #[test]
    fn handles_match_the_reference() {
        let mut pixmap = Pixmap::new(120, 60).unwrap();
        pixmap.fill(skia_color(Color::hex(0xF4F4_F4FF)));
        draw_handles(
            &mut pixmap,
            &[(20.0, 15.0), (60.0, 15.0), (100.0, 45.0)],
            1.0,
        );
        check("handles.png", &pixmap);
    }

    #[test]
    fn the_arrow_halo_is_one_layer() {
        // A white arrow on white: nowhere, the corners included, is darker than one layer of the
        // dark halo, `#000` at 35%.
        let mut pixmap = Pixmap::new(120, 40).unwrap();
        pixmap.fill(skia_color(Color::hex(0xFFFF_FFFF)));
        let arrow = Object {
            shape: Shape::Arrow {
                a: Pt::new(4.0, 20.0),
                b: Pt::new(110.0, 20.0),
            },
            color: 7,
            width: Width::M,
        };
        draw_object(&mut pixmap, &arrow, 1.0, Transform::identity(), false);
        let darkest = pixmap.pixels().iter().map(|c| c.red()).min().unwrap();
        assert!((164..=168).contains(&darkest), "{darkest}");
        // Inside the head the halo is under the white body.
        assert_eq!(pixmap.pixel(100, 20).unwrap().red(), 255);
    }

    #[test]
    fn flatten_draws_at_one_to_one() {
        let mut image = Pixmap::new(40, 40).unwrap();
        image.fill(skia_color(Color::hex(0xFFFF_FFFF)));
        let line = Object {
            shape: Shape::Line {
                a: Pt::new(0.0, 20.0),
                b: Pt::new(40.0, 20.0),
            },
            color: 0,
            width: Width::L,
        };
        let out = flatten(&image, &[line], 1.0);
        let c = out.pixel(20, 20).unwrap();
        // Red `#E5343E`, stored as BGRA.
        assert_eq!((c.red(), c.green(), c.blue()), (0x3E, 0x34, 0xE5));
        assert_eq!(image.pixel(20, 20).unwrap().red(), 255);
    }
}
