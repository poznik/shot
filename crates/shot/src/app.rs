//! The app, the same on Windows and macOS: the hotkey or the tray icon freezes every monitor under
//! the overlay; the chosen window, area or monitors go to the clipboard and to PNG files written
//! in the background, as `settings.toml` says, and a preview card shows in the corner. A click on
//! the card opens the capture in the editor. Each OS has its own event loop (`win_main`,
//! `mac_main`) that calls in here.

use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use shot_core::geom::{Point, Rect};
use shot_core::hotkey::{Hotkey, Os};
use shot_core::image::{BgrxView, Compression, encode_png};
use shot_core::naming::{LocalTime, save_captures};
use shot_core::overlay::{self, Event, Outcome, Overlay, Selection};
use shot_core::preview::{Card, Status};
use shot_core::settings::{HINT_CAPTURES, Settings, ThemeChoice};
use shot_core::tray::{self, Command};
use shot_core::ui::settings::{Request, Values};
use shot_platform::sys::{
    self, Bitmap, Clipboard, FolderWatch, InstanceLock, OverlayWindows, Tray,
};

use crate::editor::{self, EditorUi, Source};
use crate::folder_pick::FolderPick;
use crate::log::log;
use crate::preview::Previews;
use crate::settings_ui::{PermissionUi, SettingsUi, WelcomeUi};

pub const HOTKEY_CAPTURE: i32 = 1;
#[cfg(windows)]
pub const OS: Os = Os::Windows;
#[cfg(target_os = "macos")]
pub const OS: Os = Os::MacOs;
/// The density of a monitor at scale 1, which PNG files are tagged with times the monitor's scale.
#[cfg(windows)]
pub const BASE_DPI: f32 = 96.0;
#[cfg(target_os = "macos")]
pub const BASE_DPI: f32 = 72.0;

pub struct App {
    /// Shared with the editors.
    clipboard: Rc<Clipboard>,
    pub windows: OverlayWindows,
    pub tray: Tray,
    pub previews: Previews,
    /// Editor windows that may be open.
    editors: Vec<EditorUi>,
    /// `Выход` waits for editors that ask whether to save their edits.
    quitting: bool,
    settings: Settings,
    settings_path: PathBuf,
    /// The file's text as Shot last read or wrote it: a change notification with the same text
    /// is Shot's own write.
    settings_text: String,
    /// `Pictures/Shot`, used when the settings name no folder.
    default_dir: PathBuf,
    /// The registered hotkey; `None` when the OS refused it, or while the settings window records
    /// a new one.
    hotkey: Option<Hotkey>,
    settings_ui: Option<SettingsUi>,
    welcome: Option<WelcomeUi>,
    /// The Screen Recording window of macOS.
    permission_ui: Option<PermissionUi>,
    /// Shot may capture the screen: always on Windows, with the Screen Recording permission on
    /// macOS. Checked at start, when Shot becomes active and when a capture fails.
    permission: bool,
    folder_pick: Option<FolderPick>,
    /// `SHOT_PERF` is set: wait for the compositor after showing the overlay, to log when it is
    /// on screen.
    perf: bool,
    /// Held while Shot runs: a second launch finds it and wakes this one instead.
    _instance: InstanceLock,
}

