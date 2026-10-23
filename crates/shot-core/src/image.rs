//! Captured pixels and their encoding.

use std::io::{self, BufRead, Seek, Write};

pub use png::Compression;

use crate::geom::Rect;

/// Borrowed 32-bit BGRX image with rows from top to bottom: the layout both GDI and CoreGraphics
/// capture into. The fourth byte of a pixel is undefined (GDI leaves it 0) and is never read as
/// alpha.
#[derive(Clone, Copy, Debug)]
pub struct BgrxView<'a> {
    pub width: u32,
    pub height: u32,
    /// Bytes from the start of one row to the next, at least `width * 4`.
    pub stride: usize,
    pub data: &'a [u8],
}

impl<'a> BgrxView<'a> {
    pub fn row(&self, y: u32) -> &'a [u8] {
        let start = y as usize * self.stride;
        &self.data[start..start + self.width as usize * 4]
    }

    /// The part of the image inside `rect`, which must lie within it: a view into the same
    /// pixels, without a copy.
    pub fn crop(&self, rect: Rect) -> BgrxView<'a> {
        assert!(
            rect.x >= 0
                && rect.y >= 0
                && !rect.is_empty()
                && rect.right() <= self.width as i32
                && rect.bottom() <= self.height as i32,
            "crop {rect:?} outside {}x{}",
            self.width,
            self.height
        );
        BgrxView {
            width: rect.width as u32,
            height: rect.height as u32,
            stride: self.stride,
            data: &self.data[rect.y as usize * self.stride + rect.x as usize * 4..],
        }
    }
}

/// Mutable 32-bit BGRX image with rows from top to bottom: a canvas the platform puts on screen.
#[derive(Debug)]
pub struct BgrxMut<'a> {
    pub width: u32,
    pub height: u32,
    /// Bytes from the start of one row to the next, at least `width * 4`.
    pub stride: usize,
    pub data: &'a mut [u8],
}

impl BgrxMut<'_> {
    pub fn view(&self) -> BgrxView<'_> {
        BgrxView {
            width: self.width,
            height: self.height,
            stride: self.stride,
            data: self.data,
        }
    }

    /// Pixels `x0..x1` of row `y`, which the caller has clipped to the image.
    pub(crate) fn span(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u8] {
        let start = y as usize * self.stride;
        &mut self.data[start + x0 as usize * 4..start + x1 as usize * 4]
    }

    /// Copies pixels `x0..x1` of row `from` to row `to`.
    pub(crate) fn copy_row(&mut self, from: i32, to: i32, x0: i32, x1: i32) {
        let (a, b) = (x0 as usize * 4, x1 as usize * 4);
        let src = from as usize * self.stride;
        self.data
            .copy_within(src + a..src + b, to as usize * self.stride + a);
    }
}

/// Inches per metre: PNG stores density per metre.
const METRES_PER_INCH: f32 = 0.0254;

/// Encodes the image as an 8-bit RGB PNG, converting one row at a time so no second full-size
/// buffer is needed. `dpi` goes into `pHYs`: the monitor's scale times the platform's base density
/// (96 on Windows, 72 on macOS), so documents show the capture at the size it had on screen and
/// the editor knows the scale. The pixels are marked sRGB: what both platforms capture in.
pub fn encode_png<W: Write>(
    img: BgrxView,
    out: W,
    compression: Compression,
    dpi: Option<f32>,
) -> io::Result<()> {
    write_png(img.width, img.height, out, compression, dpi, |y, rgb| {
        for (dst, src) in rgb
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .zip(img.row(y).as_chunks::<4>().0)
        {
            *dst = [src[2], src[1], src[0]];
        }
    })
}

/// [`encode_png`] for pixels already in RGB order, `width * 3` bytes a row with no padding.
pub fn encode_rgb_png<W: Write>(
    width: u32,
    height: u32,
    rgb: &[u8],
    out: W,
    compression: Compression,
    dpi: Option<f32>,
) -> io::Result<()> {
    let row = width as usize * 3;
    write_png(width, height, out, compression, dpi, |y, dst| {
        let start = y as usize * row;
        dst.copy_from_slice(&rgb[start..start + row]);
    })
}

