//! Drawing for the small windows Shot shows besides the overlay — the preview card, and later the
//! editor and the settings: rounded shapes, drop shadows and image thumbnails on tiny-skia sprites
//! (premultiplied BGRA, see [`crate::canvas`]).

use tiny_skia::{FillRule, Mask, Paint, Path, PathBuilder, PixmapPaint, Transform};
pub use tiny_skia::{IntSize, Pixmap};

use crate::canvas::skia_color;
use crate::icons::push_round_rect;
use crate::image::BgrxView;
use crate::theme::{Color, Shadow};

/// A rectangle in physical px with fractional edges, as tiny-skia draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// The area moved `d` inward on every side.
    pub fn inset(self, d: f32) -> Self {
        Self::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

pub fn round_rect(a: Area, radius: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    push_round_rect(&mut pb, a.x, a.y, a.w, a.h, radius);
    pb.finish()
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint.anti_alias = true;
    paint
}

pub fn fill_round_rect(pixmap: &mut Pixmap, a: Area, radius: f32, color: Color) {
    if let Some(path) = round_rect(a, radius) {
        pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// A border of `width` drawn inside the edge of the rounded rect `a`, the way design tools draw
/// an inside stroke: the band between the outline and the outline inset by `width`.
pub fn border_inside(pixmap: &mut Pixmap, a: Area, radius: f32, width: f32, color: Color) {
    let mut pb = PathBuilder::new();
    push_round_rect(&mut pb, a.x, a.y, a.w, a.h, radius);
    let inner = a.inset(width);
    push_round_rect(
        &mut pb,
        inner.x,
        inner.y,
        inner.w,
        inner.h,
        (radius - width).max(0.0),
    );
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::EvenOdd,
            Transform::identity(),
            None,
        );
    }
}

pub fn fill_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, color: Color) {
    if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
        pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Draws `image` with its top-left corner at (`x`, `y`), only inside the rounded rect `clip`.
pub fn draw_clipped(pixmap: &mut Pixmap, image: &Pixmap, x: i32, y: i32, clip: Area, radius: f32) {
    let Some(mut mask) = Mask::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    if let Some(path) = round_rect(clip, radius) {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    }
    pixmap.draw_pixmap(
        x,
        y,
        image.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        Some(&mask),
    );
}

/// The drop shadow of a `w` × `h` rounded rect in its own sprite, and where the rect's top-left
/// corner lies in it. `shadow`'s lengths are logical px, scaled by `scale`.
pub fn shadow(
    w: f32,
    h: f32,
    radius: f32,
    s: Shadow,
    dark: bool,
    scale: f32,
) -> Option<(Pixmap, i32, i32)> {
    let r = blur_radius(s.blur / 2.0 * scale);
    let margin = 3 * r as i32 + 1;
    let dy = (s.y * scale).round() as i32;
    let width = w.ceil() as i32 + 2 * margin;
    let height = h.ceil() as i32 + 2 * margin + dy.abs();
    let (left, top) = (margin, margin + (-dy).max(0));

    // A blur this wide has no detail to keep: it runs at half the size, on a quarter of the
    // pixels, and is scaled back up. A card's shadow at 200 % takes 2 ms instead of 7 on a Mac's
    // efficiency core, where the first work after idle runs (MAC-005).
    let k = if r >= 8 { 2 } else { 1 };
    let kf = k as f32;
    let (sw, sh) = ((width as usize).div_ceil(k), (height as usize).div_ceil(k));
    let mut mask = Mask::new(sw as u32, sh as u32)?;
    let rect = Area::new(left as f32 / kf, (top + dy) as f32 / kf, w / kf, h / kf);
    let path = round_rect(rect, radius / kf)?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    let mut alpha = mask.data().to_vec();
    let rk = blur_radius(s.blur / 2.0 * scale / kf);
    let (mut line, mut sums) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        for y in 0..sh {
            blur_line(&mut alpha, y * sw, 1, sw, rk, &mut line);
        }
        blur_columns(&mut alpha, sw, rk, &mut line, &mut sums);
    }
    let (wu, hu) = (width as usize, height as usize);
    if k == 2 {
        alpha = upscale2(&alpha, sw, sh, wu, hu);
    }

    let color = if dark { s.dark } else { s.light };
    let mut pixmap = Pixmap::new(width as u32, height as u32)?;
    for (p, a) in pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(&alpha)
    {
        // Premultiplied: the color is black, so only alpha carries anything.
        let a = (*a as u32 * color.a as u32 + 127) / 255;
        p[0] = (color.b as u32 * a / 255) as u8;
        p[1] = (color.g as u32 * a / 255) as u8;
        p[2] = (color.r as u32 * a / 255) as u8;
        p[3] = a as u8;
    }
    Some((pixmap, left, top))
}

/// The radius of the three box blurs that stand in for a Gaussian of `sigma`: boxes of width
/// `2r + 1` have its variance together.
pub(crate) fn blur_radius(sigma: f32) -> usize {
    (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0)
        .round()
        .max(0.0) as usize
}