impl App {
    /// Opens the log, reads the settings and sets up the windows. `None` when another Shot is
    /// running or Shot cannot run here (logged). The watch reports changes to `settings.toml`.
    pub fn start() -> Option<(Self, Option<FolderWatch>)> {
        if let Some(dir) = sys::log_dir() {
            crate::log::init(&dir.join("shot.log"));
        }
        let Some(instance) = sys::claim_instance() else {
            log!("already running: asked it to open the settings");
            return None;
        };
        let default_dir = sys::pictures_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Shot");
        let settings_path = sys::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("settings.toml");
        let (settings, problems) = Settings::load(&settings_path, OS);
        for p in problems {
            log!("settings: {p}");
        }
        let settings_text = std::fs::read_to_string(&settings_path).unwrap_or_default();

        let clipboard = Clipboard::new().map_err(|e| log!("clipboard: {e}")).ok()?;
        let rects: Vec<Rect> = sys::monitors().iter().map(|m| m.rect).collect();
        let windows = OverlayWindows::new(&rects)
            .map_err(|e| log!("overlay windows: {e}"))
            .ok()?;
        let tray = Tray::new("Shot", false)
            .map_err(|e| log!("tray: {e}"))
            .ok()?;
        // A capture of all monitors shows a card per monitor.
        let previews = Previews::new(dark(settings.theme), sys::monitors().len())
            .map_err(|e| log!("preview windows: {e}"))
            .ok()?;
        let mut app = App {
            clipboard: Rc::new(clipboard),
            windows,
            tray,
            previews,
            editors: Vec::new(),
            quitting: false,
            settings,
            settings_path,
            settings_text,
            default_dir,
            hotkey: None,
            settings_ui: None,
            welcome: None,
            permission_ui: None,
            permission: sys::screen_permission(),
            folder_pick: None,
            perf: std::env::var_os("SHOT_PERF").is_some(),
            _instance: instance,
        };
        if app.settings_text.is_empty() {
            // The first run: write the defaults, so there is a file to edit.
            app.save_settings();
        }
        app.apply_theme();
        app.register_hotkey();
        log!(
            "ready in {:.1} ms, saving to {}",
            ms(sys::since_process_start()),
            app.out_dir().display()
        );
        let watch = app
            .settings_path
            .parent()
            .and_then(|dir| FolderWatch::new(dir).ok());

        // On macOS without the permission its window comes first, and the welcome after it.
        if !app.permission {
            log!("no Screen Recording permission");
            app.open_permission();
        } else if !app.settings.welcomed {
            app.open_welcome();
        }
        Some((app, watch))
    }
}

impl App {
    fn out_dir(&self) -> PathBuf {
        self.settings
            .folder
            .clone()
            .unwrap_or_else(|| self.default_dir.clone())
    }

    /// Registers the hotkey from the settings, unless it is the one already registered, and shows
    /// in the tray whether it works.
    fn register_hotkey(&mut self) {
        let wanted = self.settings.hotkey;
        if self.hotkey != Some(wanted) {
            if self.hotkey.take().is_some() {
                sys::unregister_hotkey(HOTKEY_CAPTURE);
            }
            match sys::register_hotkey(HOTKEY_CAPTURE, &wanted) {
                Ok(()) => {
                    self.hotkey = Some(wanted);
                    log!("hotkey: {}", wanted.display(OS));
                }
                Err(e) => log!("{} is taken by another program: {e}", wanted.display(OS)),
            }
        }
        let state = self.tray_state(false);
        self.tray.set(&tray::tooltip(&state, OS), state.alert());
    }

    /// What the tray shows. Reading `autostart` costs a registry read: only the menu needs it.
    fn tray_state(&self, autostart: bool) -> tray::State {
        tray::State {
            hotkey: self.settings.hotkey,
            hotkey_works: self.hotkey.is_some(),
            permission: self.permission,
            autostart: autostart && sys::autostart(),
        }
    }

    fn apply_theme(&mut self) {
        sys::set_menu_theme(match self.settings.theme {
            ThemeChoice::System => None,
            ThemeChoice::Light => Some(false),
            ThemeChoice::Dark => Some(true),
        });
        let dark = dark(self.settings.theme);
        self.previews.set_dark(dark);
        let follow_system = self.settings.theme == ThemeChoice::System;
        self.editors.retain(EditorUi::is_open);
        for editor in &self.editors {
            editor.window().set_theme(dark, follow_system);
        }
        let forms = [
            self.settings_ui.as_ref().map(SettingsUi::window),
            self.welcome.as_ref().map(WelcomeUi::window),
            self.permission_ui.as_ref().map(PermissionUi::window),
        ];
        for form in forms.into_iter().flatten() {
            form.set_theme(dark, follow_system);
        }
    }

    /// Opens the captures whose cards were clicked.
    pub fn open_editors(&mut self) {
        for (source, clicked) in self.previews.take_open() {
            let context = editor::Context {
                clipboard: self.clipboard.clone(),
                out_dir: self.out_dir(),
                destination: self.settings.destination,
                dark: dark(self.settings.theme),
                follow_system: self.settings.theme == ThemeChoice::System,
            };
            if let Some(editor) = editor::open(source, context, clicked) {
                self.editors.retain(EditorUi::is_open);
                self.editors.push(editor);
            }
        }
    }

