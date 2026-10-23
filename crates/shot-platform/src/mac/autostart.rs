//! Start at login through `SMAppService.mainAppService`: the system lists Shot under Login Items.
//! It needs Shot to run from its app bundle. With `SHOT_DEV` set, Shot is under test on someone's
//! Mac: the switch is only remembered, and no login item is left behind.

use std::sync::atomic::{AtomicBool, Ordering};

use objc2_service_management::{SMAppService, SMAppServiceStatus};

use super::{Error, capture};

/// The switch under `SHOT_DEV`.
static DEV_ON: AtomicBool = AtomicBool::new(false);

pub fn autostart() -> bool {
    if capture::dev() {
        return DEV_ON.load(Ordering::Relaxed);
    }
    registered()
}

/// Whether the system starts this Shot at login.
pub(crate) fn registered() -> bool {
    unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
}

pub fn set_autostart(on: bool) -> Result<(), Error> {
    if capture::dev() {
        DEV_ON.store(on, Ordering::Relaxed);
        return Ok(());
    }
    let service = unsafe { SMAppService::mainAppService() };
    let done = unsafe {
        if on {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    done.map_err(|e| Error::new(e.localizedDescription().to_string()))
}
