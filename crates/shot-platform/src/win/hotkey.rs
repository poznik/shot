//! Global hotkey through `RegisterHotKey`: no hooks, no extra permissions.

use shot_core::hotkey::{Hotkey, Key};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};

/// Registers a hotkey for the calling thread: `WM_HOTKEY` with `wParam == id` arrives in this
/// thread's message queue. Auto-repeat of a held key does not fire it again. Fails when another
/// program holds the combination.
pub fn register_hotkey(id: i32, hotkey: &Hotkey) -> windows::core::Result<()> {
    let mut flags = MOD_NOREPEAT;
    for (on, flag) in [
        (hotkey.ctrl, MOD_CONTROL),
        (hotkey.shift, MOD_SHIFT),
        (hotkey.alt, MOD_ALT),
        (hotkey.meta, MOD_WIN),
    ] {
        if on {
            flags |= flag;
        }
    }
    unsafe { RegisterHotKey(None, id, HOT_KEY_MODIFIERS(flags.0), vk_of(hotkey.key)) }
}

pub fn unregister_hotkey(id: i32) {
    let _ = unsafe { UnregisterHotKey(None, id) };
}

/// Punctuation keys of the US layout and their `VK_OEM_*` codes. Other layouts put other
/// characters on the same keys; the name follows the US key cap.
const OEM: [(char, u32); 11] = [
    ('`', 0xC0),
    ('-', 0xBD),
    ('=', 0xBB),
    ('[', 0xDB),
    (']', 0xDD),
    ('\\', 0xDC),
    (';', 0xBA),
    ('\'', 0xDE),
    (',', 0xBC),
    ('.', 0xBE),
    ('/', 0xBF),
];

const NAMED: [(Key, u32); 16] = [
    (Key::Space, 0x20),
    (Key::Enter, 0x0D),
    (Key::Tab, 0x09),
    (Key::Backspace, 0x08),
    (Key::Insert, 0x2D),
    (Key::Delete, 0x2E),
    (Key::Home, 0x24),
    (Key::End, 0x23),
    (Key::PageUp, 0x21),
    (Key::PageDown, 0x22),
    (Key::Left, 0x25),
    (Key::Right, 0x27),
    (Key::Up, 0x26),
    (Key::Down, 0x28),
    (Key::PrintScreen, 0x2C),
    (Key::Pause, 0x13),
];

/// The virtual-key code of a key.
pub fn vk_of(key: Key) -> u32 {
    match key {
        Key::Char(c) if c.is_ascii_alphanumeric() => c.to_ascii_uppercase() as u32,
        Key::Char(c) => OEM.iter().find(|(ch, _)| *ch == c).map_or(0, |(_, vk)| *vk),
        Key::F(n) => 0x70 + u32::from(n) - 1,
        named => NAMED
            .iter()
            .find(|(k, _)| *k == named)
            .map_or(0, |(_, vk)| *vk),
    }
}

/// The key of a virtual-key code, if a hotkey can use it (not a modifier, not `Esc`).
pub fn key_of(vk: u32) -> Option<Key> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(Key::Char),
        0x70..=0x87 => Some(Key::F((vk - 0x70 + 1) as u8)),
        _ => OEM
            .iter()
            .find(|(_, v)| *v == vk)
            .map(|(c, _)| Key::Char(*c))
            .or_else(|| NAMED.iter().find(|(_, v)| *v == vk).map(|(k, _)| *k)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for vk in 0..=0xFF {
            if let Some(key) = key_of(vk) {
                assert_eq!(vk_of(key), vk, "{key:?}");
            }
        }
        assert_eq!(vk_of(Key::Char('2')), 0x32);
        assert_eq!(vk_of(Key::F(12)), 0x7B);
        assert_eq!(key_of(0x1B), None, "Esc");
        assert_eq!(key_of(0x10), None, "Shift");
    }
}