    /// Shows the card of a capture in the corner of `corner`, the work area of the monitor where
    /// the cursor was at the hotkey. Returns the key its save reports with; `None` when there is
    /// no card: previews are off and nothing is saved.
    fn preview(
        &mut self,
        image: &BgrxView,
        corner: Corner,
        copied: bool,
        saved: bool,
    ) -> Option<u64> {
        if !self.settings.preview && !saved {
            return None;
        }
        let status = match (copied, saved) {
            (true, true) => Status::CopiedAndSaved,
            (true, false) => Status::Copied,
            (false, _) => Status::Saved {
                folder: folder_label(&self.out_dir()),
            },
        };
        let card = Card::new(image, corner.scale, status, Some("Открыть в редакторе"))?;
        let timeout = Duration::from_secs(self.settings.preview_seconds as u64);
        Some(
            self.previews
                .add(card, corner.work, timeout, self.settings.preview),
        )
    }

    /// Shows the tray menu and runs the chosen command.
    pub fn menu(&mut self, at: Point) {
        let items = tray::menu(&self.tray_state(true), OS);
        match self.tray.menu(&items, at) {
            Some(Command::FixHotkey | Command::Settings) => self.open_settings(),
            Some(Command::FixPermission) => self.open_permission(),
            Some(Command::OpenFolder) => {
                let dir = self.out_dir();
                let opened = std::fs::create_dir_all(&dir)
                    .map_err(|e| e.to_string())
                    .and_then(|()| sys::open_path(&dir).map_err(|e| e.to_string()));
                if let Err(e) = opened {
                    log!("open {}: {e}", dir.display());
                }
            }
            Some(Command::Autostart) => {
                set_autostart(!sys::autostart());
                self.show_values();
            }
            Some(Command::Quit) => self.quit(),
            // `Снять` arrives as `TrayEvent::Capture` once the menu is gone.
            Some(Command::Capture) | None => {}
        }
    }

    /// Quits once the editors are closed: an editor with edits asks first, and its `Отмена`
    /// keeps Shot running.
    fn quit(&mut self) {
        self.editors.retain(EditorUi::is_open);
        for editor in &self.editors {
            editor.window().ask_close();
        }
        self.editors.retain(EditorUi::is_open);
        if self.editors.is_empty() {
            return sys::quit();
        }
        log!("quit: {} editor(s) ask to save", self.editors.len());
        self.quitting = true;
    }

    /// Opens the settings window, or brings it to the front.
    pub fn open_settings(&mut self) {
        if let Some(ui) = &self.settings_ui
            && !ui.is_done()
        {
            return ui.window().show();
        }
        self.settings_ui = SettingsUi::open(
            self.values(),
            dark(self.settings.theme),
            self.settings.theme == ThemeChoice::System,
        );
    }

    fn open_welcome(&mut self) {
        self.welcome = WelcomeUi::open(
            self.settings.hotkey,
            dark(self.settings.theme),
            self.settings.theme == ThemeChoice::System,
        );
    }

    /// Opens the Screen Recording window, or brings it to the front.
    pub fn open_permission(&mut self) {
        if let Some(ui) = &self.permission_ui
            && !ui.is_done()
        {
            return ui.window().show();
        }
        self.permission_ui = PermissionUi::open(
            self.permission,
            dark(self.settings.theme),
            self.settings.theme == ThemeChoice::System,
        );
    }

    /// Checks the Screen Recording permission again: after Shot became active, the user may have
    /// changed it in the system settings.
    #[cfg(target_os = "macos")]
    pub fn check_permission(&mut self) {
        // With the permission there and its window closed, nothing waits for a change.
        if self.permission && self.permission_ui.is_none() {
            return;
        }
        self.set_permission(sys::screen_permission());
    }

    /// Shows in the menu bar and the Screen Recording window whether Shot has the permission.
    fn set_permission(&mut self, now: bool) {
        if now != self.permission {
            self.permission = now;
            log!(
                "Screen Recording permission: {}",
                if now { "given" } else { "gone" }
            );
            let state = self.tray_state(false);
            self.tray.set(&tray::tooltip(&state, OS), state.alert());
        }
        if let Some(ui) = &self.permission_ui {
            ui.set_granted(now);
        }
    }

    /// What the settings window shows.
    fn values(&self) -> Values {
        Values {
            hotkey: self.settings.hotkey,
            hotkey_works: self.hotkey == Some(self.settings.hotkey),
            destination: self.settings.destination,
            folder: self.out_dir().display().to_string(),
            preview: self.settings.preview,
            preview_seconds: self.settings.preview_seconds,
            autostart: sys::autostart(),
            theme: self.settings.theme,
        }
    }

