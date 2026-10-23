//! The animation for the README: a capture on a made-up desktop, every Shot pixel drawn by this
//! crate's own code — the overlay, the preview card and the editor.
//!
//! `cargo run -p shot-core --release --example demo [folder]`
//!
//! Writes `frame-NNN.png` and `frames.txt` (one `file duration_ms scene` per line) into `folder`,
//! `target/demo` by default, for a GIF encoder to put together; frames of one scene can share a
//! palette.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use shot_core::draw::{self, Area};
use shot_core::editor::{Editor, Input, Key, Open};
use shot_core::geom::{Point, Rect};
use shot_core::hotkey::Os;
use shot_core::image::{BgrxMut, BgrxView, Compression, encode_png};
use shot_core::overlay::{Event, Monitor, Outcome, Overlay, Selection};
use shot_core::preview::{Card, Status};
use shot_core::settings::Destination;
use shot_core::text::{self, TypeStyle, Weight};
use shot_core::theme::{self, Color, Palette};
use tiny_skia::{FillRule, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

const W: u32 = 1024;
const H: u32 = 640;
const TASKBAR: f32 = 44.0;
/// Milliseconds per frame of a movement.
const STEP: u32 = 40;
const TITLE_BAR: f32 = 32.0;

/// A tiny-skia paint for `0xRRGGBB`, in the BGRA order this crate's pixmaps hold.
fn paint(rgb: u32) -> Paint<'static> {
    let [_, r, g, b] = rgb.to_be_bytes();
    let mut paint = Paint::default();
    paint.set_color_rgba8(b, g, r, 255);
    paint.anti_alias = true;
    paint
}

