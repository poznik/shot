//! Pixel operations on BGRX canvases.
//!
//! Large flat areas (the dim, frames, crosshair arms) are written here directly: they are
//! axis-aligned whole pixels, and a plain loop over a 4K canvas is several times faster than
//! tiny-skia's pipeline. Anti-aliased shapes and text are drawn by tiny-skia into small sprites,
//! which are then composited here. Sprites hold premultiplied BGRA: tiny-skia's red channel
//! carries blue (see [`skia_color`]).

use tiny_skia::Pixmap;

use crate::geom::{Point, Rect};
use crate::image::{BgrxMut, BgrxView};
use crate::theme::Color;

/// `x / 255`, rounded to nearest, for `x <= 255 * 255`.
#[inline(always)]
fn div255(x: u16) -> u8 {
    let x = x + 128;
    ((x + (x >> 8)) >> 8) as u8
}

/// The color premultiplied in BGRX order, and how much of what lies under it stays.
fn premultiplied(c: Color) -> ([u8; 4], u16) {
    let a = c.a as u16;
    let pre = |v: u8| div255(v as u16 * a);
    ([pre(c.b), pre(c.g), pre(c.r), 0], 255 - a)
}

/// tiny-skia color for a sprite, which stores BGRA in tiny-skia's RGBA slots.
pub(crate) fn skia_color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.b, c.g, c.r, c.a)
}

fn bounds(width: u32, height: u32) -> Rect {
    Rect::new(0, 0, width as i32, height as i32)
}

fn src_span<'a>(src: &BgrxView<'a>, y: i32, x0: i32, x1: i32) -> &'a [u8] {
    &src.row(y as u32)[x0 as usize * 4..x1 as usize * 4]
}

/// Copies `rect` of `src` to the same place in `dst`, which has the same size.
pub(crate) fn copy(dst: &mut BgrxMut, src: &BgrxView, rect: Rect) {
    let Some(rect) = rect.intersect(&bounds(src.width, src.height)) else {
        return;
    };
    for y in rect.y..rect.bottom() {
        let s = src_span(src, y, rect.x, rect.right());
        dst.span(y, rect.x, rect.right()).copy_from_slice(s);
    }
}