/// Writes a PNG whose RGB row `y` `fill` puts into the buffer it is given.
fn write_png<W: Write>(
    width: u32,
    height: u32,
    out: W,
    compression: Compression,
    dpi: Option<f32>,
    mut fill: impl FnMut(u32, &mut [u8]),
) -> io::Result<()> {
    let mut encoder = png::Encoder::new(out, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(compression);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    if let Some(dpi) = dpi {
        let ppm = (dpi / METRES_PER_INCH).round() as u32;
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: ppm,
            yppu: ppm,
            unit: png::Unit::Meter,
        }));
    }
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    let mut stream = writer.stream_writer().map_err(io::Error::other)?;
    let mut rgb = vec![0u8; width as usize * 3];
    for y in 0..height {
        fill(y, &mut rgb);
        stream.write_all(&rgb)?;
    }
    stream.finish().map_err(io::Error::other)
}

/// Where the pixels of an [`encode_tiff`] file start: right after the header, as RGB rows with no
/// padding.
pub const TIFF_PIXELS_AT: usize = 8;

/// Encodes the image as an uncompressed 8-bit RGB TIFF with `dpi` and an optional ICC profile:
/// a copy with the bytes reordered, the quickest image file there is to make. The macOS clipboard
/// takes it.
pub fn encode_tiff(img: BgrxView, dpi: f32, icc: Option<&[u8]>) -> Vec<u8> {
    let mut out = vec![0; tiff_len(img.width, img.height, icc.map_or(0, <[u8]>::len))];
    write_tiff(img, dpi, icc, &mut out);
    out
}

/// The size of the [`encode_tiff`] file of a `width`×`height` image with an ICC profile of
/// `icc_len` bytes.
pub fn tiff_len(width: u32, height: u32, icc_len: usize) -> usize {
    tiff_layout(width, height, icc_len).end
}

/// [`encode_tiff`] into `out`, which is [`tiff_len`] bytes long: memory the caller chose.
pub fn write_tiff(img: BgrxView, dpi: f32, icc: Option<&[u8]>, out: &mut [u8]) {
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    const RATIONAL: u16 = 5;
    const UNDEFINED: u16 = 7;

    let (w, h) = (img.width, img.height);
    let icc = icc.unwrap_or(&[]);
    let at = tiff_layout(w, h, icc.len());
    assert_eq!(
        out.len(),
        at.end,
        "TIFF of {w}x{h} into {} bytes",
        out.len()
    );
    let mut pos = 0;
    let mut put = |bytes: &[u8]| {
        out[pos..pos + bytes.len()].copy_from_slice(bytes);
        pos += bytes.len();
    };
    put(b"II*\0");
    put(&(at.ifd as u32).to_le_bytes());

    let mut entries = Vec::with_capacity(at.entries as usize * 12);
    let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
        entries.extend_from_slice(&tag.to_le_bytes());
        entries.extend_from_slice(&kind.to_le_bytes());
        entries.extend_from_slice(&count.to_le_bytes());
        entries.extend_from_slice(&value.to_le_bytes());
    };
    entry(256, LONG, 1, w); // ImageWidth
    entry(257, LONG, 1, h); // ImageLength
    entry(258, SHORT, 3, at.bits as u32); // BitsPerSample
    entry(259, SHORT, 1, 1); // Compression: none
    entry(262, SHORT, 1, 2); // PhotometricInterpretation: RGB
    entry(273, LONG, 1, TIFF_PIXELS_AT as u32); // StripOffsets
    entry(277, SHORT, 1, 3); // SamplesPerPixel
    entry(278, LONG, 1, h); // RowsPerStrip
    entry(279, LONG, 1, at.pixels as u32); // StripByteCounts
    entry(282, RATIONAL, 1, at.resolution as u32); // XResolution
    entry(283, RATIONAL, 1, at.resolution as u32); // YResolution
    entry(284, SHORT, 1, 1); // PlanarConfiguration: interleaved
    entry(296, SHORT, 1, 2); // ResolutionUnit: inch
    if !icc.is_empty() {
        entry(34675, UNDEFINED, icc.len() as u32, at.profile as u32); // ICCProfile
    }
    let mut tail = Vec::with_capacity(at.end - at.ifd);
    tail.extend_from_slice(&at.entries.to_le_bytes());
    tail.extend_from_slice(&entries);
    tail.extend_from_slice(&0u32.to_le_bytes()); // no next directory
    for v in [8u16, 8, 8, 0] {
        tail.extend_from_slice(&v.to_le_bytes());
    }
    tail.extend_from_slice(&((dpi * 100.0).round() as u32).to_le_bytes());
    tail.extend_from_slice(&100u32.to_le_bytes());
    tail.extend_from_slice(icc);
    out[at.ifd..].copy_from_slice(&tail);

    let rows =
        out[TIFF_PIXELS_AT..TIFF_PIXELS_AT + at.pixels].chunks_exact_mut((w as usize * 3).max(1));
    for (dst, y) in rows.zip(0..h) {
        let src = img.row(y).as_chunks::<4>().0;
        for (d, s) in dst.as_chunks_mut::<3>().0.iter_mut().zip(src) {
            *d = [s[2], s[1], s[0]];
        }
    }
    out[TIFF_PIXELS_AT + at.pixels..at.ifd].fill(0);
}

