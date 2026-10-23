//! What lies under the annotations (`docs/design/editor.md`, "Image"): the image at the current
//! zoom, and around it the background, the 1 px edge and the shadow. Drawn straight into the
//! window's canvas, a part at a time.

use tiny_skia::Pixmap;

use crate::canvas;
use crate::draw;
use crate::geom::Rect;
use crate::image::{BgrxMut, BgrxView};
use crate::theme::{Palette, SHADOW_SM, stroke};

/// The image prepared for the size it is shown at; where it is shown comes with each draw, so
/// scrolling prepares nothing.
pub struct Backdrop {
    /// The image scaled down to its size on screen, below 100%.
    scaled: Option<Pixmap>,
    edge: Edge,
}

/// The shadow of the image rect. A rect's blurred mask is the product of its blurred sides, so
/// two lines of falloff give it at every pixel.
struct Edge {
    /// Across the image's width and height plus `margin` on each side.
    fx: Vec<u8>,
    fy: Vec<u8>,
    margin: i32,
    /// Downward offset.
    dy: i32,
    alpha: u8,
    line: i32,
}

impl Edge {
    fn new(size: (i32, i32), scale: f32, dark: bool) -> Self {
        let s = SHADOW_SM;
        let r = draw::blur_radius(s.blur / 2.0 * scale);
        let margin = 3 * r as i32 + 1;
        let falloff = |n: i32| {
            let len = (n + 2 * margin) as usize;
            let mut data = vec![0u8; len];
            data[margin as usize..(margin + n) as usize].fill(255);
            let mut line = Vec::new();
            for _ in 0..3 {
                draw::blur_line(&mut data, 0, 1, len, r, &mut line);
            }
            data
        };
        Self {
            fx: falloff(size.0),
            fy: falloff(size.1),
            margin,
            dy: (s.y * scale).round() as i32,
            alpha: if dark { s.dark.a } else { s.light.a },
            line: stroke(1.0, scale),
        }
    }
}

impl Backdrop {
    /// `size`: the image's size on screen.
    pub fn new(image: &Pixmap, size: (i32, i32), scale: f32, dark: bool) -> Self {
        let smaller = size.0 < image.width() as i32 || size.1 < image.height() as i32;
        let scaled = smaller
            .then(|| {
                let view = BgrxView {
                    width: image.width(),
                    height: image.height(),
                    stride: image.width() as usize * 4,
                    data: image.data(),
                };
                draw::thumbnail(&view, size.0 as u32, size.1 as u32)
            })
            .flatten();
        Self {
            scaled,
            edge: Edge::new(size, scale, dark),
        }
    }

    /// Draws the image, shown at `shown`, where it overlaps `region` of the window into `dst`,
    /// which covers `region`.
    pub fn draw_image(&self, dst: &mut BgrxMut, region: Rect, image: &Pixmap, shown: Rect) {
        let Some(clip) = shown.intersect(&region) else {
            return;
        };
        let src = self.scaled.as_ref().unwrap_or(image);
        let (sw, sh) = (src.width() as usize, src.height() as usize);
        let data = src.data();
        let stride = sw * 4;
        let (x0, x1) = (clip.x - region.x, clip.right() - region.x);
        if (sw as i32, sh as i32) == (shown.width, shown.height) {
            let left = (clip.x - shown.x) as usize * 4;
            for y in clip.y..clip.bottom() {
                let start = (y - shown.y) as usize * stride + left;
                dst.span(y - region.y, x0, x1)
                    .copy_from_slice(&data[start..start + clip.width as usize * 4]);
            }
            return;
        }
        // Enlarged: nearest neighbour, so pixels stay sharp.
        let pick = |d: i32, len: i32, n: usize| {
            (((d as f32 + 0.5) * n as f32 / len as f32) as usize).min(n - 1)
        };
        let xs: Vec<usize> = (clip.x..clip.right())
            .map(|x| pick(x - shown.x, shown.width, sw))
            .collect();
        let mut last = None;
        for y in clip.y..clip.bottom() {
            let sy = pick(y - shown.y, shown.height, sh);
            let dy = y - region.y;
            // A row the same as the one above is copied from it.
            if let Some((above, ly)) = last
                && above == sy
            {
                dst.copy_row(ly, dy, x0, x1);
                continue;
            }
            let row = data[sy * stride..(sy + 1) * stride].as_chunks::<4>().0;
            for (d, &sx) in dst
                .span(dy, x0, x1)
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(&xs)
            {
                *d = row[sx];
            }
            last = Some((sy, dy));
        }
    }

