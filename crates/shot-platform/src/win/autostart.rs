//! Autostart at sign-in (SPEC §3.5): the `Shot` value under `HKCU\…\CurrentVersion\Run`. The
//! registry is the only record of it, so the menu and the settings never disagree with Task
//! Manager's Startup page, which can switch the value off on its own.

use windows::core::w;

use super::registry;

const RUN: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
/// Task Manager's on/off state of each Run value: the first byte is even when it is on.
const APPROVED: windows::core::PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
const NAME: windows::core::PCWSTR = w!("Shot");

/// Whether Shot starts at sign-in.
pub fn autostart() -> bool {
    registry::get_string(RUN, NAME).is_some()
        && registry::get_binary(APPROVED, NAME)
            .is_none_or(|state| state.first().is_none_or(|b| b % 2 == 0))
}

/// Turns autostart on for this `shot.exe`, or off.
pub fn set_autostart(on: bool) -> windows::core::Result<()> {
    if on {
        let exe = std::env::current_exe().map_err(|_| windows::core::Error::from_thread())?;
        registry::set_string(RUN, NAME, &format!("\"{}\"", exe.display()))?;
        // Clears an "off" Task Manager kept from before.
        registry::delete(APPROVED, NAME)
    } else {
        registry::delete(RUN, NAME)
    }
}