/// Where the parts of a TIFF go: the header, the pixels, the directory at an even offset, then
/// the values it points to.
struct TiffLayout {
    pixels: usize,
    ifd: usize,
    entries: u16,
    bits: usize,
    /// One rational for both directions.
    resolution: usize,
    profile: usize,
    end: usize,
}

fn tiff_layout(width: u32, height: u32, icc_len: usize) -> TiffLayout {
    let pixels = width as usize * height as usize * 3;
    let ifd = (TIFF_PIXELS_AT + pixels + 1) & !1;
    let entries: u16 = if icc_len == 0 { 13 } else { 14 };
    let bits = ifd + 2 + entries as usize * 12 + 4;
    let resolution = bits + 8;
    let profile = resolution + 8;
    TiffLayout {
        pixels,
        ifd,
        entries,
        bits,
        resolution,
        profile,
        end: profile + icc_len,
    }
}

/// A decoded PNG: opaque BGRA rows, top to bottom, `width * 4` bytes each — the layout of a
/// tiny-skia sprite with every pixel opaque.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// From `pHYs`, when the file says.
    pub dpi: Option<f32>,
}

/// Reads an 8-bit PNG of any color type. Transparent pixels are composed over white: the editor
/// shows and saves opaque images.
pub fn decode_png<R: BufRead + Seek>(input: R) -> io::Result<Decoded> {
    let mut decoder = png::Decoder::new(input);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(io::Error::other)?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    let dpi = info
        .pixel_dims
        .filter(|d| d.unit == png::Unit::Meter && d.xppu > 0)
        .map(|d| d.xppu as f32 * METRES_PER_INCH);
    let (color, _) = reader.output_color_type();
    let channels = color.samples();
    let mut data = vec![0u8; width as usize * height as usize * 4];
    let mut rows = data.chunks_exact_mut(width as usize * 4);
    while let Some(row) = reader.next_row().map_err(io::Error::other)? {
        let Some(out) = rows.next() else { break };
        let out = out.as_chunks_mut::<4>().0;
        let px = row.data().chunks_exact(channels);
        for (d, s) in out.iter_mut().zip(px) {
            let (r, g, b, a) = match *s {
                [l] => (l, l, l, 255),
                [l, a] => (l, l, l, a),
                [r, g, b] => (r, g, b, 255),
                [r, g, b, a] => (r, g, b, a),
                _ => (0, 0, 0, 255),
            };
            // Over white.
            let over = |c: u8| ((c as u16 * a as u16 + 255 * (255 - a as u16) + 127) / 255) as u8;
            *d = [over(b), over(g), over(r), 255];
        }
    }
    Ok(Decoded {
        width,
        height,
        data,
        dpi,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_keeps_colors() {
        // 2×2 BGRX with a padded stride: red, green / blue, white.
        let data = [
            0, 0, 255, 0, 0, 255, 0, 0, 9, 9, 9, 9, //
            255, 0, 0, 0, 255, 255, 255, 0, 9, 9, 9, 9,
        ];
        let img = BgrxView {
            width: 2,
            height: 2,
            stride: 12,
            data: &data,
        };
        let mut png_bytes = Vec::new();
        encode_png(img, &mut png_bytes, Compression::Fast, Some(120.0)).unwrap();
        let decoded = decode_png(io::Cursor::new(&png_bytes)).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        assert_eq!(
            decoded.data,
            [
                0, 0, 255, 255, 0, 255, 0, 255, //
                255, 0, 0, 255, 255, 255, 255, 255
            ]
        );
        // 125% on Windows: 4724 px per metre.
        assert!((decoded.dpi.unwrap() - 120.0).abs() < 0.02);

        let mut reader = png::Decoder::new(io::Cursor::new(png_bytes))
            .read_info()
            .unwrap();
        let mut rgb = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut rgb).unwrap();
        assert_eq!(rgb, [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
    }

    #[test]
    fn rgb_png_matches_bgrx_png() {
        let bgrx = [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0];
        let img = BgrxView {
            width: 2,
            height: 2,
            stride: 8,
            data: &bgrx,
        };
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let (mut a, mut b) = (Vec::new(), Vec::new());
        encode_png(img, &mut a, Compression::Fast, Some(144.0)).unwrap();
        encode_rgb_png(2, 2, &rgb, &mut b, Compression::Fast, Some(144.0)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn tiff_has_rgb_pixels_and_its_tags() {
        // 3×1 with a padded stride: red, green, blue.
        let data = [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 9, 9, 9, 9];
        let img = BgrxView {
            width: 3,
            height: 1,
            stride: 16,
            data: &data,
        };
        let icc = [1, 2, 3];
        let tiff = encode_tiff(img, 144.0, Some(&icc));
        let u16_at = |i: usize| u16::from_le_bytes([tiff[i], tiff[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(tiff[i..i + 4].try_into().unwrap());
        assert_eq!(&tiff[..4], b"II*\0");
        assert_eq!(
            &tiff[TIFF_PIXELS_AT..TIFF_PIXELS_AT + 9],
            [255, 0, 0, 0, 255, 0, 0, 0, 255]
        );
        let ifd = u32_at(4) as usize;
        assert_eq!(ifd % 2, 0);
        let count = u16_at(ifd) as usize;
        let tags: Vec<u16> = (0..count).map(|i| u16_at(ifd + 2 + i * 12)).collect();
        assert!(tags.is_sorted());
        let value = |tag: u16| {
            let i = tags.iter().position(|&t| t == tag).unwrap();
            u32_at(ifd + 2 + i * 12 + 8)
        };
        assert_eq!((value(256), value(257)), (3, 1));
        assert_eq!(value(273) as usize, TIFF_PIXELS_AT);
        assert_eq!(value(279), 9);
        let x_res = value(282) as usize;
        assert_eq!(u32_at(x_res) as f32 / u32_at(x_res + 4) as f32, 144.0);
        let profile = value(34675) as usize;
        assert_eq!(&tiff[profile..], icc);
        assert_eq!(u32_at(ifd + 2 + count * 12), 0);
    }

    #[test]
    fn crop_reads_the_inner_pixels() {
        // 3×3, every pixel's first byte is its index.
        let data: Vec<u8> = (0..9).flat_map(|i| [i, 0, 0, 0]).collect();
        let img = BgrxView {
            width: 3,
            height: 3,
            stride: 12,
            data: &data,
        };
        let part = img.crop(Rect::new(1, 1, 2, 2));
        let firsts = |y| part.row(y).iter().step_by(4).copied().collect::<Vec<_>>();
        assert_eq!((part.width, part.height), (2, 2));
        assert_eq!(firsts(0), [4, 5]);
        assert_eq!(firsts(1), [7, 8]);
    }
}
