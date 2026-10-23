//! Reference renders of the overlay, compared pixel by pixel with `tests/overlay/*.png`.
//!
//! After an intended change of the look, regenerate them with
//! `SHOT_BLESS=1 cargo test -p shot-core --test overlay` and review the PNGs before committing.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use shot_core::geom::{Point, Rect};
use shot_core::image::{BgrxMut, BgrxView, Compression, encode_png};
use shot_core::overlay::{Event, Monitor, Overlay, Selection};

/// A desktop: a gradient with flat light windows that have a darker title bar.
fn desktop(width: i32, height: i32, windows: &[Rect]) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let pixel = match windows.iter().find(|w| w.contains(x, y)) {
                Some(w) if y < w.y + 24 => [205, 200, 196, 0],
                Some(_) => [250, 248, 246, 0],
                None => [
                    (160 + 80 * y / height) as u8,
                    (90 + 60 * x / width) as u8,
                    (40 + 40 * y / height) as u8,
                    0,
                ],
            };
            data.extend(pixel);
        }
    }
    data
}

struct Scene {
    name: &'static str,
    width: i32,
    height: i32,
    scale: f32,
    hint: bool,
    events: Vec<Event>,
}

const WINDOWS: [Rect; 2] = [Rect::new(80, 60, 300, 200), Rect::new(260, 140, 320, 220)];

fn render(scene: &Scene) -> Vec<u8> {
    let rect = Rect::new(0, 0, scene.width, scene.height);
    let monitor = Monitor {
        rect,
        work: Rect::new(0, 0, scene.width, scene.height - 40),
        scale: scene.scale,
    };
    let windows = WINDOWS.to_vec();
    let frame = desktop(scene.width, scene.height, &windows);
    let selection = Selection::new(vec![monitor], windows, 4, Point::new(0, 0));
    let mut overlay = Overlay::new(selection, scene.hint);
    for &event in &scene.events {
        overlay.handle(event);
    }

    let stride = scene.width as usize * 4;
    let mut canvas = vec![0u8; frame.len()];
    let view = BgrxView {
        width: scene.width as u32,
        height: scene.height as u32,
        stride,
        data: &frame,
    };
    let mut out = BgrxMut {
        width: scene.width as u32,
        height: scene.height as u32,
        stride,
        data: &mut canvas,
    };
    overlay.render(0, &view, &mut out, rect);
    canvas
}

fn reference_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("overlay")
        .join(format!("{name}.png"))
}

fn save(scene: &Scene, canvas: &[u8], path: &PathBuf) {
    let view = BgrxView {
        width: scene.width as u32,
        height: scene.height as u32,
        stride: scene.width as usize * 4,
        data: canvas,
    };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = BufWriter::new(File::create(path).unwrap());
    encode_png(view, file, Compression::High, None).unwrap();
}

/// RGB bytes of a PNG.
fn load(path: &PathBuf) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut rgb = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut rgb).unwrap();
    rgb
}

fn check(scene: Scene) {
    let canvas = render(&scene);
    let path = reference_path(scene.name);
    if std::env::var_os("SHOT_BLESS").is_some() {
        save(&scene, &canvas, &path);
        return;
    }
    let expected = load(&path);
    let actual = canvas.as_chunks::<4>().0.iter().map(|p| [p[2], p[1], p[0]]);
    let differing = actual
        .zip(expected.as_chunks::<3>().0)
        .filter(|(a, e)| a != *e)
        .count();
    if differing > 0 {
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{}.png", scene.name));
        save(&scene, &canvas, &out);
        panic!(
            "{}: {differing} pixels differ from {}; the render is in {}",
            scene.name,
            path.display(),
            out.display()
        );
    }
}

const fn p(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

#[test]
fn hover_window_with_hint() {
    check(Scene {
        name: "hover-window",
        width: 640,
        height: 400,
        scale: 1.0,
        hint: true,
        events: vec![Event::Move(p(150, 120))],
    });
}

#[test]
fn drag_area_moves_the_hint_up() {
    check(Scene {
        name: "drag-area",
        width: 640,
        height: 400,
        scale: 1.0,
        hint: true,
        events: vec![
            Event::Press(p(120, 90)),
            Event::Move(p(200, 150)),
            Event::Move(p(470, 300)),
        ],
    });
}

#[test]
fn label_flips_at_the_corner_at_150_percent() {
    check(Scene {
        name: "desktop-150",
        width: 960,
        height: 540,
        scale: 1.5,
        hint: true,
        events: vec![Event::Move(p(930, 20))],
    });
}