    /// Has the settings window show the settings as they are now.
    fn show_values(&self) {
        if let Some(ui) = &self.settings_ui {
            ui.update(self.values());
        }
    }

    /// Carries out what the settings window, the welcome and the Screen Recording window asked for,
    /// and lets go of closed ones.
    pub fn serve_windows(&mut self) {
        if self.quitting {
            self.editors.retain(EditorUi::is_open);
            if self.editors.is_empty() {
                sys::quit();
            } else if !self.editors.iter().any(EditorUi::asking) {
                // `Отмена`, or a save that failed: the edits stay, and so does Shot.
                log!("quit: cancelled in the editor");
                self.quitting = false;
            }
        }
        if let Some(ui) = &self.permission_ui {
            if ui.take_open_settings() {
                sys::ask_screen_permission();
            }
            if ui.is_done() {
                self.permission_ui = None;
                if !self.settings.welcomed && self.welcome.is_none() {
                    self.open_welcome();
                }
            }
        }
        if let Some(welcome) = &self.welcome
            && let Some(finish) = welcome.finished()
        {
            self.welcome = None;
            self.settings.welcomed = true;
            self.save_settings();
            if let Some(finish) = finish {
                if finish.autostart != sys::autostart() {
                    set_autostart(finish.autostart);
                }
                if finish.settings {
                    self.open_settings();
                }
            }
        }
        if let Some(pick) = &self.folder_pick
            && let Some(chosen) = pick.done()
        {
            self.folder_pick = None;
            if let Some(dir) = chosen {
                log!("folder: {}", dir.display());
                self.settings.folder = (dir != self.default_dir).then_some(dir);
                self.save_settings();
                self.show_values();
            }
        }
        let Some(ui) = &self.settings_ui else {
            return;
        };
        let requests = ui.take_requests();
        let done = ui.is_done();
        if requests.is_empty() && !done {
            return;
        }
        for request in requests {
            self.apply(request);
        }
        if done {
            self.settings_ui = None;
        } else {
            self.show_values();
        }
    }

    fn apply(&mut self, request: Request) {
        match request {
            Request::PauseHotkey => {
                if self.hotkey.take().is_some() {
                    sys::unregister_hotkey(HOTKEY_CAPTURE);
                }
                return;
            }
            Request::ResumeHotkey => return self.register_hotkey(),
            Request::SetHotkey(wanted) => return self.set_hotkey(wanted),
            Request::Destination(d) => self.settings.destination = d,
            Request::PickFolder => {
                if self.folder_pick.is_none()
                    && let Some(ui) = &self.settings_ui
                {
                    let notifier = self.previews.notifier();
                    self.folder_pick = FolderPick::start(ui.window(), &self.out_dir(), notifier);
                }
                return;
            }
            Request::Preview(on) => self.settings.preview = on,
            Request::PreviewSeconds(n) => self.settings.preview_seconds = n,
            Request::Autostart(on) => return set_autostart(on),
            Request::Theme(theme) => {
                self.settings.theme = theme;
                self.apply_theme();
            }
        }
        self.save_settings();
    }

    /// Swaps the hotkey for `wanted`; if the OS refuses it, takes the old one back.
    fn set_hotkey(&mut self, wanted: Hotkey) {
        if self.hotkey.take().is_some() {
            sys::unregister_hotkey(HOTKEY_CAPTURE);
        }
        let old = self.settings.hotkey;
        self.settings.hotkey = wanted;
        self.register_hotkey();
        if self.hotkey.is_some() {
            self.save_settings();
            if let Some(ui) = &self.settings_ui {
                ui.hotkey_taken(wanted);
            }
        } else {
            self.settings.hotkey = old;
            self.register_hotkey();
            if let Some(ui) = &self.settings_ui {
                ui.hotkey_refused(wanted, self.hotkey.is_some());
            }
        }
    }

    /// Reads `settings.toml` after it changed on disk, and applies it.
    pub fn reload_settings(&mut self) {
        let Ok(text) = std::fs::read_to_string(&self.settings_path) else {
            return;
        };
        // An empty file is an editor between truncating and writing; the write follows.
        if text == self.settings_text || text.trim().is_empty() {
            return;
        }
        let (settings, problems) = Settings::parse(&text, OS);
        for p in problems {
            log!("settings: {p}");
        }
        log!("settings: reloaded");
        self.settings = settings;
        self.settings_text = text;
        self.apply_theme();
        // While the settings window records a hotkey, the old one stays let go.
        if !self.settings_ui.as_ref().is_some_and(SettingsUi::recording) {
            self.register_hotkey();
        }
        self.show_values();
    }

