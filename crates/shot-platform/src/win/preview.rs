//! Preview windows (SPEC §3.2, WIN-004). Each card is a layered popup that never takes the focus,
//! with its shadow in a second, click-through window under it: a shadow inside the card's own
//! window would catch the clicks meant for the taskbar below. Both are left out of screen
//! captures, so a card on screen never ends up in the next screenshot.
//!
//! The app reads the mouse, the timer and the save notices from their messages with
//! [`PreviewWindows::event`]; the timer runs only while cards are on screen.

use std::time::Duration;

use shot_core::geom::{Point, Rect};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, TRUE, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, CreateWindowExW, DefWindowProcW, DeferWindowPos, EndDeferWindowPos,
    HWND_MESSAGE, HWND_TOPMOST, IDC_HAND, KillTimer, LoadCursorW, MA_NOACTIVATE, MSG, PostMessageW,
    RegisterClassW, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SetCursor, SetTimer, SetWindowDisplayAffinity, SetWindowPos, ULW_ALPHA,
    UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WM_APP, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;

use super::Bitmap;

const CLASS: windows::core::PCWSTR = w!("ShotPreview");
/// Posted to the notice window by [`Notifier::notify`], from any thread.
const WM_PREVIEW_NOTIFY: u32 = WM_APP + 0x30;
/// Posted to the notice window by [`PreviewWindows::acquire`]: the spare pair it gave out is
/// replaced once the loop is idle, not on the next capture's path.
const WM_PREVIEW_SPARE: u32 = WM_APP + 0x31;
const TIMER_ID: usize = 1;
/// windows-rs keeps it under `UI::Controls`, a feature Shot does not need otherwise.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// What happened to the cards. `slot` is the window pair from [`PreviewWindows::acquire`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewEvent {
    /// The cursor came onto the card.
    Enter(usize),
    Leave(usize),
    /// A click at `at`, in the card's own pixels.
    Click {
        slot: usize,
        at: Point,
    },
    /// The time asked for with [`PreviewWindows::set_timer`] has come.
    Timer,
    /// A [`Notifier`] posted.
    Notify,
    /// The windows handled a message of their own; nothing changed for the app.
    Internal,
}

/// Premultiplied BGRA pixels, rows top to bottom without padding.
#[derive(Clone, Copy, Debug)]
pub struct Sprite<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Wakes the app from another thread: a save finished.
#[derive(Clone, Debug)]
pub struct Notifier(isize);

impl Notifier {
    pub fn notify(&self) {
        let hwnd = HWND(self.0 as *mut _);
        let _ = unsafe { PostMessageW(Some(hwnd), WM_PREVIEW_NOTIFY, WPARAM(0), LPARAM(0)) };
    }
}

pub struct PreviewWindows {
    pairs: Vec<Pair>,
    /// Message-only window: the timer and the notices.
    notice: HWND,
    /// Free pairs kept ready: making one takes ~7 ms, mostly in DWM.
    spares: usize,
}

struct Pair {
    card: HWND,
    shadow: HWND,
    used: bool,
    shown: bool,
    /// `TrackMouseEvent` waits for the cursor to leave the card.
    tracking: bool,
}