/// Copies `rect` of `src` to the same place in `dst` with `over` blended on top, in one pass.
pub(crate) fn copy_under(dst: &mut BgrxMut, src: &BgrxView, rect: Rect, over: Color) {
    let Some(rect) = rect.intersect(&bounds(src.width, src.height)) else {
        return;
    };
    let (add, keep) = premultiplied(over);
    for y in rect.y..rect.bottom() {
        let s = src_span(src, y, rect.x, rect.right());
        let d = dst.span(y, rect.x, rect.right());
        // Whole 16-byte blocks in SSE2 on x86-64: the compiler's own vectors of this loop are
        // three times slower there (CORE-005). On aarch64 it vectorizes well by itself.
        #[cfg(target_arch = "x86_64")]
        let (d, s) = x86::scale_add(d, s, add, keep);
        for (d, s) in d
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(s.as_chunks::<4>().0)
        {
            for i in 0..4 {
                d[i] = add[i] + div255(s[i] as u16 * keep);
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use std::arch::x86_64::{
        __m128i, _mm_add_epi8, _mm_add_epi16, _mm_loadu_si128, _mm_mullo_epi16, _mm_packus_epi16,
        _mm_set1_epi16, _mm_set1_epi32, _mm_setzero_si128, _mm_srli_epi16, _mm_storeu_si128,
        _mm_unpackhi_epi8, _mm_unpacklo_epi8,
    };

    /// `d = add + div255(s * keep)` byte by byte, `add` repeating every pixel, over the whole
    /// 16-byte blocks of `s`. Returns the rest of both spans.
    pub(super) fn scale_add<'d, 's>(
        d: &'d mut [u8],
        s: &'s [u8],
        add: [u8; 4],
        keep: u16,
    ) -> (&'d mut [u8], &'s [u8]) {
        let (d_blocks, d_rest) = d.as_chunks_mut::<16>();
        let (s_blocks, s_rest) = s.as_chunks::<16>();
        // SSE2 is part of x86-64: every CPU that runs this has it.
        unsafe {
            let add = _mm_set1_epi32(i32::from_le_bytes(add));
            let keep = _mm_set1_epi16(keep as i16);
            let half = _mm_set1_epi16(128);
            let zero = _mm_setzero_si128();
            // div255 of [`super::div255`] in eight 16-bit lanes.
            let scale = |x: __m128i| {
                let t = _mm_add_epi16(_mm_mullo_epi16(x, keep), half);
                _mm_srli_epi16::<8>(_mm_add_epi16(t, _mm_srli_epi16::<8>(t)))
            };
            for (d, s) in d_blocks.iter_mut().zip(s_blocks) {
                let p = _mm_loadu_si128(s.as_ptr().cast());
                let lo = scale(_mm_unpacklo_epi8(p, zero));
                let hi = scale(_mm_unpackhi_epi8(p, zero));
                let out = _mm_add_epi8(_mm_packus_epi16(lo, hi), add);
                _mm_storeu_si128(d.as_mut_ptr().cast(), out);
            }
        }
        (d_rest, s_rest)
    }
}

/// Fills `rect` with `color`, blending when it is translucent.
pub(crate) fn fill(dst: &mut BgrxMut, rect: Rect, color: Color) {
    let Some(rect) = rect.intersect(&bounds(dst.width, dst.height)) else {
        return;
    };
    let (add, keep) = premultiplied(color);
    if keep == 0 {
        // Opaque, as the editor's background around the image: a plain store.
        for y in rect.y..rect.bottom() {
            dst.span(y, rect.x, rect.right())
                .as_chunks_mut::<4>()
                .0
                .fill(add);
        }
        return;
    }
    for y in rect.y..rect.bottom() {
        for d in dst.span(y, rect.x, rect.right()).as_chunks_mut::<4>().0 {
            for i in 0..4 {
                d[i] = add[i] + div255(d[i] as u16 * keep);
            }
        }
    }
}

/// Composites a premultiplied BGRA sprite with its top-left corner at `at`, only inside `clip`.
pub(crate) fn draw_sprite(dst: &mut BgrxMut, sprite: &Pixmap, at: Point, clip: Rect) {
    draw_sprite_faded(dst, sprite, at, clip, 255);
}

/// [`draw_sprite`] at `opacity` of 255: for a fading popover.
pub(crate) fn draw_sprite_faded(
    dst: &mut BgrxMut,
    sprite: &Pixmap,
    at: Point,
    clip: Rect,
    opacity: u8,
) {
    let area = Rect::new(at.x, at.y, sprite.width() as i32, sprite.height() as i32);
    let Some(rect) = area
        .intersect(&clip)
        .and_then(|r| r.intersect(&bounds(dst.width, dst.height)))
    else {
        return;
    };
    let width = sprite.width() as usize;
    for y in rect.y..rect.bottom() {
        let start = ((y - at.y) as usize * width + (rect.x - at.x) as usize) * 4;
        let s = &sprite.data()[start..start + rect.width as usize * 4];
        let d = dst.span(y, rect.x, rect.right());
        for (d, s) in d
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(s.as_chunks::<4>().0)
        {
            if opacity == 255 {
                let keep = 255 - s[3] as u16;
                for i in 0..3 {
                    d[i] = s[i] + div255(d[i] as u16 * keep);
                }
            } else {
                let o = opacity as u16;
                let keep = 255 - div255(s[3] as u16 * o) as u16;
                for i in 0..3 {
                    d[i] = div255(s[i] as u16 * o) + div255(d[i] as u16 * keep);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div255_rounds_like_floats() {
        for a in 0..=255u16 {
            for b in 0..=255u16 {
                let exact = (a as f32 * b as f32 / 255.0).round() as u8;
                assert_eq!(div255(a * b), exact, "{a} * {b}");
            }
        }
    }

    #[test]
    fn dim_matches_the_scalar_rule_at_every_length() {
        // Long enough for whole blocks and every remainder; values cover the whole byte range.
        let src: Vec<u8> = (0..4 * 41).map(|i| (i * 37 % 256) as u8).collect();
        for over in [
            Color::hex(0x0000_0073),
            Color::hex(0x3366_99C0),
            Color::hex(0xFFFF_FF00),
        ] {
            let (add, keep) = premultiplied(over);
            for width in 0..=41 {
                let view = BgrxView {
                    width,
                    height: 1,
                    stride: src.len(),
                    data: &src,
                };
                let mut out = vec![7u8; src.len()];
                let mut dst = BgrxMut {
                    width,
                    height: 1,
                    stride: src.len(),
                    data: &mut out,
                };
                copy_under(&mut dst, &view, Rect::new(0, 0, width as i32, 1), over);
                for (i, (o, s)) in out.iter().zip(&src).enumerate() {
                    let want = if i < width as usize * 4 {
                        add[i % 4] + div255(*s as u16 * keep)
                    } else {
                        7
                    };
                    assert_eq!(*o, want, "width {width}, byte {i}");
                }
            }
        }
    }

    #[test]
    fn dim_and_blend() {
        let src = [200, 100, 0, 0, 255, 255, 255, 0];
        let view = BgrxView {
            width: 2,
            height: 1,
            stride: 8,
            data: &src,
        };
        let mut out = [0u8; 8];
        let mut dst = BgrxMut {
            width: 2,
            height: 1,
            stride: 8,
            data: &mut out,
        };
        // 45% black keeps 140/255 of each channel.
        copy_under(
            &mut dst,
            &view,
            Rect::new(0, 0, 2, 1),
            Color::hex(0x0000_0073),
        );
        assert_eq!(&out[..3], [110, 55, 0]);
        assert_eq!(&out[4..7], [140, 140, 140]);

        let mut dst = BgrxMut {
            width: 2,
            height: 1,
            stride: 8,
            data: &mut out,
        };
        fill(&mut dst, Rect::new(1, 0, 5, 5), Color::hex(0xFF00_0080));
        // Red at 50% over gray 140: blue and green halve, red gets 128 + 70.
        assert_eq!(&out[4..7], [70, 70, 198]);
        assert_eq!(&out[..3], [110, 55, 0]);
    }
}