fn fill(p: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, radius: f32, rgb: u32) {
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
    if let Some(path) = PathBuilder::from_circle(x, y, r) {
        p.fill_path(
            &path,
            &paint(rgb),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn label(p: &mut Pixmap, x: f32, top: f32, size: f32, weight: Weight, rgb: u32, s: &str) -> f32 {
    let style = TypeStyle {
        weight,
        size,
        line_height: (size * 1.4).round(),
    };
    text::draw(p, style, 1.0, s, x, top, &paint(rgb))
}

/// A window with a plain title bar and a shadow; its content is drawn by the caller.
fn window(p: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, title: &str) {
    // The shadow pixmap holds the window's shape at (`left`, `top`).
    if let Some((shadow, left, top)) = draw::shadow(w, h, 8.0, theme::SHADOW_LG, false, 1.0) {
        let (sx, sy) = (x as i32 - left, y as i32 - top);
        p.draw_pixmap(
            sx,
            sy,
            shadow.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    fill(p, x - 1.0, y - 1.0, w + 2.0, h + 2.0, 9.0, 0xC9CED6);
    fill(p, x, y, w, h, 8.0, 0xFFFFFF);
    fill(p, x, y, w, TITLE_BAR, 8.0, 0xF1F3F5);
    fill(p, x, y + 16.0, w, TITLE_BAR - 16.0, 0.0, 0xF1F3F5);
    fill(p, x, y + TITLE_BAR, w, 1.0, 0.0, 0xDEE2E6);
    label(
        p,
        x + 14.0,
        y + 7.0,
        13.0,
        Weight::SemiBold,
        0x343A40,
        title,
    );
    for i in 0..3 {
        circle(p, x + w - 20.0 - i as f32 * 20.0, y + 16.0, 5.0, 0xCED4DA);
    }
}

/// Where the editor marks go, in desktop px.
struct Spots {
    window_a: Rect,
    window_b: Rect,
    tallest_bar: (f32, f32),
    revenue: (f32, f32, f32, f32),
    orders: (f32, f32),
    returns: (f32, f32),
}

/// A made-up desktop: a report window, a checklist window over it and a task bar.
fn desktop() -> (Pixmap, Spots) {
    let mut p = Pixmap::new(W, H).unwrap();
    fill(&mut p, 0.0, 0.0, W as f32, H as f32, 0.0, 0x4A6491);
    circle(&mut p, 860.0, 80.0, 330.0, 0x56709C);
    circle(&mut p, 120.0, 640.0, 260.0, 0x435C88);

    // The report.
    let (x, y, w, h) = (36.0, 34.0, 600.0, 410.0);
    window(&mut p, x, y, w, h, "Sales — Q3 report");
    fill(
        &mut p,
        x,
        y + TITLE_BAR + 1.0,
        136.0,
        h - TITLE_BAR - 1.0,
        0.0,
        0xF8F9FA,
    );
    fill(
        &mut p,
        x + 136.0,
        y + TITLE_BAR + 1.0,
        1.0,
        h - TITLE_BAR - 1.0,
        0.0,
        0xE9ECEF,
    );
    for (i, item) in ["Overview", "Sales", "Customers", "Products", "Settings"]
        .iter()
        .enumerate()
    {
        let top = y + 50.0 + i as f32 * 30.0;
        if i == 1 {
            fill(&mut p, x + 8.0, top - 4.0, 120.0, 26.0, 6.0, 0xE7ECFF);
        }
        let (weight, rgb) = if i == 1 {
            (Weight::SemiBold, 0x3B5BDB)
        } else {
            (Weight::Regular, 0x495057)
        };
        label(&mut p, x + 20.0, top, 13.0, weight, rgb, item);
    }
    let mx = x + 156.0;
    label(
        &mut p,
        mx,
        y + 46.0,
        18.0,
        Weight::SemiBold,
        0x212529,
        "Quarterly sales",
    );
    label(
        &mut p,
        mx,
        y + 74.0,
        12.0,
        Weight::Regular,
        0x868E96,
        "July – September",
    );
    let tiles = [
        ("Revenue", "$48.2k"),
        ("Orders", "1,284"),
        ("Returns", "2.1%"),
    ];
    let tile = |i: usize| (mx + i as f32 * 142.0, y + 102.0);
    for (i, (name, value)) in tiles.iter().enumerate() {
        let (tx, ty) = tile(i);
        fill(&mut p, tx, ty, 130.0, 62.0, 8.0, 0xF1F3F5);
        label(
            &mut p,
            tx + 12.0,
            ty + 9.0,
            11.0,
            Weight::Regular,
            0x868E96,
            name,
        );
        label(
            &mut p,
            tx + 12.0,
            ty + 27.0,
            18.0,
            Weight::SemiBold,
            0x212529,
            value,
        );
    }
    let base = y + 372.0;
    for i in 0..4 {
        fill(
            &mut p,
            mx,
            base - i as f32 * 45.0,
            420.0,
            1.0,
            0.0,
            0xE9ECEF,
        );
    }
    let heights = [92.0, 118.0, 74.0, 146.0, 108.0, 170.0];
    let mut tallest = (0.0, 0.0);
    for (i, bar) in heights.iter().enumerate() {
        let bx = mx + 14.0 + i as f32 * 70.0;
        let rgb = if i == 5 { 0x4C6EF5 } else { 0xA5B4FC };
        fill(&mut p, bx, base - bar, 42.0, *bar, 4.0, rgb);
        fill(&mut p, bx, base - 4.0, 42.0, 4.0, 0.0, rgb);
        label(
            &mut p,
            bx + 9.0,
            base + 6.0,
            11.0,
            Weight::Regular,
            0x868E96,
            &format!("W{}", i + 1),
        );
        if i == 5 {
            tallest = (bx + 21.0, base - bar);
        }
    }

    // The checklist.
    let (bx, by, bw, bh) = (618.0, 168.0, 370.0, 276.0);
    window(&mut p, bx, by, bw, bh, "Notes");
    label(
        &mut p,
        bx + 20.0,
        by + 50.0,
        16.0,
        Weight::SemiBold,
        0x212529,
        "Release checklist",
    );
    let items = [
        ("Write the changelog", true),
        ("Build for Windows and macOS", true),
        ("Record the README animation", false),
        ("Tag v1.0.0", false),
        ("Publish the release", false),
    ];
    for (i, (item, done)) in items.iter().enumerate() {
        let top = by + 88.0 + i as f32 * 34.0;
        if *done {
            fill(&mut p, bx + 20.0, top + 2.0, 16.0, 16.0, 4.0, 0x40C057);
            let mut pb = PathBuilder::new();
            pb.move_to(bx + 24.0, top + 10.0);
            pb.line_to(bx + 27.0, top + 13.5);
            pb.line_to(bx + 32.5, top + 6.5);
            let stroke = Stroke {
                width: 2.0,
                ..Stroke::default()
            };
            p.stroke_path(
                &pb.finish().unwrap(),
                &paint(0xFFFFFF),
                &stroke,
                Transform::identity(),
                None,
            );
        } else {
            fill(&mut p, bx + 20.0, top + 2.0, 16.0, 16.0, 4.0, 0xADB5BD);
            fill(&mut p, bx + 21.5, top + 3.5, 13.0, 13.0, 3.0, 0xFFFFFF);
        }
        let rgb = if *done { 0x868E96 } else { 0x343A40 };
        label(&mut p, bx + 46.0, top, 13.0, Weight::Regular, rgb, item);
    }

    // The task bar.
    let ty = H as f32 - TASKBAR;
    fill(&mut p, 0.0, ty, W as f32, TASKBAR, 0.0, 0x1E2633);
    let icons = [0x4C6EF5, 0x40C057, 0xFAB005, 0xFA5252, 0xBE4BDB];
    for (i, rgb) in icons.iter().enumerate() {
        let ix = W as f32 / 2.0 - 96.0 + i as f32 * 40.0;
        fill(&mut p, ix, ty + 8.0, 28.0, 28.0, 7.0, *rgb);
    }
    label(
        &mut p,
        W as f32 - 58.0,
        ty + 12.0,
        13.0,
        Weight::Regular,
        0xF1F3F5,
        "17:03",
    );

    let (rx, ry) = tile(0);
    let spots = Spots {
        window_a: Rect::new(x as i32, y as i32, w as i32, h as i32),
        window_b: Rect::new(bx as i32, by as i32, bw as i32, bh as i32),
        tallest_bar: tallest,
        revenue: (rx, ry, 130.0, 62.0),
        orders: (tile(1).0 + 118.0, tile(1).1 + 12.0),
        returns: (tile(2).0 + 118.0, tile(2).1 + 12.0),
    };
    (p, spots)
}

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
    pb.move_to(x + shape[0].0, y + shape[0].1);
    for (dx, dy) in &shape[1..] {
        pb.line_to(x + dx, y + dy);
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
        width: 1.0,
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

fn crosshair(p: &mut Pixmap, (x, y): (f32, f32)) {
    let mut pb = PathBuilder::new();
    pb.move_to(x - 9.0, y + 0.5);
    pb.line_to(x + 10.0, y + 0.5);
    pb.move_to(x + 0.5, y - 9.0);
    pb.line_to(x + 0.5, y + 10.0);
    let path = pb.finish().unwrap();
    for (width, rgb) in [(3.0, 0xFFFFFF), (1.0, 0x000000)] {
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        p.stroke_path(&path, &paint(rgb), &stroke, Transform::identity(), None);
    }
}

fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// `n` points from `a` (excluded) to `b` (included), slow at both ends.
fn path(a: (f32, f32), b: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    (1..=n)
        .map(|i| {
            let t = ease(i as f32 / n as f32);
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        })
        .collect()
}

struct Film {
    dir: PathBuf,
    list: String,
    scene: &'static str,
}

impl Film {
    /// One frame of BGRX or BGRA pixels the size of the desktop, shown for `ms`.
    fn frame(&mut self, data: &[u8], ms: u32) {
        let name = format!("frame-{:03}.png", self.list.lines().count());
        let view = BgrxView {
            width: W,
            height: H,
            stride: W as usize * 4,
            data,
        };
        let out = BufWriter::new(File::create(self.dir.join(&name)).expect("create PNG"));
        encode_png(view, out, Compression::Fast, None).expect("write PNG");
        writeln!(self.list, "{name} {ms} {}", self.scene).unwrap();
    }
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

/// The editor window over the desktop.
struct EditorShot {
    editor: Editor,
    at: (f32, f32),
    size: (u32, u32),
    /// Image px to window px: where the image's top-left corner is.
    image: (f32, f32),
    now: Instant,
}

impl EditorShot {
    fn handle(&mut self, input: Input) {
        self.now += Duration::from_millis(STEP as u64);
        self.editor.handle(input, self.now);
        self.editor.take_damage();
    }

    /// Desktop px of the capture to window px.
    fn to_window(&self, capture: Rect, (x, y): (f32, f32)) -> (f32, f32) {
        (
            self.image.0 + x - capture.x as f32,
            self.image.1 + y - capture.y as f32,
        )
    }

    fn compose(&self, desk: &Pixmap, cursor: (f32, f32), cross: bool) -> Pixmap {
        let mut p = desk.clone();
        let (x, y) = self.at;
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        window(
            &mut p,
            x,
            y,
            w,
            h + TITLE_BAR,
            "Shot 2026-10-22 17-03-15.png",
        );
        let mut client = Pixmap::new(self.size.0, self.size.1).unwrap();
        let whole = Rect::new(0, 0, self.size.0 as i32, self.size.1 as i32);
        self.editor.render(&mut client, whole);
        // The editor draws opaque pixels for a window and leaves alpha alone.
        for px in client.data_mut().as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let (cx, cy) = (x as i32, (y + TITLE_BAR) as i32);
        p.draw_pixmap(
            cx,
            cy,
            client.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        let at = (x + cursor.0, y + TITLE_BAR + cursor.1);
        if cross {
            crosshair(&mut p, at)
        } else {
            pointer(&mut p, at)
        }
        p
    }
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or("target/demo".into()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create the folder");
    let mut film = Film {
        dir: dir.clone(),
        list: String::new(),
        scene: "desktop",
    };
    let (desk, spots) = desktop();

    // The desktop, then the hotkey.
    let mut cursor = (470.0, 560.0);
    let mut p = desk.clone();
    pointer(&mut p, cursor);
    film.frame(p.data(), 900);

    let full = Rect::new(0, 0, W as i32, H as i32);
    let monitor = Monitor {
        rect: full,
        work: Rect::new(0, 0, W as i32, H as i32 - TASKBAR as i32),
        scale: 1.0,
    };
    let at = |(x, y): (f32, f32)| Point::new(x as i32, y as i32);
    let windows = vec![spots.window_b, spots.window_a];
    let mut overlay = Overlay::new(Selection::new(vec![monitor], windows, 4, at(cursor)), true);
    let frame = BgrxView {
        width: W,
        height: H,
        stride: W as usize * 4,
        data: desk.data(),
    };
    let mut canvas = vec![0u8; desk.data().len()];
    let mut shoot = |overlay: &mut Overlay, film: &mut Film, ms: u32| {
        let mut out = BgrxMut {
            width: W,
            height: H,
            stride: W as usize * 4,
            data: &mut canvas,
        };
        overlay.render(0, &frame, &mut out, full);
        film.frame(&canvas, ms);
    };
    film.scene = "overlay";
    shoot(&mut overlay, &mut film, 500);

    // Hover over both windows, then drag an area over the report.
    for (to, n, hold) in [((800.0, 330.0), 9, 450), ((330.0, 250.0), 10, 450)] {
        for (i, c) in path(cursor, to, n).into_iter().enumerate() {
            overlay.handle(Event::Move(at(c)));
            shoot(
                &mut overlay,
                &mut film,
                if i + 1 == n { hold } else { STEP },
            );
        }
        cursor = to;
    }
    let (from, to) = ((182.0, 129.0), (612.0, 432.0));
    for (i, c) in path(cursor, from, 6).into_iter().enumerate() {
        overlay.handle(Event::Move(at(c)));
        shoot(&mut overlay, &mut film, if i == 5 { 200 } else { STEP });
    }
    overlay.handle(Event::Press(at(from)));
    for (i, c) in path(from, to, 16).into_iter().enumerate() {
        overlay.handle(Event::Move(at(c)));
        shoot(&mut overlay, &mut film, if i == 15 { 600 } else { STEP });
    }
    let capture = match overlay.handle(Event::Release(at(to))).outcome {
        Some(Outcome::Capture(target)) => target.rect,
        other => panic!("the drag captured {other:?}"),
    };
    cursor = to;

    // The preview card in the corner; the pointer goes to it.
    let image = crop(&desk, capture);
    let view = BgrxView {
        width: image.width(),
        height: image.height(),
        stride: image.width() as usize * 4,
        data: image.data(),
    };
    let palette = Palette::of(false);
    let card = Card::new(
        &view,
        1.0,
        Status::CopiedAndSaved,
        Some("Открыть в редакторе"),
    )
    .unwrap();
    let g = &card.geometry;
    let card_at = (
        W as i32 - g.width - 16,
        H as i32 - TASKBAR as i32 - g.height - 16,
    );
    let (shadow, offset) = card.render_shadow(palette).unwrap();
    let with_card = |hover: bool| {
        let mut p = desk.clone();
        let flat = (PixmapPaint::default(), Transform::identity());
        let (sx, sy) = (card_at.0 - offset.x, card_at.1 - offset.y);
        p.draw_pixmap(sx, sy, shadow.as_ref(), &flat.0, flat.1, None);
        let face = card.render(palette, hover).unwrap();
        p.draw_pixmap(card_at.0, card_at.1, face.as_ref(), &flat.0, flat.1, None);
        p
    };
    film.scene = "card";
    let mut p = with_card(false);
    pointer(&mut p, cursor);
    film.frame(p.data(), 700);
    let target = (
        (card_at.0 + g.width / 2) as f32,
        (card_at.1 + g.height / 2) as f32,
    );
    for (i, c) in path(cursor, target, 12).into_iter().enumerate() {
        let inside = c.0 >= card_at.0 as f32 && c.1 >= card_at.1 as f32;
        let mut p = with_card(inside);
        pointer(&mut p, c);
        film.frame(p.data(), if i == 11 { 900 } else { STEP });
    }

    // A click opens the editor; mark the chart up.
    let size = (880, 440);
    let open = Open {
        image: image.clone(),
        image_scale: 1.0,
        name: "Shot 2026-10-22 17-03-15.png".into(),
        has_file: true,
        window: size,
        scale: 1.0,
        dark: false,
        destination: Destination::Both,
        os: Os::Windows,
    };
    let now = Instant::now();
    let mut editor = Editor::new(open, now);
    editor.take_damage();
    let fit = |room: u32, len: i32| ((room as i32 - len) / 2) as f32;
    let image_at = (
        24.0 + fit(size.0 - 48, capture.width),
        68.0 + fit(size.1 - 92, capture.height),
    );
    let mut ed = EditorShot {
        editor,
        at: (72.0, 66.0),
        size,
        image: image_at,
        now,
    };
    let mut cursor = (440.0, 260.0);
    film.scene = "editor";
    film.frame(ed.compose(&desk, cursor, false).data(), 700);

    let drag = |ed: &mut EditorShot, film: &mut Film, cursor: &mut (f32, f32), a, b, hold| {
        for c in path(*cursor, a, 6) {
            ed.handle(Input::Move {
                x: c.0,
                y: c.1,
                shift: false,
            });
            film.frame(ed.compose(&desk, c, true).data(), STEP);
        }
        ed.handle(Input::Down {
            x: a.0,
            y: a.1,
            shift: false,
        });
        for c in path(a, b, 10) {
            ed.handle(Input::Move {
                x: c.0,
                y: c.1,
                shift: false,
            });
            film.frame(ed.compose(&desk, c, true).data(), STEP);
        }
        ed.handle(Input::Up {
            x: b.0,
            y: b.1,
            shift: false,
        });
        film.frame(ed.compose(&desk, b, true).data(), hold);
        *cursor = b;
    };
    let key = |ed: &mut EditorShot, c: char| {
        ed.handle(Input::Key {
            key: Key::Char(c),
            ctrl: false,
            shift: false,
        });
        ed.handle(Input::KeyUp {
            key: Key::Char(c),
            shift: false,
        });
    };

    key(&mut ed, 'a');
    let bar = spots.tallest_bar;
    let (a, b) = (
        ed.to_window(capture, (bar.0 - 150.0, bar.1 - 30.0)),
        ed.to_window(capture, (bar.0 - 10.0, bar.1 - 6.0)),
    );
    drag(&mut ed, &mut film, &mut cursor, a, b, 500);

    key(&mut ed, 'r');
    let (rx, ry, rw, rh) = spots.revenue;
    let (a, b) = (
        ed.to_window(capture, (rx - 6.0, ry - 6.0)),
        ed.to_window(capture, (rx + rw + 6.0, ry + rh + 6.0)),
    );
    drag(&mut ed, &mut film, &mut cursor, a, b, 500);

    key(&mut ed, 'n');
    for spot in [spots.orders, spots.returns] {
        let c = ed.to_window(capture, spot);
        for m in path(cursor, c, 8) {
            ed.handle(Input::Move {
                x: m.0,
                y: m.1,
                shift: false,
            });
            film.frame(ed.compose(&desk, m, true).data(), STEP);
        }
        ed.handle(Input::Down {
            x: c.0,
            y: c.1,
            shift: false,
        });
        ed.handle(Input::Up {
            x: c.0,
            y: c.1,
            shift: false,
        });
        film.frame(ed.compose(&desk, c, true).data(), 400);
        cursor = c;
    }
    let rest = (150.0, 380.0);
    for m in path(cursor, rest, 8) {
        ed.handle(Input::Move {
            x: m.0,
            y: m.1,
            shift: false,
        });
        film.frame(ed.compose(&desk, m, false).data(), STEP);
    }
    film.frame(ed.compose(&desk, rest, false).data(), 2600);

    fs::write(dir.join("frames.txt"), &film.list).expect("write frames.txt");
    println!("{} frames in {}", film.list.lines().count(), dir.display());
}