    /// Draws the background, the edge and the shadow of the image shown at `shown` in `piece` of
    /// the window, which lies outside the image and inside `region`, into `dst`, which covers
    /// `region`.
    pub fn draw_surround(
        &self,
        dst: &mut BgrxMut,
        region: Rect,
        piece: Rect,
        shown: Rect,
        palette: &Palette,
    ) {
        let local = |r: Rect| r.offset(-region.x, -region.y);
        canvas::fill(dst, local(piece), palette.bg_canvas);

        let e = &self.edge;
        let (left, top) = (shown.x - e.margin, shown.y + e.dy - e.margin);
        let shadow = Rect::new(
            left,
            top,
            shown.width + 2 * e.margin,
            shown.height + 2 * e.margin,
        );
        if let Some(area) = shadow.intersect(&piece) {
            for y in area.y..area.bottom() {
                let fy = e.fy[(y - top) as usize] as u32;
                if fy == 0 {
                    continue;
                }
                let span = dst.span(y - region.y, area.x - region.x, area.right() - region.x);
                for (d, x) in span.as_chunks_mut::<4>().0.iter_mut().zip(area.x..) {
                    let fx = e.fx[(x - left) as usize] as u32;
                    // Black at `alpha × fx × fy`: what stays of the pixel is the rest.
                    let a = (e.alpha as u32 * fx * fy + 255 * 255 / 2) / (255 * 255);
                    let keep = 255 - a;
                    for c in &mut d[..3] {
                        *c = ((*c as u32 * keep + 127) / 255) as u8;
                    }
                }
            }
        }

        let l = e.line;
        let ring = [
            Rect::new(shown.x - l, shown.y - l, shown.width + 2 * l, l),
            Rect::new(shown.x - l, shown.bottom(), shown.width + 2 * l, l),
            Rect::new(shown.x - l, shown.y, l, shown.height),
            Rect::new(shown.right(), shown.y, l, shown.height),
        ];
        for r in ring {
            if let Some(r) = r.intersect(&piece) {
                canvas::fill(dst, local(r), palette.border_default);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::LIGHT;

    fn canvas_of(region: Rect) -> Vec<u8> {
        vec![7u8; region.width as usize * region.height as usize * 4]
    }

    fn px(data: &[u8], region: Rect, x: i32, y: i32) -> [u8; 3] {
        let i = ((y - region.y) * region.width + (x - region.x)) as usize * 4;
        [data[i], data[i + 1], data[i + 2]]
    }

    /// An image whose pixel (x, y) is (x, y, 99).
    fn gradient(w: u32, h: u32) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        for (i, c) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            *c = [x as u8, y as u8, 99, 255];
        }
        p
    }

    #[test]
    fn copies_scales_and_enlarges() {
        let image = gradient(40, 20);
        let region = Rect::new(0, 0, 200, 100);
        for (shown, probe, want) in [
            // 100%: (12, 7) of the image at (22, 17).
            (Rect::new(10, 10, 40, 20), (22, 17), [12, 7, 99]),
            // 200%: window (30, 30) is image (10, 10).
            (Rect::new(10, 10, 80, 40), (30, 30), [10, 10, 99]),
        ] {
            let b = Backdrop::new(&image, (shown.width, shown.height), 1.0, false);
            let mut data = canvas_of(region);
            let mut dst = BgrxMut {
                width: 200,
                height: 100,
                stride: 800,
                data: &mut data,
            };
            b.draw_image(&mut dst, region, &image, shown);
            assert_eq!(px(&data, region, probe.0, probe.1), want);
            // Nothing outside the image.
            assert_eq!(px(&data, region, 5, 5), [7, 7, 7]);
        }
        // 50%: averaged, so the blue stays and red and green halve the coordinates.
        let b = Backdrop::new(&image, (20, 10), 1.0, false);
        let mut data = canvas_of(region);
        let mut dst = BgrxMut {
            width: 200,
            height: 100,
            stride: 800,
            data: &mut data,
        };
        b.draw_image(&mut dst, region, &image, Rect::new(0, 0, 20, 10));
        let [r, g, blue] = px(&data, region, 5, 5);
        assert_eq!(blue, 99);
        assert!((10..=11).contains(&r) && (10..=11).contains(&g), "{r} {g}");
    }

    #[test]
    fn surround_has_edge_and_shadow_below() {
        let image = gradient(40, 20);
        let shown = Rect::new(20, 20, 40, 20);
        let b = Backdrop::new(&image, (shown.width, shown.height), 1.0, false);
        let region = Rect::new(0, 0, 100, 80);
        let mut data = canvas_of(region);
        let mut dst = BgrxMut {
            width: 100,
            height: 80,
            stride: 400,
            data: &mut data,
        };
        for piece in region.subtract(&shown) {
            b.draw_surround(&mut dst, region, piece, shown, &LIGHT);
        }
        let bg = [LIGHT.bg_canvas.b, LIGHT.bg_canvas.g, LIGHT.bg_canvas.r];
        assert_eq!(px(&data, region, 2, 2), bg);
        // The edge, just outside the image, darker than the shadow beyond it.
        let edge = px(&data, region, 40, 40)[0];
        let shade = px(&data, region, 40, 42)[0];
        assert!(edge < shade && shade < bg[0], "{edge} {shade} {}", bg[0]);
        // The shadow falls down: more below than above.
        assert!(px(&data, region, 40, 42)[0] < px(&data, region, 40, 17)[0]);
        // The image itself is left alone.
        assert_eq!(px(&data, region, 30, 30), [7, 7, 7]);
    }
}
