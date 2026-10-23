//! Overlay frame times on a 4K monitor (SPEC §2: a frame while the mouse moves, target 8 ms).
//!
//! `cargo run -p shot-core --release --example overlay_frame [scale]`
//!
//! Prints min / median / p95 / max in ms for: the first frame (sprites drawn, every pixel
//! dimmed), a full redraw, and one input with its damage redrawn while dragging an area,
//! hovering inside a window and moving between windows.

use std::time::{Duration, Instant};

use shot_core::geom::{Point, Rect};
use shot_core::image::{BgrxMut, BgrxView};
use shot_core::overlay::{Event, Monitor, Overlay, Selection};

const WIDTH: i32 = 3840;
const HEIGHT: i32 = 2160;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn report(name: &str, mut samples: Vec<Duration>, pixels: Option<Vec<i64>>) {
    samples.sort();
    let at = |q: f64| ms(samples[((samples.len() - 1) as f64 * q).round() as usize]);
    let mut line = format!(
        "{name:<34} min {:6.2}  med {:6.2}  p95 {:6.2}  max {:6.2}  (n={})",
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

struct Screen {
    frame: Vec<u8>,
    canvas: Vec<u8>,
}

impl Screen {
    fn render(&mut self, overlay: &mut Overlay, clip: Rect) {
        let stride = WIDTH as usize * 4;
        let frame = BgrxView {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            stride,
            data: &self.frame,
        };
        let mut canvas = BgrxMut {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            stride,
            data: &mut self.canvas,
        };
        overlay.render(0, &frame, &mut canvas, clip);
    }

    /// Applies one input and redraws what it damaged; returns the damaged pixel count.
    fn step(&mut self, overlay: &mut Overlay, event: Event) -> i64 {
        let update = overlay.handle(event);
        for d in &update.damage {
            self.render(overlay, d.rect);
        }
        update.damage.iter().map(|d| d.rect.area()).sum()
    }
}

fn selection(scale: f32, windows: &[Rect], cursor: Point) -> Selection {
    let rect = Rect::new(0, 0, WIDTH, HEIGHT);
    let monitor = Monitor {
        rect,
        work: Rect::new(0, 0, WIDTH, HEIGHT - (48.0 * scale) as i32),
        scale,
    };
    Selection::new(vec![monitor], windows.to_vec(), 4, cursor)
}

fn timed_steps(
    screen: &mut Screen,
    overlay: &mut Overlay,
    events: impl Iterator<Item = Event>,
) -> (Vec<Duration>, Vec<i64>) {
    let (mut times, mut pixels) = (Vec::new(), Vec::new());
    for event in events {
        let t = Instant::now();
        pixels.push(screen.step(overlay, event));
        times.push(t.elapsed());
    }
    (times, pixels)
}

fn main() {
    let scale: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.5);
    println!("{WIDTH}x{HEIGHT}, scale {scale}");

    let mut frame = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            frame.extend([(x / 15) as u8, (y / 9) as u8, ((x + y) / 24) as u8, 0]);
        }
    }
    let mut screen = Screen {
        canvas: vec![0; frame.len()],
        frame,
    };
    let full = Rect::new(0, 0, WIDTH, HEIGHT);
    let windows = [
        Rect::new(200, 150, 1800, 1200),
        Rect::new(2100, 300, 1500, 1500),
    ];

    let mut first = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let mut overlay = Overlay::new(selection(scale, &windows, Point::new(900, 700)), true);
        screen.render(&mut overlay, full);
        first.push(t.elapsed());
    }
    report("first frame (new overlay, full)", first, None);

    let mut overlay = Overlay::new(selection(scale, &windows, Point::new(900, 700)), true);
    screen.render(&mut overlay, full);
    let mut redraw = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        screen.render(&mut overlay, full);
        redraw.push(t.elapsed());
    }
    report("full redraw", redraw, None);

    // Hover inside one window: only the crosshair and the label move.
    let hover = (0..200).map(|i| Event::Move(Point::new(400 + i * 6, 400 + i * 3)));
    let (t, px) = timed_steps(&mut screen, &mut overlay, hover);
    report("hover inside a window, per move", t, Some(px));

    // Hover from one window to the other and back: the whole of both is redrawn.
    let between = (0..40).map(|i| {
        let x = if i % 2 == 0 { 2500 } else { 1000 };
        Event::Move(Point::new(x, 800))
    });
    let (t, px) = timed_steps(&mut screen, &mut overlay, between);
    report("hover between windows, per move", t, Some(px));

    // Drag an area from the top-left to the bottom-right and back.
    screen.step(&mut overlay, Event::Press(Point::new(600, 400)));
    let drag = (0..240).map(|i| {
        let k = if i < 120 { i } else { 240 - i };
        Event::Move(Point::new(620 + k * 25, 420 + k * 13))
    });
    let (t, px) = timed_steps(&mut screen, &mut overlay, drag);
    report("drag an area, per move", t, Some(px));
}
