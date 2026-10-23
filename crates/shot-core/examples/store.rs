//! The screenshots of the Microsoft Store listing: Shot on a made-up 1280 × 720 desktop at 300 %,
//! so 3840 × 2160 px, every Shot pixel drawn by this crate's own code.
//!
//! `cargo run -p shot-core --release --example store [folder]`
//!
//! Writes `ru/` and `en/` into `folder`, `target/store` by default: the same seven scenes on a
//! desktop whose windows speak the listing's language. Shot's own interface is Russian in both,
//! as it is in the app. `tools/store_media.py` runs this and puts the files into `store/media/`.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use shot_core::draw::{self, Area};
use shot_core::editor::{Editor, Input, Key, Open};
use shot_core::geom::{Point, Rect};
use shot_core::hotkey::{Hotkey, Os};
use shot_core::image::{BgrxMut, BgrxView, Compression, encode_png};
use shot_core::overlay::{Event, Monitor, Outcome, Overlay, Selection};
use shot_core::preview::{Card, Side, Status};
use shot_core::settings::{Destination, ThemeChoice};
use shot_core::text::{self, TypeStyle, Weight};
use shot_core::theme::{self, Color, Palette};
use shot_core::ui::settings::Values;
use shot_core::ui::{SettingsWindow, Welcome};
use tiny_skia::{
    FillRule, GradientStop, IntSize, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint,
    SpreadMode, Stroke, Transform,
};

/// Logical px to physical px.
const S: f32 = 3.0;
const LW: f32 = 1280.0;
const LH: f32 = 720.0;
const W: u32 = 3840;
const H: u32 = 2160;
const TASKBAR: f32 = 48.0;
const TITLE_BAR: f32 = 32.0;
const FILE_NAME: &str = "Shot 2026-10-23 17-03-15.png";

/// The words on the made-up desktop in one listing language.
struct Words {
    report: &'static str,
    nav: [&'static str; 5],
    heading: &'static str,
    period: &'static str,
    tiles: [(&'static str, &'static str); 3],
    week: &'static str,
    notes: &'static str,
    checklist: &'static str,
    items: [(&'static str, bool); 5],
    date: &'static str,
}

const RU: Words = Words {
    report: "Продажи — III квартал",
    nav: ["Обзор", "Продажи", "Клиенты", "Товары", "Настройки"],
    heading: "Продажи за квартал",
    period: "Июль — сентябрь",
    tiles: [
        ("Выручка", "3,9 млн"),
        ("Заказы", "1 284"),
        ("Возвраты", "2,1 %"),
    ],
    week: "Н",
    notes: "Заметки",
    checklist: "Перед релизом",
    items: [
        ("Написать список изменений", true),
        ("Собрать для Windows и macOS", true),
        ("Снять скриншоты для Store", false),
        ("Поставить тег v2.1.0", false),
        ("Опубликовать релиз", false),
    ],
    date: "23.10.2026",
};

const EN: Words = Words {
    report: "Sales — Q3 report",
    nav: ["Overview", "Sales", "Customers", "Products", "Settings"],
    heading: "Quarterly sales",
    period: "July – September",
    tiles: [
        ("Revenue", "$48.2k"),
        ("Orders", "1,284"),
        ("Returns", "2.1%"),
    ],
    week: "W",
    notes: "Notes",
    checklist: "Release checklist",
    items: [
        ("Write the changelog", true),
        ("Build for Windows and macOS", true),
        ("Take the Store screenshots", false),
        ("Tag v2.1.0", false),
        ("Publish the release", false),
    ],
    date: "10/23/2026",
};

/// A tiny-skia paint for `0xRRGGBB`, in the BGRA order this crate's pixmaps hold.
fn paint(rgb: u32) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia(rgb));
    paint.anti_alias = true;
    paint
}

fn skia(rgb: u32) -> tiny_skia::Color {
    let [_, r, g, b] = rgb.to_be_bytes();
    tiny_skia::Color::from_rgba8(b, g, r, 255)
}

fn ph((x, y): (f32, f32)) -> (f32, f32) {
    (x * S, y * S)
}

fn at((x, y): (f32, f32)) -> Point {
    Point::new(x as i32, y as i32)
}

/// Logical rect to physical.
fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(
        (x * S) as i32,
        (y * S) as i32,
        (w * S) as i32,
        (h * S) as i32,
    )
}

