//! Editor frame times (SPEC §2: a frame while the mouse moves, target 8 ms) on a 1440p and a 4K
//! window showing a full-screen capture.
//!
//! `cargo run -p shot-core --release --example editor_frame`
//!
//! Prints min / median / p95 / max in ms for: opening (the editor made and the whole window
//! drawn), a full redraw, and one input with its damage redrawn: drawing an arrow, moving a
//! rectangle, hovering the toolbar, zooming and scrolling. A part of the window is drawn as the
//! platform draws it: into a canvas with a margin of `EDGE`, in memory kept between frames.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use shot_core::editor::{EDGE, Editor, Input, Key, Open};
use shot_core::geom::Rect;
use shot_core::hotkey::Os;
use shot_core::settings::Destination;
use tiny_skia::{IntSize, Pixmap};

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn report(name: &str, mut samples: Vec<Duration>, pixels: Option<Vec<i64>>) {
    samples.sort();
    let at = |q: f64| ms(samples[((samples.len() - 1) as f64 * q).round() as usize]);
    let mut line = format!(
        "  {name:<32} min {:6.2}  med {:6.2}  p95 {:6.2}  max {:6.2}  (n={})",
        at(0.0),
        at(0.5),
        at(0.95),
        at(1.0),
        samples.len()
    );
    if let Some(mut px) = pixels {
        px.sort();
        let mpx = |v: i64| v as f64 / 1e6;
        line += &format!(
            "  damage med {:.3} Mpx, max {:.3}",
            mpx(px[px.len() / 2]),
            mpx(px[px.len() - 1])
        );
    }
    println!("{line}");
}

/// A capture-like image: gradients and some edges.
fn capture(w: u32, h: u32) -> Pixmap {
    let mut p = Pixmap::new(w, h).unwrap();
    for (i, c) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        *c = [(x / 15) as u8, (y / 9) as u8, ((x + y) / 24) as u8, 255];
    }
    p
}