/// One box blur of radius `r` along a line of `len` values `step` apart, starting at `start`.
pub(crate) fn blur_line(
    data: &mut [u8],
    start: usize,
    step: usize,
    len: usize,
    r: usize,
    line: &mut Vec<u8>,
) {
    line.clear();
    line.extend((0..len).map(|i| data[start + i * step]));
    let window = 2 * r + 1;
    let mut sum: usize = line[..r.min(len)].iter().map(|&v| v as usize).sum();
    for i in 0..len {
        if i + r < len {
            sum += line[i + r] as usize;
        }
        data[start + i * step] = ((sum + window / 2) / window) as u8;
        if i >= r {
            sum -= line[i - r] as usize;
        }
    }
}

/// `src`, `sw` × `sh` values, scaled up twice to `w` × `h` (at most `2sw` × `2sh`) by linear
/// interpolation: on each axis an output value weighs its source value 3 to 1 against the
/// neighbour on its side.
fn upscale2(src: &[u8], sw: usize, sh: usize, w: usize, h: usize) -> Vec<u8> {
    let other = |i: usize, odd: bool, n: usize| {
        if odd {
            (i + 1).min(n - 1)
        } else {
            i.saturating_sub(1)
        }
    };
    // Across, times 4.
    let mut wide = vec![0u16; w * sh];
    for y in 0..sh {
        let s = &src[y * sw..][..sw];
        for (x, v) in wide[y * w..][..w].iter_mut().enumerate() {
            let i = x / 2;
            *v = 3 * s[i] as u16 + s[other(i, x % 2 == 1, sw)] as u16;
        }
    }
    // Down, times 16, rounded.
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        let i = y / 2;
        let (a, b) = (
            &wide[i * w..][..w],
            &wide[other(i, y % 2 == 1, sh) * w..][..w],
        );
        for ((o, &p), &q) in out[y * w..][..w].iter_mut().zip(a).zip(b) {
            *o = ((3 * p as u32 + q as u32 + 8) / 16) as u8;
        }
    }
    out
}

/// [`blur_line`] down every column of an image `w` values wide, done row by row: a column's values
/// lie a row apart in memory, and reading them one column at a time misses the cache on each
/// on a large image. `src` and `sums` are scratch space.
pub(crate) fn blur_columns(
    data: &mut [u8],
    w: usize,
    r: usize,
    src: &mut Vec<u8>,
    sums: &mut Vec<u32>,
) {
    let h = data.len() / w;
    src.clear();
    src.extend_from_slice(data);
    sums.clear();
    sums.resize(w, 0);
    let row = |y: usize| &src[y * w..][..w];
    let add = |sums: &mut [u32], y: usize| {
        for (s, &v) in sums.iter_mut().zip(row(y)) {
            *s += v as u32;
        }
    };
    for y in 0..r.min(h) {
        add(sums, y);
    }
    let window = (2 * r + 1) as u32;
    for y in 0..h {
        if y + r < h {
            add(sums, y + r);
        }
        for (out, &sum) in data[y * w..][..w].iter_mut().zip(sums.iter()) {
            *out = ((sum + window / 2) / window) as u8;
        }
        if y >= r {
            for (s, &v) in sums.iter_mut().zip(row(y - r)) {
                *s -= v as u32;
            }
        }
    }
}

/// `image` scaled down to `w` × `h` as an opaque sprite. Each pixel averages up to 4 × 4 samples
/// spread over the source pixels it covers: the cost depends on the thumbnail, not on the capture.
/// A large one, as the editor's image below 100%, is split between threads.
pub fn thumbnail(image: &BgrxView, w: u32, h: u32) -> Option<Pixmap> {
    let mut out = Pixmap::new(w, h)?;
    let (kx, ky) = (
        image.width as f32 / w as f32,
        image.height as f32 / h as f32,
    );
    let samples = |k: f32| (k.ceil() as usize).clamp(1, 4);
    let (nx, ny) = (samples(kx), samples(ky));
    // Which source pixels each output column samples.
    let xs: Vec<usize> = (0..w as usize)
        .flat_map(|tx| {
            (0..nx).map(move |i| {
                let x = ((tx as f32 + (i as f32 + 0.5) / nx as f32) * kx) as usize;
                x.min(image.width as usize - 1)
            })
        })
        .collect();
    let rows_of = |ty: usize| {
        let mut rows: [&[[u8; 4]]; 4] = [&[]; 4];
        for (j, row) in rows.iter_mut().enumerate().take(ny) {
            let y = ((ty as f32 + (j as f32 + 0.5) / ny as f32) * ky) as u32;
            *row = image.row(y.min(image.height - 1)).as_chunks().0;
        }
        rows
    };
    let row_bytes = w as usize * 4;
    let fill = |first: usize, part: &mut [u8]| {
        for (i, out_row) in part.chunks_exact_mut(row_bytes).enumerate() {
            let rows = rows_of(first + i);
            average_row(out_row, &rows[..ny], &xs, nx);
        }
    };
    let threads = if w as usize * h as usize >= 1 << 20 {
        std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
    } else {
        1
    };
    let per = (h as usize).div_ceil(threads);
    let data = out.data_mut();
    if threads == 1 {
        fill(0, data);
    } else {
        std::thread::scope(|scope| {
            let mut parts = data.chunks_mut(per * row_bytes).enumerate();
            let first = parts.next();
            for (i, part) in parts {
                let fill = &fill;
                scope.spawn(move || fill(i * per, part));
            }
            if let Some((_, part)) = first {
                fill(0, part);
            }
        });
    }
    Some(out)
}