    fn save_settings(&mut self) {
        match self.settings.save(&self.settings_path, OS) {
            Ok(()) => self.settings_text = self.settings.to_toml(OS),
            Err(e) => log!("settings: {} not saved: {e}", self.settings_path.display()),
        }
    }
}

/// One capture: from the hotkey until the choice.
pub struct Session {
    overlay: Overlay,
    /// The frozen frame of each monitor, in the overlay's monitor order.
    frames: Vec<Option<Bitmap>>,
    time: LocalTime,
    /// Where the preview goes.
    corner: Corner,
}

/// The work area of the monitor where the cursor was at the hotkey, and its scale.
#[derive(Clone, Copy)]
struct Corner {
    work: Rect,
    scale: f32,
}

impl Session {
    /// Freezes the monitor under the cursor and shows its overlay first, then the others
    /// (SPEC §5.4).
    pub fn start(app: &mut App, queued_ms: u32) -> Option<Self> {
        if !app.permission {
            // The hotkey leads to the permission instead.
            app.open_permission();
            return None;
        }
        let t0 = Instant::now();
        let time = sys::local_time();
        let monitors = sys::monitors();
        let (cx, cy) = sys::cursor_pos();
        let cursor = Point::new(cx, cy);
        // The rule `Selection` uses: the monitor under the cursor, else the nearest.
        let first = (0..monitors.len()).min_by_key(|&i| monitors[i].rect.distance_sq(cursor))?;
        let frame = match sys::capture_rects(&[monitors[first].rect], false) {
            Ok(mut frames) => frames.pop()?,
            Err(e) => {
                log!("capture failed: {e}");
                // The permission may have been taken away while Shot ran.
                if !sys::screen_permission() {
                    app.set_permission(false);
                    app.open_permission();
                }
                return None;
            }
        };
        let t_capture = t0.elapsed();

        let window_rects = sys::window_rects();
        let window_count = window_rects.len();
        let selection = Selection::new(
            monitors
                .iter()
                .map(|m| overlay::Monitor {
                    rect: m.rect,
                    work: m.work,
                    scale: m.scale,
                })
                .collect(),
            window_rects,
            sys::drag_threshold(),
            cursor,
        );
        let t_list = t0.elapsed();
        let rects: Vec<Rect> = monitors.iter().map(|m| m.rect).collect();
        if let Err(e) = app.windows.ensure(&rects) {
            log!("overlay windows: {e}");
            return None;
        }
        let mut session = Session {
            overlay: Overlay::new(selection, app.settings.show_key_hint()),
            frames: monitors.iter().map(|_| None).collect(),
            time,
            corner: Corner {
                work: monitors[first].work,
                scale: monitors[first].scale,
            },
        };
        if let Err(e) = session.show(&mut app.windows, first, frame) {
            log!("overlay canvas: {e}");
            app.windows.hide();
            return None;
        }
        let t_shown = t0.elapsed();
        let on_screen = app.perf.then(|| {
            sys::wait_for_compose();
            format!(", on screen {:.1} ms", ms(t0.elapsed()))
        });

        for i in (0..monitors.len()).filter(|&i| i != first) {
            let shown =
                sys::capture_rects(&[monitors[i].rect], false).and_then(|mut frames| match frames
                    .pop()
                {
                    Some(frame) => session.show(&mut app.windows, i, frame),
                    None => Ok(()),
                });
            if let Err(e) = shown {
                log!("overlay on monitor {i}: {e}");
            }
        }
        log!(
            "overlay: queued ~{queued_ms} ms, capture {:.1} ms, window list {:.2} ms ({window_count}), \
             shown {:.1} ms{}; all {} monitors {:.1} ms",
            ms(t_capture),
            ms(t_list - t_capture),
            ms(t_shown),
            on_screen.unwrap_or_default(),
            monitors.len(),
            ms(t0.elapsed()),
        );
        Some(session)
    }