impl PreviewWindows {
    /// Registers the class. `spares` pairs are made ahead, off the capture path, once the loop
    /// is idle: as many as one capture shows cards at once.
    pub fn new(spares: usize) -> windows::core::Result<Self> {
        let instance = unsafe { GetModuleHandleW(None)? };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
        let notice = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                None,
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )?
        };
        unsafe { PostMessageW(Some(notice), WM_PREVIEW_SPARE, WPARAM(0), LPARAM(0))? };
        Ok(Self {
            pairs: Vec::new(),
            notice,
            spares: spares.max(1),
        })
    }

    pub fn notifier(&self) -> Notifier {
        Notifier(self.notice.0 as isize)
    }

    /// A free window pair: a spare, or a new one when none is left.
    pub fn acquire(&mut self) -> windows::core::Result<usize> {
        let slot = match self.pairs.iter().position(|p| !p.used) {
            Some(i) => i,
            None => self.add_pair()?,
        };
        self.pairs[slot].used = true;
        let _ = unsafe { PostMessageW(Some(self.notice), WM_PREVIEW_SPARE, WPARAM(0), LPARAM(0)) };
        Ok(slot)
    }

    fn make_spares(&mut self) -> windows::core::Result<()> {
        while self.pairs.iter().filter(|p| !p.used).count() < self.spares {
            self.add_pair()?;
        }
        Ok(())
    }

    fn add_pair(&mut self) -> windows::core::Result<usize> {
        let shadow = create(WS_EX_TRANSPARENT)?;
        let card = create(WINDOW_EX_STYLE(0))?;
        self.pairs.push(Pair {
            card,
            shadow,
            used: false,
            shown: false,
            tracking: false,
        });
        Ok(self.pairs.len() - 1)
    }

    /// Hides the pair and makes it free.
    pub fn release(&mut self, slot: usize) {
        let Some(p) = self.pairs.get_mut(slot) else {
            return;
        };
        for hwnd in [p.card, p.shadow] {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_HIDEWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
                );
            }
        }
        p.used = false;
        p.shown = false;
        p.tracking = false;
    }

    /// Puts the card at `at` and its shadow at `shadow_at`, both at opacity `alpha`. A sprite
    /// that is `None` keeps the pixels the window has. Shows the pair if it is hidden.
    pub fn update(
        &mut self,
        slot: usize,
        card: Option<Sprite>,
        at: Point,
        shadow: Option<Sprite>,
        shadow_at: Point,
        alpha: u8,
    ) -> windows::core::Result<()> {
        let Some(p) = self.pairs.get_mut(slot) else {
            return Ok(());
        };
        layer(p.shadow, shadow, shadow_at, alpha)?;
        layer(p.card, card, at, alpha)?;
        if p.shown {
            return Ok(());
        }
        p.shown = true;
        let (new_shadow, new_card) = (p.shadow, p.card);
        // One move for all: the new shadow on top, then every card over it, so that no shadow
        // lies on a card.
        let cards: Vec<HWND> = self
            .pairs
            .iter()
            .filter(|q| q.shown)
            .map(|q| q.card)
            .collect();
        let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
        unsafe {
            let mut batch = BeginDeferWindowPos(cards.len() as i32 + 1)?;
            batch = DeferWindowPos(
                batch,
                new_shadow,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                flags | SWP_SHOWWINDOW,
            )?;
            for card in cards {
                let show = if card == new_card {
                    SWP_SHOWWINDOW
                } else {
                    Default::default()
                };
                batch = DeferWindowPos(batch, card, Some(HWND_TOPMOST), 0, 0, 0, 0, flags | show)?;
            }
            EndDeferWindowPos(batch)
        }
    }

    /// Asks for [`PreviewEvent::Timer`] after `after`, or for none.
    pub fn set_timer(&self, after: Option<Duration>) {
        unsafe {
            match after {
                Some(d) => {
                    let ms = d.as_millis().clamp(1, u32::MAX as u128) as u32;
                    SetTimer(Some(self.notice), TIMER_ID, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.notice), TIMER_ID);
                }
            }
        }
    }

    /// The card event a message carries.
    pub fn event(&mut self, msg: &MSG) -> Option<PreviewEvent> {
        if msg.hwnd == self.notice {
            return match msg.message {
                WM_TIMER if msg.wParam.0 == TIMER_ID => {
                    // One shot: the app sets the next one.
                    unsafe {
                        let _ = KillTimer(Some(self.notice), TIMER_ID);
                    }
                    Some(PreviewEvent::Timer)
                }
                WM_PREVIEW_NOTIFY => Some(PreviewEvent::Notify),
                WM_PREVIEW_SPARE => {
                    // A failure here is tried again on the capture path.
                    let _ = self.make_spares();
                    Some(PreviewEvent::Internal)
                }
                _ => None,
            };
        }
        let slot = self.pairs.iter().position(|p| p.card == msg.hwnd)?;
        let p = &mut self.pairs[slot];
        match msg.message {
            WM_MOUSEMOVE if !p.tracking => {
                p.tracking = true;
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: p.card,
                    dwHoverTime: 0,
                };
                let _ = unsafe { TrackMouseEvent(&mut track) };
                Some(PreviewEvent::Enter(slot))
            }
            WM_MOUSELEAVE => {
                p.tracking = false;
                Some(PreviewEvent::Leave(slot))
            }
            WM_LBUTTONUP => {
                let (x, y) = (msg.lParam.0 as i16, (msg.lParam.0 >> 16) as i16);
                Some(PreviewEvent::Click {
                    slot,
                    at: Point::new(x as i32, y as i32),
                })
            }
            _ => None,
        }
    }
}

fn create(extra: WINDOW_EX_STYLE) -> windows::core::Result<HWND> {
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | extra,
            CLASS,
            None,
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(GetModuleHandleW(None)?.into()),
            None,
        )?
    };
    unsafe {
        // The fades are Shot's own.
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &TRUE as *const _ as _,
            size_of_val(&TRUE) as u32,
        );
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }
    Ok(hwnd)
}

/// `UpdateLayeredWindow` with new pixels, or only a new position and opacity.
fn layer(hwnd: HWND, sprite: Option<Sprite>, at: Point, alpha: u8) -> windows::core::Result<()> {
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let dst = POINT { x: at.x, y: at.y };
    let Some(sprite) = sprite else {
        return unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&dst),
                None,
                None,
                None,
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        };
    };
    let mut bitmap = Bitmap::new(Rect::new(0, 0, sprite.width as i32, sprite.height as i32))?;
    bitmap.view_mut().data.copy_from_slice(sprite.data);
    let size = SIZE {
        cx: sprite.width as i32,
        cy: sprite.height as i32,
    };
    let src = POINT { x: 0, y: 0 };
    unsafe {
        UpdateLayeredWindow(
            hwnd,
            None,
            Some(&dst),
            Some(&size),
            Some(bitmap.dc()),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
