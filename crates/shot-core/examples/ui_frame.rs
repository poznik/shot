//! The settings window, the welcome and the Screen Recording window: how long the first frame and
//! a redraw take, and, with a folder, what they look like.
//!
//! `cargo run -p shot-core --release --example ui_frame [folder]`
//!
//! Prints min / median / max in ms for the model made plus its first frame, and for a redraw after
//! hovering a control. With `folder`, writes `settings-<scale>-<theme>.png`,
//! `welcome-<scale>-<theme>.png` and `permission-<scale>-<theme>.png` there, the settings with an
//! error line under the hotkey, the Screen Recording window without the permission and, at
//! scale 2, with it; and `settings-mac-<theme>.png` and `welcome-mac-<theme>.png`, as on macOS at
//! scale 2.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::time::{Duration, Instant};

use shot_core::hotkey::{Hotkey, Os};
use shot_core::image::{BgrxMut, BgrxView, Compression, encode_png};
use shot_core::preview::Side;
use shot_core::settings::{Destination, ThemeChoice};
use shot_core::ui::settings::Values;
use shot_core::ui::{Input, Permission, SettingsWindow, Welcome};

const RUNS: usize = 50;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn report(name: &str, mut samples: Vec<Duration>) {
    samples.sort();
    println!(
        "{name:<28} min {:6.2}  med {:6.2}  max {:6.2}  (n={})",
        ms(samples[0]),
        ms(samples[samples.len() / 2]),
        ms(samples[samples.len() - 1]),
        samples.len()
    );
}

fn values(os: Os) -> Values {
    Values {
        hotkey: Hotkey::default_for(os),
        hotkey_works: true,
        destination: Destination::Both,
        folder: r"C:\Users\Someone\Pictures\Shot".to_string(),
        preview: true,
        preview_seconds: 5,
        preview_side: Side::Left,
        autostart: true,
        theme: ThemeChoice::System,
    }
}

/// A window's client area.
struct Client {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Client {
    fn new((width, height): (u32, u32)) -> Self {
        Client {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    fn canvas(&mut self) -> BgrxMut<'_> {
        BgrxMut {
            width: self.width,
            height: self.height,
            stride: self.width as usize * 4,
            data: &mut self.data,
        }
    }

    fn save(&self, path: &Path, scale: f32) {
        let view = BgrxView {
            width: self.width,
            height: self.height,
            stride: self.width as usize * 4,
            data: &self.data,
        };
        let out = BufWriter::new(File::create(path).expect("create PNG"));
        encode_png(view, out, Compression::Fast, Some(96.0 * scale)).expect("write PNG");
        println!("wrote {}", path.display());
    }
}

fn main() {
    let folder = std::env::args().nth(1);
    let scale = 1.5;

    let mut first = Vec::new();
    let mut redraw = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut w = SettingsWindow::new(values(Os::Windows), "0.1.0", Os::Windows, scale, false);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        first.push(t.elapsed());
        w.event(Input::Move { x: 700.0, y: 200.0 });
        let t = Instant::now();
        w.draw(&mut client.canvas());
        redraw.push(t.elapsed());
    }
    report("settings: open + frame", first);
    report("settings: redraw", redraw);

    let mut first = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut w = Welcome::new(Hotkey::default_for(Os::Windows), Os::Windows, scale, false);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        first.push(t.elapsed());
    }
    report("welcome: open + frame", first);

    let mut first = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut w = Permission::new(false, scale, false);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        first.push(t.elapsed());
    }
    report("permission: open + frame", first);

    let Some(folder) = folder else {
        return;
    };
    let folder = Path::new(&folder);
    for (scale, dark) in [(1.0, false), (1.0, true), (1.5, false), (2.0, true)] {
        let theme = if dark { "dark" } else { "light" };
        let mut w = SettingsWindow::new(values(Os::Windows), "0.1.0", Os::Windows, scale, dark);
        w.hotkey_refused(Hotkey::parse("Ctrl+Alt+S").expect("hotkey"), true);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        client.save(&folder.join(format!("settings-{scale}-{theme}.png")), scale);

        let mut w = Welcome::new(Hotkey::default_for(Os::Windows), Os::Windows, scale, dark);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        client.save(&folder.join(format!("welcome-{scale}-{theme}.png")), scale);

        let mut w = Permission::new(scale == 2.0, scale, dark);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        client.save(
            &folder.join(format!("permission-{scale}-{theme}.png")),
            scale,
        );
    }
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let mut w = SettingsWindow::new(values(Os::MacOs), "0.1.0", Os::MacOs, 2.0, dark);
        w.hotkey_refused(Hotkey::parse("Ctrl+Opt+Cmd+S").expect("hotkey"), true);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        client.save(&folder.join(format!("settings-mac-{theme}.png")), 2.0);

        let mut w = Welcome::new(Hotkey::default_for(Os::MacOs), Os::MacOs, 2.0, dark);
        let mut client = Client::new(w.content_size());
        w.draw(&mut client.canvas());
        client.save(&folder.join(format!("welcome-mac-{theme}.png")), 2.0);
    }
}