    /// Draws the whole overlay of monitor `i` over its frozen `frame` and shows it.
    fn show(
        &mut self,
        windows: &mut OverlayWindows,
        i: usize,
        frame: Bitmap,
    ) -> Result<(), sys::Error> {
        let mut canvas = Bitmap::new(frame.rect)?;
        let all = Rect::new(0, 0, frame.rect.width, frame.rect.height);
        self.overlay
            .render(i, &frame.view(), &mut canvas.view_mut(), all);
        windows.show(i, canvas);
        self.frames[i] = Some(frame);
        Ok(())
    }

    /// Applies one input and redraws what it changed. Returns the choice once it is made.
    pub fn handle(&mut self, windows: &mut OverlayWindows, event: Event) -> Option<Outcome> {
        let update = self.overlay.handle(event);
        if update.outcome.is_some() {
            return update.outcome;
        }
        for d in &update.damage {
            if let Some(frame) = &self.frames[d.monitor]
                && let Some(canvas) = windows.canvas(d.monitor)
            {
                self.overlay
                    .render(d.monitor, &frame.view(), &mut canvas.view_mut(), d.rect);
                windows.present(d.monitor, d.rect);
            }
        }
        None
    }

    /// Closes the overlay; the choice goes to the clipboard and, in the background, to files, and
    /// its card to the corner.
    pub fn finish(self, app: &mut App, outcome: Outcome) {
        let t0 = Instant::now();
        let cursor_monitor = self.overlay.selection().cursor_monitor();
        let dpi: Vec<f32> = (self.overlay.selection().monitors().iter())
            .map(|m| m.scale * BASE_DPI)
            .collect();
        let destination = app.settings.destination;
        let (time, out_dir, corner) = (self.time, app.out_dir(), self.corner);
        let report = app.previews.reporter();
        let mut frames = self.frames;
        match outcome {
            Outcome::Cancel => {
                app.windows.hide();
                log!("overlay: cancelled");
                return;
            }
            Outcome::Capture(target) => {
                let Some(frame) = frames[target.monitor].take() else {
                    return log!("no frame of monitor {}", target.monitor);
                };
                let local = target.rect.offset(-frame.rect.x, -frame.rect.y);
                let image = frame.view().crop(local);
                let hidden_first = (!CARD_FIRST).then(|| hide(app, t0));
                let key = app.preview(&image, corner, destination.clipboard(), destination.file());
                let t_preview = t0.elapsed();
                let t_hidden = hidden_first.unwrap_or_else(|| hide(app, t0));
                if destination.clipboard() {
                    copy(&app.clipboard, image, dpi[target.monitor]);
                }
                log!(
                    "{:?} {}x{}: preview {:.1} ms, hidden {:.1} ms, clipboard {:.1} ms{}",
                    target.kind,
                    local.width,
                    local.height,
                    ms(t_preview),
                    ms(t_hidden),
                    ms(t0.elapsed()),
                    on_screen(app, t0),
                );
                let dpi = dpi[target.monitor];
                if destination.file() {
                    thread::spawn(move || {
                        let results = save(&out_dir, time, &[(frame.view().crop(local), dpi)]);
                        for (key, result) in [key].into_iter().zip(results) {
                            if let Some(key) = key {
                                report.saved(key, result);
                            }
                        }
                    });
                } else if let Some(key) = key {
                    thread::spawn(move || {
                        if let Some(source) = keep(frame.view().crop(local), dpi, time) {
                            report.kept(key, source);
                        }
                    });
                }
            }
            Outcome::AllMonitors => {
                let frames: Vec<(usize, Bitmap)> = frames
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, f)| Some((i, f?)))
                    .collect();
                let copied = |i: usize| destination.clipboard() && i == cursor_monitor;
                let hidden_first = (!CARD_FIRST).then(|| hide(app, t0));
                let keys: Vec<Option<u64>> = frames
                    .iter()
                    .filter(|(i, _)| destination.file() || copied(*i))
                    .map(|(i, frame)| {
                        app.preview(&frame.view(), corner, copied(*i), destination.file())
                    })
                    .collect();
                let t_preview = t0.elapsed();
                let t_hidden = hidden_first.unwrap_or_else(|| hide(app, t0));
                if let Some((i, frame)) = frames.iter().find(|(i, _)| copied(*i)) {
                    copy(&app.clipboard, frame.view(), dpi[*i]);
                }
                log!(
                    "all {} monitors: preview {:.1} ms, hidden {:.1} ms, clipboard {:.1} ms{}",
                    frames.len(),
                    ms(t_preview),
                    ms(t_hidden),
                    ms(t0.elapsed()),
                    on_screen(app, t0),
                );
                if destination.file() {
                    thread::spawn(move || {
                        let views: Vec<(BgrxView, f32)> =
                            frames.iter().map(|(i, f)| (f.view(), dpi[*i])).collect();
                        let results = save(&out_dir, time, &views);
                        for (key, result) in keys.into_iter().zip(results) {
                            if let Some(key) = key {
                                report.saved(key, result);
                            }
                        }
                    });
                } else if let Some(Some(key)) = keys.first().copied()
                    && let Some((i, frame)) = frames.into_iter().find(|(i, _)| copied(*i))
                {
                    let dpi = dpi[i];
                    thread::spawn(move || {
                        if let Some(source) = keep(frame.view(), dpi, time) {
                            report.kept(key, source);
                        }
                    });
                }
            }
        }
        if app.settings.captures < HINT_CAPTURES {
            app.settings.captures += 1;
            app.save_settings();
        }
    }
}