/// One output row: each pixel averages its `nx` columns of each of `rows`.
fn average_row(out: &mut [u8], rows: &[&[[u8; 4]]], xs: &[usize], nx: usize) {
    let n = (nx * rows.len()) as u32;
    // `(sum + n / 2) / n` as a multiply: exact for every `n` of 1–4 × 1–4 samples.
    let inv = (1 << 16) / n + 1;
    for (px, cols) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(xs.chunks_exact(nx))
    {
        let mut sum = [0u32; 3];
        for &x in cols {
            for row in rows {
                let p = row[x];
                for c in 0..3 {
                    sum[c] += p[c] as u32;
                }
            }
        }
        for c in 0..3 {
            px[c] = (((sum[c] + n / 2) * inv) >> 16) as u8;
        }
        px[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::SHADOW_LG;

    #[test]
    fn shadow_is_soft_and_centered_below() {
        let (pixmap, left, top) = shadow(200.0, 150.0, 12.0, SHADOW_LG, false, 1.0).unwrap();
        let alpha = |x: i32, y: i32| pixmap.pixel(x as u32, y as u32).unwrap().alpha();
        // Under the middle of the rect: the full shadow color.
        assert_eq!(alpha(left + 100, top + 75), 0x38);
        // Fades out toward the sprite's edge, and reaches further below than above.
        assert_eq!(alpha(0, 0), 0);
        assert!(alpha(left + 100, top + 150 + 10) > alpha(left + 100, top - 10));
        assert!(alpha(left + 100, top + 150 + 10) < 0x38);
    }

    #[test]
    fn shadow_at_half_size_matches_the_full_one() {
        // The full-size blur, as the shadow was made before.
        let (w, h, radius, scale) = (248.0, 212.0, 12.0, 2.0);
        let (pixmap, left, top) = shadow(
            w * scale,
            h * scale,
            radius * scale,
            SHADOW_LG,
            false,
            scale,
        )
        .unwrap();
        let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
        let r = blur_radius(SHADOW_LG.blur / 2.0 * scale);
        let dy = (SHADOW_LG.y * scale).round() as i32;
        let mut mask = Mask::new(width as u32, height as u32).unwrap();
        let rect = Area::new(left as f32, (top + dy) as f32, w * scale, h * scale);
        let path = round_rect(rect, radius * scale).unwrap();
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        let mut full = mask.data().to_vec();
        let mut line = Vec::new();
        for _ in 0..3 {
            for y in 0..height {
                blur_line(&mut full, y * width, 1, width, r, &mut line);
            }
            blur_columns(&mut full, width, r, &mut line, &mut Vec::new());
        }
        let worst = (pixmap.pixels().iter().zip(&full))
            .map(|(p, &f)| {
                let f = (f as u32 * SHADOW_LG.light.a as u32 + 127) / 255;
                (p.alpha() as i32 - f as i32).abs()
            })
            .max()
            .unwrap();
        assert!(worst <= 1, "off by {worst}");
    }

    #[test]
    fn columns_blur_as_lines_do() {
        let (w, h, r) = (37, 23, 4);
        let mut by_line: Vec<u8> = (0..w * h).map(|i| (i * 97 % 251) as u8).collect();
        let mut by_row = by_line.clone();
        let mut line = Vec::new();
        for x in 0..w {
            blur_line(&mut by_line, x, w, h, r, &mut line);
        }
        blur_columns(&mut by_row, w, r, &mut line, &mut Vec::new());
        assert_eq!(by_line, by_row);
    }

    #[test]
    fn thumbnail_averages() {
        // 4 × 2 pixels: left half black, right half white; scaled to 2 × 1.
        let mut data = [0u8; 32];
        for x in 2..4 {
            for y in 0..2 {
                data[(y * 4 + x) * 4..][..3].fill(255);
            }
        }
        let view = BgrxView {
            width: 4,
            height: 2,
            stride: 16,
            data: &data,
        };
        let t = thumbnail(&view, 2, 1).unwrap();
        assert_eq!(&t.data()[..4], [0, 0, 0, 255]);
        assert_eq!(&t.data()[4..], [255, 255, 255, 255]);
    }

    #[test]
    fn inside_border_stays_inside() {
        let mut pixmap = Pixmap::new(20, 20).unwrap();
        border_inside(
            &mut pixmap,
            Area::new(0.0, 0.0, 20.0, 20.0),
            0.0,
            1.0,
            Color::hex(0xFF),
        );
        assert_eq!(pixmap.pixel(10, 0).unwrap().alpha(), 255);
        assert_eq!(pixmap.pixel(10, 1).unwrap().alpha(), 0);
        assert_eq!(pixmap.pixel(10, 10).unwrap().alpha(), 0);
    }
}
