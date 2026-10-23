//! The Windows event loop: every message goes to the tray, the preview cards, the hotkey or the
//! overlay, whichever it belongs to, and then to its window.

use shot_platform::sys::{self, TrayEvent, Wake};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, WM_HOTKEY};

use crate::app::{App, HOTKEY_CAPTURE, Session};

pub fn run() {
    let _ = sys::enable_per_monitor_dpi();
    let Some((mut app, watch)) = App::start() else {
        return;
    };
    let mut session: Option<Session> = None;
    let mut msg = MSG::default();
    loop {
        app.serve_windows();
        match sys::next_event(&mut msg, watch.as_ref()) {
            Wake::Quit => break,
            Wake::Folder => {
                app.reload_settings();
                continue;
            }
            Wake::Message => {}
        }
        if let Some(event) = app.tray.event(&msg) {
            match event {
                TrayEvent::Capture if session.is_none() => session = Session::start(&mut app, 0),
                TrayEvent::Menu(at) if session.is_none() => app.menu(at),
                TrayEvent::Wake => app.open_settings(),
                _ => {}
            }
            continue;
        }
        if app.previews.event(&msg) {
            app.open_editors();
            continue;
        }
        if msg.message == WM_HOTKEY && msg.wParam.0 == HOTKEY_CAPTURE as usize {
            if session.is_none() {
                // `msg.time` and `GetTickCount` share a clock with ~16 ms resolution: a rough
                // measure of how long the hotkey waited in the queue.
                let queued_ms = unsafe { GetTickCount() }.wrapping_sub(msg.time);
                session = Session::start(&mut app, queued_ms);
            }
            continue;
        }
        if let Some(s) = session.as_mut()
            && let Some(event) = app.windows.event(&msg)
            && let Some(outcome) = s.handle(&mut app.windows, event)
            && let Some(s) = session.take()
        {
            s.finish(&mut app, outcome);
        }
        unsafe { DispatchMessageW(&msg) };
    }
}