/// Whether the card goes up before the overlay comes down. On Windows it does: a layered window
/// shown while DWM recomposes the monitors the overlay left waits for it, 10–18 ms (WIN-004). On
/// macOS it does not: the overlay hidden after a card waits for the window server to take the
/// card, 11–18 ms, and hidden first it is gone in 3–6 ms (MAC-005).
const CARD_FIRST: bool = cfg!(windows);

/// Hides the overlay; returns when that was done, from `t0`.
fn hide(app: &mut App, t0: Instant) -> Duration {
    app.windows.hide();
    t0.elapsed()
}

/// With `SHOT_PERF`: waits for the compositor, to log when the card and the overlay's absence
/// are on screen.
fn on_screen(app: &App, t0: Instant) -> String {
    if !app.perf {
        return String::new();
    }
    sys::wait_for_compose();
    format!(", on screen {:.1} ms", ms(t0.elapsed()))
}

fn set_autostart(on: bool) {
    match sys::set_autostart(on) {
        Ok(()) => log!("autostart {}", if on { "on" } else { "off" }),
        Err(e) => log!("autostart: {e}"),
    }
}

/// The theme of the windows Shot draws.
fn dark(theme: ThemeChoice) -> bool {
    match theme {
        ThemeChoice::System => sys::apps_use_dark_theme(),
        ThemeChoice::Light => false,
        ThemeChoice::Dark => true,
    }
}

/// The folder as the user knows it: `Pictures\Shot` (`Pictures/Shot`) for a folder in the user's
/// profile.
fn folder_label(dir: &Path) -> String {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .and_then(|home| dir.strip_prefix(home).ok().map(Path::to_path_buf))
        .filter(|rel| !rel.as_os_str().is_empty())
        .unwrap_or_else(|| dir.to_path_buf())
        .display()
        .to_string()
}

fn copy(clipboard: &Clipboard, image: BgrxView, dpi: f32) {
    if let Err(e) = clipboard.set_image(image, dpi) {
        log!("clipboard failed: {e}");
    }
}

/// A capture that went only to the clipboard, as PNG in memory: its card opens it in the editor.
/// A few MB at most, while the frame it comes from is 8–33 MB.
fn keep(image: BgrxView, dpi: f32, time: LocalTime) -> Option<Source> {
    let t0 = Instant::now();
    let mut png = Vec::new();
    match encode_png(image, &mut png, Compression::Fast, Some(dpi)) {
        Ok(()) => {
            png.shrink_to_fit();
            log!(
                "kept {}x{} for the editor: {} KB in {:.1} ms",
                image.width,
                image.height,
                png.len() / 1024,
                ms(t0.elapsed())
            );
            Some(Source::Memory { png, time })
        }
        Err(e) => {
            log!("keeping the capture failed: {e}");
            None
        }
    }
}

/// Writes the images as PNG files and logs each one.
fn save(dir: &Path, time: LocalTime, images: &[(BgrxView, f32)]) -> Vec<io::Result<PathBuf>> {
    let t0 = Instant::now();
    let results = save_captures(dir, time, images);
    for (result, (image, _)) in results.iter().zip(images) {
        match result {
            Ok(path) => {
                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                log!(
                    "saved {} ({}x{}, {} KB) at {:.1} ms",
                    path.display(),
                    image.width,
                    image.height,
                    size / 1024,
                    ms(t0.elapsed())
                );
            }
            Err(e) => log!("save to {} failed: {e}", dir.display()),
        }
    }
    results
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
