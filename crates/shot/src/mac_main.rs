//! The macOS event loop: AppKit's events go to their windows inside `next_event`; what concerns
//! the app comes out of it already sorted.

use shot_platform::sys::{self, Event, TrayEvent};

use crate::app::{App, Session};

pub fn run() {
    sys::init();
    let Some((mut app, _watch)) = App::start() else {
        return;
    };
    let mut session: Option<Session> = None;
    loop {
        app.serve_windows();
        match sys::next_event() {
            Event::Quit => break,
            Event::Folder => app.reload_settings(),
            Event::Activated => {
                app.check_permission();
            }
            Event::Hotkey { queued_ms } if session.is_none() => {
                session = Session::start(&mut app, queued_ms);
            }
            Event::Tray(TrayEvent::Capture) if session.is_none() => {
                session = Session::start(&mut app, 0);
            }
            Event::Tray(TrayEvent::Menu(at)) if session.is_none() => app.menu(at),
            Event::Tray(TrayEvent::Wake) => app.open_settings(),
            Event::Preview(event) => {
                app.previews.handle(event);
                app.open_editors();
            }
            Event::Overlay(event) => {
                if let Some(s) = session.as_mut()
                    && let Some(outcome) = s.handle(&mut app.windows, event)
                    && let Some(s) = session.take()
                {
                    s.finish(&mut app, outcome);
                }
            }
            _ => {}
        }
    }
}
