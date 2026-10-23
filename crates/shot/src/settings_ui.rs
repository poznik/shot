//! The settings window, the first-run welcome and the Screen Recording window of macOS: models
//! from `shot_core::ui` shown in form windows. The window's handler feeds a model its input and draws it; the app, between messages,
//! carries out what the model asks for and tells it what came of it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use shot_core::geom::{Point, Rect};
use shot_core::hotkey::Hotkey;
use shot_core::image::BgrxMut;
use shot_core::ui::settings::{Request, Values};
use shot_core::ui::welcome::Finish;
use shot_core::ui::{Input, Permission, SettingsWindow, Welcome};
use shot_platform::sys::{self, FormEvent, FormHandler, FormRef, FormWindow};

use crate::app::{OS, ms};
use crate::log::log;

const SETTINGS_TITLE: &str = "Настройки Shot";
const WELCOME_TITLE: &str = "Shot";
const PERMISSION_TITLE: &str = "Shot";

/// What the form handler needs of a model.
trait Model {
    fn event(&mut self, input: Input) -> bool;
    fn content_size(&self) -> (u32, u32);
    fn draw(&mut self, dst: &mut BgrxMut);
    fn closed(&self) -> bool;
    fn recording(&self) -> bool;
}

impl Model for SettingsWindow {
    fn event(&mut self, input: Input) -> bool {
        SettingsWindow::event(self, input)
    }
    fn content_size(&self) -> (u32, u32) {
        SettingsWindow::content_size(self)
    }
    fn draw(&mut self, dst: &mut BgrxMut) {
        SettingsWindow::draw(self, dst);
    }
    fn closed(&self) -> bool {
        SettingsWindow::closed(self)
    }
    fn recording(&self) -> bool {
        SettingsWindow::recording(self)
    }
}

impl Model for Welcome {
    fn event(&mut self, input: Input) -> bool {
        Welcome::event(self, input)
    }
    fn content_size(&self) -> (u32, u32) {
        Welcome::content_size(self)
    }
    fn draw(&mut self, dst: &mut BgrxMut) {
        Welcome::draw(self, dst);
    }
    fn closed(&self) -> bool {
        self.finished().is_some()
    }
    fn recording(&self) -> bool {
        false
    }
}

impl Model for Permission {
    fn event(&mut self, input: Input) -> bool {
        Permission::event(self, input)
    }
    fn content_size(&self) -> (u32, u32) {
        Permission::content_size(self)
    }
    fn draw(&mut self, dst: &mut BgrxMut) {
        Permission::draw(self, dst);
    }
    fn closed(&self) -> bool {
        Permission::closed(self)
    }
    fn recording(&self) -> bool {
        false
    }
}

struct Handler<M> {
    model: Rc<RefCell<M>>,
    /// `settings`, `welcome` or `permission`, for the log.
    name: &'static str,
    /// When the app asked for the window, until its first frame is on screen.
    opened: Option<Instant>,
}

impl<M: Model> FormHandler for Handler<M> {
    fn event(&mut self, window: &mut FormWindow, event: FormEvent) {
        let mut model = self.model.borrow_mut();
        let changed = match event {
            FormEvent::Input(input) => model.event(input),
            FormEvent::Refresh => true,
        };
        if model.closed() {
            window.capture_keys(false);
            return window.close();
        }
        window.capture_keys(model.recording());
        let (w, h) = model.content_size();
        if window.size() != (w, h) {
            // A resize follows, and the frame with it.
            return window.set_client_size(w, h);
        }
        if !changed {
            return;
        }
        if let Some(mut canvas) = window.canvas() {
            model.draw(&mut canvas);
        }
        window.present(Rect::new(0, 0, w as i32, h as i32));
        if let Some(t) = self.opened.take() {
            log!("{} open in {:.1} ms", self.name, ms(t.elapsed()));
        }
    }
}