thread_local! {
    /// The platform's canvas memory, kept between frames: fresh memory is zeroed by the OS on
    /// first touch, which would add 5 ms to a 4K frame.
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Draws `region` of the window as the platform does; returns its pixel count.
fn draw(e: &Editor, region: Rect) -> i64 {
    let canvas = region.inflate(EDGE);
    SCRATCH.with_borrow_mut(|scratch| {
        let mut data = std::mem::take(scratch);
        data.resize(canvas.area() as usize * 4, 0);
        let size = IntSize::from_wh(canvas.width as u32, canvas.height as u32).unwrap();
        let mut out = Pixmap::from_vec(data, size).unwrap();
        e.render(&mut out, canvas);
        *scratch = std::hint::black_box(out).take();
    });
    region.area()
}

/// Applies one input and redraws its damage; returns the damaged pixel count.
fn step(e: &mut Editor, input: Input, now: Instant) -> i64 {
    e.handle(input, now);
    let damage = e.take_damage();
    damage.into_iter().map(|r| draw(e, r)).sum()
}

fn timed(
    e: &mut Editor,
    inputs: impl IntoIterator<Item = Input>,
    now: Instant,
) -> (Vec<Duration>, Vec<i64>) {
    let (mut times, mut pixels) = (Vec::new(), Vec::new());
    for input in inputs {
        let t = Instant::now();
        pixels.push(step(e, input, now));
        times.push(t.elapsed());
    }
    (times, pixels)
}

fn mv(x: f32, y: f32) -> Input {
    Input::Move { x, y, shift: false }
}

fn run(name: &str, window: (u32, u32), scale: f32) {
    let (w, h) = window;
    println!("{name}: window {w}x{h}, scale {scale}, image {w}x{h}");
    let image = capture(w, h);
    let open = || Open {
        image: image.clone(),
        image_scale: scale,
        name: "Shot 2026-10-22 17-03-15.png".into(),
        has_file: true,
        window,
        scale,
        dark: false,
        destination: Destination::Both,
        os: Os::Windows,
    };
    let now = Instant::now();
    let whole = Rect::new(0, 0, w as i32, h as i32);

    let mut first = Vec::new();
    for _ in 0..10 {
        let o = open();
        let t = Instant::now();
        let mut e = Editor::new(o, now);
        e.take_damage();
        draw(&e, whole);
        first.push(t.elapsed());
    }
    report("open: new editor, full frame", first, None);

    let mut e = Editor::new(open(), now);
    e.take_damage();
    let mut full = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        draw(&e, whole);
        full.push(t.elapsed());
    }
    report("full redraw", full, None);

    let (fw, fh) = (w as f32, h as f32);
    // Hover over the image with nothing under the cursor.
    let hover = (0..100).map(|i| mv(fw * 0.3 + i as f32 * 3.0, fh * 0.5));
    let (t, px) = timed(&mut e, hover, now);
    report("hover over the image", t, Some(px));

    // Draw an arrow: press, then 200 moves.
    let (x0, y0) = (fw * 0.25, fh * 0.3);
    step(
        &mut e,
        Input::Down {
            x: x0,
            y: y0,
            shift: false,
        },
        now,
    );
    let arrow = (1..=200).map(|i| mv(x0 + i as f32 * fw * 0.002, y0 + i as f32 * fh * 0.0015));
    let (t, px) = timed(&mut e, arrow, now);
    report("draw an arrow, per move", t, Some(px));
    let (x1, y1) = (x0 + 200.0 * fw * 0.002, y0 + 200.0 * fh * 0.0015);
    step(
        &mut e,
        Input::Up {
            x: x1,
            y: y1,
            shift: false,
        },
        now,
    );

    // A rectangle, then move it by its edge with the select tool.
    step(
        &mut e,
        Input::Key {
            key: Key::Char('r'),
            ctrl: false,
            shift: false,
        },
        now,
    );
    let (rx, ry) = (fw * 0.55, fh * 0.45);
    step(
        &mut e,
        Input::Down {
            x: rx,
            y: ry,
            shift: false,
        },
        now,
    );
    step(&mut e, mv(rx + fw * 0.2, ry + fh * 0.2), now);
    step(
        &mut e,
        Input::Up {
            x: rx + fw * 0.2,
            y: ry + fh * 0.2,
            shift: false,
        },
        now,
    );
    step(
        &mut e,
        Input::Key {
            key: Key::Char('v'),
            ctrl: false,
            shift: false,
        },
        now,
    );
    step(&mut e, mv(rx, ry + fh * 0.1), now);
    step(
        &mut e,
        Input::Down {
            x: rx,
            y: ry + fh * 0.1,
            shift: false,
        },
        now,
    );
    let drag = (1..=200).map(|i| mv(rx - i as f32 * 2.0, ry + fh * 0.1 - i as f32));
    let (t, px) = timed(&mut e, drag, now);
    report("move a rectangle, per move", t, Some(px));
    step(
        &mut e,
        Input::Up {
            x: rx - 400.0,
            y: ry + fh * 0.1 - 200.0,
            shift: false,
        },
        now,
    );

    // Hover along the toolbar: buttons light up one after another.
    let left = (w as f32 - 888.0 * scale) / 2.0;
    let toolbar = (0..120).map(|i| mv(left + (i % 60) as f32 * 14.0 * scale, 34.0 * scale));
    let (t, px) = timed(&mut e, toolbar, now);
    report("hover the toolbar, per move", t, Some(px));

    // Zoom in and out around the cursor: the whole window.
    let zoom = (0..20).map(|i| Input::Zoom {
        x: fw * 0.5,
        y: fh * 0.5,
        steps: if i % 2 == 0 { 1 } else { -1 },
    });
    let (t, px) = timed(&mut e, zoom, now);
    report("zoom a step", t, Some(px));

    // At 200%, scroll: the whole window.
    step(
        &mut e,
        Input::Zoom {
            x: fw * 0.5,
            y: fh * 0.5,
            steps: 3,
        },
        now,
    );
    let scroll = (0..40).map(|i| Input::Scroll {
        dx: 0.0,
        dy: if i < 20 { -40.0 } else { 40.0 },
    });
    let (t, px) = timed(&mut e, scroll, now);
    report("scroll at 200%", t, Some(px));
}

fn main() {
    run("1440p", (2560, 1440), 1.25);
    run("4K", (3840, 2160), 1.5);
}