/// Logical coordinates in, drawn at S.
fn fill(p: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, radius: f32, rgb: u32) {
    let (x, y, w, h, radius) = (x * S, y * S, w * S, h * S, radius * S);
    if radius > 0.0 {
        draw::fill_round_rect(
            p,
            Area::new(x, y, w, h),
            radius,
            Color::hex(rgb << 8 | 0xFF),
        );
    } else if let Some(r) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        p.fill_rect(r, &paint(rgb), Transform::identity(), None);
    }
}

fn circle(p: &mut Pixmap, x: f32, y: f32, r: f32, rgb: u32) {
    if let Some(path) = PathBuilder::from_circle(x * S, y * S, r * S) {
        p.fill_path(
            &path,
            &paint(rgb),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Logical points joined by a line `width` logical px wide.
fn line(p: &mut Pixmap, points: &[(f32, f32)], width: f32, rgb: u32) {
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0 * S, points[0].1 * S);
    for (x, y) in &points[1..] {
        pb.line_to(x * S, y * S);
    }
    let stroke = Stroke {
        width: width * S,
        ..Stroke::default()
    };
    p.stroke_path(
        &pb.finish().unwrap(),
        &paint(rgb),
        &stroke,
        Transform::identity(),
        None,
    );
}

fn style(size: f32, weight: Weight) -> TypeStyle {
    TypeStyle {
        weight,
        size,
        line_height: (size * 1.4).round(),
    }
}

/// Text at logical (`x`, `top`); returns its logical width.
fn label(p: &mut Pixmap, x: f32, top: f32, size: f32, weight: Weight, rgb: u32, s: &str) -> f32 {
    text::draw(p, style(size, weight), S, s, x * S, top * S, &paint(rgb)) / S
}

fn label_width(size: f32, weight: Weight, s: &str) -> f32 {
    text::width(style(size, weight), S, s) / S
}

fn blit(p: &mut Pixmap, src: &Pixmap, x: i32, y: i32) {
    p.draw_pixmap(
        x,
        y,
        src.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

/// A Windows 11 window at logical (`x`, `y`), `w` × `h` with its title bar, in the color of the
/// title bar; its content is drawn by the caller below the bar.
/// A dialog (`form`) has no maximize button, and Windows draws it grayed out.
#[allow(clippy::too_many_arguments)]
fn window(
    p: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    title: &str,
    bar: u32,
    dark: bool,
    form: bool,
) {
    if let Some((shadow, left, top)) =
        draw::shadow(w * S, h * S, 8.0 * S, theme::SHADOW_LG, dark, S)
    {
        blit(p, &shadow, (x * S) as i32 - left, (y * S) as i32 - top);
    }
    let edge = if dark { 0x404040 } else { 0xC9CED6 };
    fill(
        p,
        x - 1.0 / S,
        y - 1.0 / S,
        w + 2.0 / S,
        h + 2.0 / S,
        8.0 + 1.0 / S,
        edge,
    );
    fill(p, x, y, w, h, 8.0, bar);
    let ink = if dark { 0xF3F3F3 } else { 0x1F1F1F };
    label(p, x + 14.0, y + 8.0, 12.0, Weight::Regular, ink, title);
    // Minimize, maximize, close: 46 px wide each, as Windows draws them.
    let glyph = if dark { 0xD0D0D0 } else { 0x404040 };
    let grayed = match (form, dark) {
        (false, _) => glyph,
        (true, false) => 0xB4B4B4,
        (true, true) => 0x5C5C5C,
    };
    let mid = y + TITLE_BAR / 2.0;
    let close = x + w - 23.0;
    let max = close - 46.0;
    let min = max - 46.0;
    line(
        p,
        &[(min - 5.0, mid), (min + 5.0, mid)],
        1.0 / S * 2.0,
        glyph,
    );
    line(
        p,
        &[
            (max - 5.0, mid - 5.0),
            (max + 5.0, mid - 5.0),
            (max + 5.0, mid + 5.0),
            (max - 5.0, mid + 5.0),
            (max - 5.0, mid - 5.0),
        ],
        1.0 / S * 2.0,
        grayed,
    );
    line(
        p,
        &[(close - 5.0, mid - 5.0), (close + 5.0, mid + 5.0)],
        1.0 / S * 2.0,
        glyph,
    );
    line(
        p,
        &[(close + 5.0, mid - 5.0), (close - 5.0, mid + 5.0)],
        1.0 / S * 2.0,
        glyph,
    );
}

/// Where the editor marks go, in physical desktop px.
struct Spots {
    report: Rect,
    notes: Rect,
    tallest_bar: (f32, f32),
    revenue: (f32, f32, f32, f32),
    orders: (f32, f32),
    returns: (f32, f32),
    low_bar: (f32, f32, f32, f32),
    /// The chart, a good area to drag.
    chart: ((f32, f32), (f32, f32)),
}

/// Shot's tray icon, `assets/app/windows/app-flat-48.png`, premultiplied in BGRA.
fn tray_icon() -> Pixmap {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/app/windows/app-flat-48.png");
    let file = BufReader::new(File::open(&path).expect("open the tray icon"));
    let mut reader = png::Decoder::new(file)
        .read_info()
        .expect("read the tray icon");
    let (w, h) = (reader.info().width, reader.info().height);
    let (color, _) = reader.output_color_type();
    assert_eq!(color, png::ColorType::Rgba, "the tray icon is RGBA");
    let mut data = Vec::with_capacity(w as usize * h as usize * 4);
    while let Some(row) = reader.next_row().expect("read a row") {
        for px in row.data().as_chunks::<4>().0 {
            let a = px[3] as u16;
            let pre = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            data.extend_from_slice(&[pre(px[2]), pre(px[1]), pre(px[0]), px[3]]);
        }
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h).unwrap()).unwrap()
}

/// A made-up desktop: a report window, a notes window over it and the task bar.
fn desktop(words: &Words) -> (Pixmap, Spots) {
    let mut p = Pixmap::new(W, H).unwrap();
    let wallpaper = LinearGradient::new(
        tiny_skia::Point::from_xy(0.0, 0.0),
        tiny_skia::Point::from_xy(W as f32, H as f32),
        vec![
            GradientStop::new(0.0, skia(0x3E5A9A)),
            GradientStop::new(0.55, skia(0x4D5FA8)),
            GradientStop::new(1.0, skia(0x6A56B0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    let all = tiny_skia::Rect::from_xywh(0.0, 0.0, W as f32, H as f32).unwrap();
    let wall = Paint {
        shader: wallpaper,
        ..Default::default()
    };
    p.fill_rect(all, &wall, Transform::identity(), None);
    circle(&mut p, 1090.0, 90.0, 400.0, 0x5A6CB4);
    circle(&mut p, 1150.0, 40.0, 250.0, 0x6676BE);
    circle(&mut p, 120.0, 760.0, 330.0, 0x42579A);

    // The report.
    let (x, y, w, h) = (64.0, 44.0, 760.0, 500.0);
    window(&mut p, x, y, w, h, words.report, 0xF3F3F3, false, false);
    let body = y + TITLE_BAR;
    fill(&mut p, x, body, w, h - TITLE_BAR, 0.0, 0xFFFFFF);
    fill(&mut p, x, y + h - 8.0, w, 8.0, 8.0, 0xFFFFFF);
    fill(&mut p, x, body, 168.0, h - TITLE_BAR - 8.0, 0.0, 0xF8F9FA);
    fill(&mut p, x, y + h - 16.0, 168.0, 16.0, 8.0, 0xF8F9FA);
    fill(
        &mut p,
        x + 168.0,
        body,
        1.0 / S * 2.0,
        h - TITLE_BAR,
        0.0,
        0xE9ECEF,
    );
    for (i, item) in words.nav.iter().enumerate() {
        let top = body + 20.0 + i as f32 * 34.0;
        let (weight, rgb) = if i == 1 {
            fill(&mut p, x + 10.0, top - 6.0, 148.0, 30.0, 6.0, 0xE7ECFF);
            (Weight::SemiBold, 0x3B5BDB)
        } else {
            (Weight::Regular, 0x495057)
        };
        label(&mut p, x + 24.0, top, 14.0, weight, rgb, item);
    }
    let mx = x + 196.0;
    label(
        &mut p,
        mx,
        body + 18.0,
        22.0,
        Weight::SemiBold,
        0x212529,
        words.heading,
    );
    label(
        &mut p,
        mx,
        body + 52.0,
        13.0,
        Weight::Regular,
        0x868E96,
        words.period,
    );
    let tile = |i: usize| (mx + i as f32 * 178.0, body + 86.0);
    for (i, (name, value)) in words.tiles.iter().enumerate() {
        let (tx, ty) = tile(i);
        fill(&mut p, tx, ty, 164.0, 72.0, 8.0, 0xF1F3F5);
        label(
            &mut p,
            tx + 14.0,
            ty + 11.0,
            12.0,
            Weight::Regular,
            0x868E96,
            name,
        );
        label(
            &mut p,
            tx + 14.0,
            ty + 31.0,
            22.0,
            Weight::SemiBold,
            0x212529,
            value,
        );
    }
    let base = y + h - 46.0;
    for i in 0..5 {
        fill(
            &mut p,
            mx,
            base - i as f32 * 48.0,
            528.0,
            1.0 / S * 2.0,
            0.0,
            0xE9ECEF,
        );
    }
    let heights = [104.0, 132.0, 82.0, 164.0, 120.0, 190.0, 150.0];
    let mut tallest = (0.0, 0.0);
    let mut low = (0.0, 0.0, 0.0, 0.0);
    for (i, bar) in heights.iter().enumerate() {
        let bx = mx + 16.0 + i as f32 * 74.0;
        let rgb = if i == 5 { 0x4C6EF5 } else { 0xA5B4FC };
        fill(&mut p, bx, base - bar, 46.0, *bar, 4.0, rgb);
        fill(&mut p, bx, base - 4.0, 46.0, 4.0, 0.0, rgb);
        label(
            &mut p,
            bx + 12.0,
            base + 8.0,
            12.0,
            Weight::Regular,
            0x868E96,
            &format!("{}{}", words.week, i + 1),
        );
        if i == 5 {
            tallest = (bx + 23.0, base - bar);
        }
        if i == 2 {
            low = (bx - 10.0, base - bar - 12.0, 66.0, bar + 44.0);
        }
    }

    // The notes.
    let (nx, ny, nw, nh) = (804.0, 196.0, 420.0, 316.0);
    window(&mut p, nx, ny, nw, nh, words.notes, 0xF3F3F3, false, false);
    fill(
        &mut p,
        nx,
        ny + TITLE_BAR,
        nw,
        nh - TITLE_BAR - 8.0,
        0.0,
        0xFFFFFF,
    );
    fill(&mut p, nx, ny + nh - 16.0, nw, 16.0, 8.0, 0xFFFFFF);
    label(
        &mut p,
        nx + 22.0,
        ny + 54.0,
        18.0,
        Weight::SemiBold,
        0x212529,
        words.checklist,
    );
    for (i, (item, done)) in words.items.iter().enumerate() {
        let top = ny + 96.0 + i as f32 * 38.0;
        if *done {
            fill(&mut p, nx + 22.0, top + 2.0, 18.0, 18.0, 4.0, 0x40C057);
            line(
                &mut p,
                &[
                    (nx + 26.5, top + 11.0),
                    (nx + 29.5, top + 14.5),
                    (nx + 35.5, top + 7.0),
                ],
                2.0,
                0xFFFFFF,
            );
        } else {
            fill(&mut p, nx + 22.0, top + 2.0, 18.0, 18.0, 4.0, 0xADB5BD);
            fill(&mut p, nx + 23.5, top + 3.5, 15.0, 15.0, 3.0, 0xFFFFFF);
        }
        let rgb = if *done { 0x868E96 } else { 0x343A40 };
        label(&mut p, nx + 52.0, top, 14.0, Weight::Regular, rgb, item);
    }

    // The task bar: Windows 11, the buttons in the middle, the tray and the clock on the right.
    let ty = LH - TASKBAR;
    fill(&mut p, 0.0, ty, LW, TASKBAR, 0.0, 0x1C2230);
    fill(&mut p, 0.0, ty, LW, 1.0 / S * 2.0, 0.0, 0x2E3648);
    let buttons = [0x4C6EF5, 0x40C057, 0xFAB005, 0xFA5252, 0xBE4BDB, 0x15AABF];
    let first = LW / 2.0 - (buttons.len() as f32 * 44.0) / 2.0;
    for (i, rgb) in buttons.iter().enumerate() {
        fill(
            &mut p,
            first + i as f32 * 44.0 + 6.0,
            ty + 8.0,
            32.0,
            32.0,
            7.0,
            *rgb,
        );
    }
    let clock_w = label_width(12.0, Weight::Regular, words.date);
    let cx = LW - 16.0 - clock_w;
    let time_w = label_width(12.0, Weight::Regular, "17:03");
    label(
        &mut p,
        cx + clock_w - time_w,
        ty + 7.0,
        12.0,
        Weight::Regular,
        0xF1F3F5,
        "17:03",
    );
    label(
        &mut p,
        cx,
        ty + 24.0,
        12.0,
        Weight::Regular,
        0xF1F3F5,
        words.date,
    );
    let icon = tray_icon();
    let ix = cx - 24.0 - 16.0;
    blit(&mut p, &icon, (ix * S) as i32, ((ty + 16.0) * S) as i32);
    // The up arrow of the hidden tray icons.
    line(
        &mut p,
        &[
            (ix - 22.0, ty + 26.0),
            (ix - 17.0, ty + 21.0),
            (ix - 12.0, ty + 26.0),
        ],
        1.0,
        0xE9ECEF,
    );

    let (rx, ry) = tile(0);
    let spots = Spots {
        report: rect(x, y, w, h),
        notes: rect(nx, ny, nw, nh),
        tallest_bar: ph(tallest),
        revenue: (rx * S, ry * S, 164.0 * S, 72.0 * S),
        orders: ph((tile(1).0 + 150.0, tile(1).1 + 14.0)),
        returns: ph((tile(2).0 + 150.0, tile(2).1 + 14.0)),
        low_bar: (low.0 * S, low.1 * S, low.2 * S, low.3 * S),
        chart: (ph((mx - 16.0, body + 78.0)), ph((mx + 536.0, base + 34.0))),
    };
    (p, spots)
}

/// The Windows pointer at physical (`x`, `y`).
fn pointer(p: &mut Pixmap, (x, y): (f32, f32)) {
    let shape = [
        (0.0, 0.0),
        (0.0, 16.0),
        (4.0, 12.5),
        (7.0, 19.0),
        (9.5, 18.0),
        (6.5, 11.5),
        (11.5, 11.5),
    ];
    let mut pb = PathBuilder::new();
    pb.move_to(x, y);
    for (dx, dy) in &shape[1..] {
        pb.line_to(x + dx * S, y + dy * S);
    }
    pb.close();
    let path = pb.finish().unwrap();
    p.fill_path(
        &path,
        &paint(0xFFFFFF),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let stroke = Stroke {
        width: S,
        ..Stroke::default()
    };
    p.stroke_path(
        &path,
        &paint(0x000000),
        &stroke,
        Transform::identity(),
        None,
    );
}

fn save(path: &Path, p: &Pixmap) {
    let view = BgrxView {
        width: p.width(),
        height: p.height(),
        stride: p.width() as usize * 4,
        data: p.data(),
    };
    let out = BufWriter::new(File::create(path).expect("create PNG"));
    encode_png(view, out, Compression::Fast, Some(96.0 * S)).expect("write PNG");
    println!("wrote {}", path.display());
}

/// BGRX pixels to an opaque pixmap.
fn opaque(mut data: Vec<u8>) -> Pixmap {
    for px in data.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    Pixmap::from_vec(data, IntSize::from_wh(W, H).unwrap()).unwrap()
}

/// The capture of `r` from the desktop, as the editor gets it.
fn crop(desk: &Pixmap, r: Rect) -> Pixmap {
    let mut out = Vec::with_capacity(r.area() as usize * 4);
    for row in r.y..r.y + r.height {
        let at = (row as usize * W as usize + r.x as usize) * 4;
        out.extend_from_slice(&desk.data()[at..at + r.width as usize * 4]);
    }
    Pixmap::from_vec(
        out,
        IntSize::from_wh(r.width as u32, r.height as u32).unwrap(),
    )
    .unwrap()
}

fn view(p: &Pixmap) -> BgrxView<'_> {
    BgrxView {
        width: p.width(),
        height: p.height(),
        stride: p.width() as usize * 4,
        data: p.data(),
    }
}

/// The overlay over the whole desktop after `events`.
fn overlay(desk: &Pixmap, spots: &Spots, start: (f32, f32), events: &[Event]) -> (Pixmap, Overlay) {
    let full = Rect::new(0, 0, W as i32, H as i32);
    let monitor = Monitor {
        rect: full,
        work: Rect::new(0, 0, W as i32, H as i32 - (TASKBAR * S) as i32),
        scale: S,
    };
    let windows = vec![spots.notes, spots.report];
    let mut o = Overlay::new(Selection::new(vec![monitor], windows, 12, at(start)), true);
    for e in events {
        o.handle(*e);
    }
    let mut canvas = vec![0u8; desk.data().len()];
    let mut out = BgrxMut {
        width: W,
        height: H,
        stride: W as usize * 4,
        data: &mut canvas,
    };
    o.render(0, &view(desk), &mut out, full);
    (opaque(canvas), o)
}

/// A preview card with its shadow at physical (`x`, `y`).
fn card(p: &mut Pixmap, card: &Card, (x, y): (i32, i32), hover: bool) {
    let palette = Palette::of(false);
    let (shadow, offset) = card.render_shadow(palette).unwrap();
    blit(p, &shadow, x - offset.x, y - offset.y);
    blit(p, &card.render(palette, hover).unwrap(), x, y);
}

/// A window of Shot's at logical (`x`, `y`): its title bar takes the color of its content's top
/// left pixel, as a Windows 11 window with Mica does.
fn shot_window(
    p: &mut Pixmap,
    x: f32,
    y: f32,
    title: &str,
    dark: bool,
    form: bool,
    client: &Pixmap,
) {
    let w = client.width() as f32 / S;
    let h = client.height() as f32 / S;
    let px = &client.data()[0..4];
    let bar = (px[2] as u32) << 16 | (px[1] as u32) << 8 | px[0] as u32;
    window(p, x, y, w, h + TITLE_BAR, title, bar, dark, form);
    // Round the bottom corners of the client the way the window rounds them.
    let mut shaped = Pixmap::new(client.width(), client.height()).unwrap();
    let clip = Area::new(0.0, -20.0 * S, w * S, (h + 20.0) * S);
    draw::draw_clipped(&mut shaped, client, 0, 0, clip, 8.0 * S);
    blit(p, &shaped, (x * S) as i32, ((y + TITLE_BAR) * S) as i32);
}

/// A settings-like window model drawn into an opaque pixmap.
fn client(size: (u32, u32), draw_into: impl FnOnce(&mut BgrxMut)) -> Pixmap {
    let (w, h) = size;
    let mut data = vec![0u8; w as usize * h as usize * 4];
    draw_into(&mut BgrxMut {
        width: w,
        height: h,
        stride: w as usize * 4,
        data: &mut data,
    });
    for px in data.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h).unwrap()).unwrap()
}

fn values() -> Values {
    Values {
        hotkey: Hotkey::default_for(Os::Windows),
        hotkey_works: true,
        destination: Destination::Both,
        folder: r"C:\Users\Anna\Pictures\Shot".to_string(),
        preview: true,
        preview_seconds: 5,
        preview_side: Side::Left,
        autostart: true,
        theme: ThemeChoice::System,
    }
}

fn scenes(dir: &Path, words: &Words) {
    fs::create_dir_all(dir).expect("create the folder");
    let (desk, spots) = desktop(words);

    // 1. Drag an area over the chart; the size shows at the cursor.
    let (from, to) = spots.chart;
    let start = ph((560.0, 600.0));
    let (shot, mut o) = overlay(
        &desk,
        &spots,
        start,
        &[
            Event::Move(at(from)),
            Event::Press(at(from)),
            Event::Move(at(to)),
        ],
    );
    save(&dir.join("1-area.png"), &shot);
    let capture = match o.handle(Event::Release(at(to))).outcome {
        Some(Outcome::Capture(target)) => target.rect,
        other => panic!("the drag captured {other:?}"),
    };

    // 2. Hover the notes: the window under the cursor lights up, a click takes it.
    let over = ph((1010.0, 300.0));
    let (shot, _) = overlay(&desk, &spots, start, &[Event::Move(at(over))]);
    save(&dir.join("2-window.png"), &shot);

    // 3. Two captures in a row: the cards stack up from the bottom left corner.
    let area = crop(&desk, capture);
    let notes = crop(&desk, spots.notes);
    let newest = Card::new(
        &view(&area),
        S,
        Status::CopiedAndSaved,
        Some("Открыть в редакторе"),
    )
    .unwrap();
    let older = Card::new(
        &view(&notes),
        S,
        Status::CopiedAndSaved,
        Some("Открыть в редакторе"),
    )
    .unwrap();
    let margin = (16.0 * S) as i32;
    let bottom = H as i32 - (TASKBAR * S) as i32 - margin;
    let low = (margin, bottom - newest.geometry.height);
    let high = (margin, low.1 - (8.0 * S) as i32 - older.geometry.height);
    let mut p = desk.clone();
    card(&mut p, &older, high, false);
    card(&mut p, &newest, low, true);
    let g = &newest.geometry;
    pointer(
        &mut p,
        ((low.0 + g.width / 2) as f32, (low.1 + g.height / 2) as f32),
    );
    save(&dir.join("3-preview.png"), &p);

    // 4. The editor: a frame, an arrow, numbered steps and an ellipse in another color.
    let size = ((1000.0 * S) as u32, (560.0 * S) as u32);
    let open = Open {
        image: area.clone(),
        image_scale: S,
        name: FILE_NAME.into(),
        has_file: true,
        window: size,
        scale: S,
        dark: false,
        destination: Destination::Both,
        os: Os::Windows,
    };
    let mut now = Instant::now();
    let mut editor = Editor::new(open, now);
    let mut send = |editor: &mut Editor, input: Input| {
        now += Duration::from_millis(40);
        editor.handle(input, now);
    };
    let fit = |room: f32, len: i32| ((room as i32 - len) / 2) as f32;
    let image_at = (
        24.0 * S + fit(size.0 as f32 - 48.0 * S, capture.width),
        68.0 * S + fit(size.1 as f32 - 92.0 * S, capture.height),
    );
    let to_window = |(x, y): (f32, f32)| {
        (
            image_at.0 + x - capture.x as f32,
            image_at.1 + y - capture.y as f32,
        )
    };
    let drag = |editor: &mut Editor,
                send: &mut dyn FnMut(&mut Editor, Input),
                a: (f32, f32),
                b: (f32, f32)| {
        send(
            editor,
            Input::Move {
                x: a.0,
                y: a.1,
                shift: false,
            },
        );
        send(
            editor,
            Input::Down {
                x: a.0,
                y: a.1,
                shift: false,
            },
        );
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            send(editor, Input::Move { x, y, shift: false });
        }
        send(
            editor,
            Input::Up {
                x: b.0,
                y: b.1,
                shift: false,
            },
        );
    };
    let click =
        |editor: &mut Editor, send: &mut dyn FnMut(&mut Editor, Input), (x, y): (f32, f32)| {
            send(editor, Input::Move { x, y, shift: false });
            send(editor, Input::Down { x, y, shift: false });
            send(editor, Input::Up { x, y, shift: false });
        };
    let key = |editor: &mut Editor, send: &mut dyn FnMut(&mut Editor, Input), c: char| {
        send(
            editor,
            Input::Key {
                key: Key::Char(c),
                ctrl: false,
                shift: false,
            },
        );
        send(
            editor,
            Input::KeyUp {
                key: Key::Char(c),
                shift: false,
            },
        );
    };
    // The swatches sit in the toolbar centered at the top: 279 + 32 i logical px from its left.
    let toolbar_left = (size.0 as f32 - 888.0 * S) / 2.0;
    let swatch = |i: usize| {
        (
            toolbar_left + (279.0 + 32.0 * i as f32 + 14.0) * S,
            34.0 * S,
        )
    };

    key(&mut editor, &mut send, 'r');
    let (rx, ry, rw, rh) = spots.revenue;
    drag(
        &mut editor,
        &mut send,
        to_window((rx - 6.0 * S, ry - 6.0 * S)),
        to_window((rx + rw + 6.0 * S, ry + rh + 6.0 * S)),
    );
    key(&mut editor, &mut send, 'a');
    let bar = spots.tallest_bar;
    drag(
        &mut editor,
        &mut send,
        to_window((bar.0 - 190.0 * S, bar.1 - 34.0 * S)),
        to_window((bar.0 - 14.0 * S, bar.1 - 6.0 * S)),
    );
    // A swatch recolors the selected object, and a new object stays selected: click the empty
    // backdrop with the select tool first.
    let pick = |editor: &mut Editor, send: &mut dyn FnMut(&mut Editor, Input), color: usize| {
        key(editor, send, 'v');
        click(editor, send, (40.0 * S, 300.0 * S));
        click(editor, send, swatch(color));
    };
    pick(&mut editor, &mut send, 4);
    key(&mut editor, &mut send, 'o');
    let (lx, ly, lw, lh) = spots.low_bar;
    drag(
        &mut editor,
        &mut send,
        to_window((lx, ly)),
        to_window((lx + lw, ly + lh)),
    );
    pick(&mut editor, &mut send, 5);
    key(&mut editor, &mut send, 'n');
    for spot in [spots.orders, spots.returns] {
        click(&mut editor, &mut send, to_window(spot));
    }
    let rest = ph((560.0, 500.0));
    send(
        &mut editor,
        Input::Move {
            x: rest.0,
            y: rest.1,
            shift: false,
        },
    );
    editor.take_damage();
    let mut client_px = Pixmap::new(size.0, size.1).unwrap();
    editor.render(
        &mut client_px,
        Rect::new(0, 0, size.0 as i32, size.1 as i32),
    );
    for px in client_px.data_mut().as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let mut p = desk.clone();
    let (ex, ey) = (140.0, 40.0);
    shot_window(&mut p, ex, ey, &editor.title(), false, false, &client_px);
    pointer(&mut p, (ex * S + rest.0, (ey + TITLE_BAR) * S + rest.1));
    save(&dir.join("4-editor.png"), &p);

    // 5. The first start: Shot says where it lives and offers to start with Windows.
    let mut welcome = Welcome::new(Hotkey::default_for(Os::Windows), Os::Windows, S, false);
    let c = client(welcome.content_size(), |out| welcome.draw(out));
    let mut p = desk.clone();
    let (cw, ch) = (c.width() as f32 / S, c.height() as f32 / S);
    shot_window(
        &mut p,
        (LW - cw) / 2.0,
        (LH - TASKBAR - ch - TITLE_BAR) / 2.0,
        "Shot",
        false,
        true,
        &c,
    );
    save(&dir.join("5-welcome.png"), &p);

    // 6, 7. The settings in the light and the dark theme.
    for (name, dark) in [
        ("6-settings-light.png", false),
        ("7-settings-dark.png", true),
    ] {
        let version = env!("CARGO_PKG_VERSION");
        let mut settings = SettingsWindow::new(values(), version, Os::Windows, S, dark);
        let c = client(settings.content_size(), |out| settings.draw(out));
        let mut p = desk.clone();
        let (cw, ch) = (c.width() as f32 / S, c.height() as f32 / S);
        let y = ((LH - TASKBAR - ch - TITLE_BAR) / 2.0).max(16.0);
        shot_window(&mut p, (LW - cw) / 2.0, y, "Настройки Shot", dark, true, &c);
        save(&dir.join(name), &p);
    }
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or("target/store".into()));
    let _ = fs::remove_dir_all(&dir);
    scenes(&dir.join("ru"), &RU);
    scenes(&dir.join("en"), &EN);
}