/// Opens a form window for `model`, centered on the monitor with the cursor. `make` builds the
/// model for that monitor's scale.
fn open<M: Model + 'static>(
    name: &'static str,
    title: &str,
    dark: bool,
    follow_system: bool,
    make: impl FnOnce(f32) -> M,
) -> Option<(Rc<RefCell<M>>, FormRef)> {
    let opened = Instant::now();
    let monitors = sys::monitors();
    let (x, y) = sys::cursor_pos();
    let cursor = Point::new(x, y);
    let monitor = monitors.iter().min_by_key(|m| m.rect.distance_sq(cursor))?;
    let model = make(monitor.scale);
    let (w, h) = model.content_size();
    let work = monitor.work;
    let client = Rect::new(
        work.x + (work.width - w as i32) / 2,
        work.y + (work.height - h as i32) / 2,
        w as i32,
        h as i32,
    );
    let model = Rc::new(RefCell::new(model));
    let handler = Handler {
        model: model.clone(),
        name,
        opened: Some(opened),
    };
    match FormWindow::open(
        title,
        client,
        monitor.scale,
        dark,
        follow_system,
        Box::new(handler),
    ) {
        Ok(window) => Some((model, window)),
        Err(e) => {
            log!("{name} window: {e}");
            None
        }
    }
}

/// The open settings window.
pub struct SettingsUi {
    model: Rc<RefCell<SettingsWindow>>,
    window: FormRef,
}

impl SettingsUi {
    pub fn open(values: Values, dark: bool, follow_system: bool) -> Option<Self> {
        let (model, window) = open("settings", SETTINGS_TITLE, dark, follow_system, |scale| {
            SettingsWindow::new(values, env!("CARGO_PKG_VERSION"), OS, scale, dark)
        })?;
        Some(SettingsUi { model, window })
    }

    pub fn window(&self) -> FormRef {
        self.window
    }

    /// The window is gone and asks for nothing more.
    pub fn is_done(&self) -> bool {
        !self.window.is_open()
    }

    pub fn recording(&self) -> bool {
        self.model.borrow().recording()
    }

    pub fn take_requests(&self) -> Vec<Request> {
        self.model.borrow_mut().take_requests()
    }

    /// Shows `values` if they changed.
    pub fn update(&self, values: Values) {
        let mut model = self.model.borrow_mut();
        if model.values() != &values {
            model.set_values(values);
            self.window.refresh();
        }
    }

    pub fn hotkey_taken(&self, hotkey: Hotkey) {
        self.model.borrow_mut().hotkey_taken(hotkey);
        self.window.refresh();
    }

    pub fn hotkey_refused(&self, wanted: Hotkey, old_works: bool) {
        self.model.borrow_mut().hotkey_refused(wanted, old_works);
        self.window.refresh();
    }
}

/// The open welcome.
pub struct WelcomeUi {
    model: Rc<RefCell<Welcome>>,
    window: FormRef,
}

impl WelcomeUi {
    pub fn open(hotkey: Hotkey, dark: bool, follow_system: bool) -> Option<Self> {
        let (model, window) = open("welcome", WELCOME_TITLE, dark, follow_system, |scale| {
            Welcome::new(hotkey, OS, scale, dark)
        })?;
        Some(WelcomeUi { model, window })
    }

    pub fn window(&self) -> FormRef {
        self.window
    }

    /// The welcome ended: with the choice, or closed some other way (`None` from a window that
    /// is gone without one).
    pub fn finished(&self) -> Option<Option<Finish>> {
        let finish = self.model.borrow().finished();
        match finish {
            Some(f) => Some(Some(f)),
            None if !self.window.is_open() => Some(None),
            None => None,
        }
    }
}

/// The open Screen Recording window.
pub struct PermissionUi {
    model: Rc<RefCell<Permission>>,
    window: FormRef,
}

impl PermissionUi {
    /// `granted`: whether Shot has the permission now.
    pub fn open(granted: bool, dark: bool, follow_system: bool) -> Option<Self> {
        let (model, window) = open(
            "permission",
            PERMISSION_TITLE,
            dark,
            follow_system,
            |scale| Permission::new(granted, scale, dark),
        )?;
        Some(PermissionUi { model, window })
    }

    pub fn window(&self) -> FormRef {
        self.window
    }

    /// The window is gone.
    pub fn is_done(&self) -> bool {
        !self.window.is_open()
    }

    /// `Открыть Системные настройки` was pressed since the last call.
    pub fn take_open_settings(&self) -> bool {
        self.model.borrow_mut().take_open_settings()
    }

    /// Shows whether the permission is there now.
    pub fn set_granted(&self, granted: bool) {
        if self.model.borrow_mut().set_granted(granted) {
            self.window.refresh();
        }
    }
}
