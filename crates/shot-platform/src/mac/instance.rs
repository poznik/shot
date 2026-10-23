//! One Shot per user: a lock on a file next to the settings. A second launch of the binary finds it
//! taken, tells the first through a distributed notification and quits; a second `open Shot.app`
//! never starts a process and comes as the app delegate's reopen.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString, ns_string};

use super::pump::{Event, push_and_wake};
use super::tray::TrayEvent;

/// Held while Shot runs.
pub struct InstanceLock {
    _file: Option<File>,
    _observer: Retained<ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
}

fn name() -> &'static NSString {
    ns_string!("com.poznik.shot.open-settings")
}

/// Takes the lock; `None` when another Shot holds it, which is asked to open its settings.
pub fn claim_instance() -> Option<InstanceLock> {
    let center = NSDistributedNotificationCenter::defaultCenter();
    let file = super::config_dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("shot.lock"))
            .ok()
    });
    if let Some(file) = &file {
        let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK);
        if taken {
            unsafe {
                center.postNotificationName_object_userInfo_deliverImmediately(
                    name(),
                    None,
                    None,
                    true,
                )
            };
            return None;
        }
    }
    let block = RcBlock::new(|_: NonNull<NSNotification>| {
        push_and_wake(Event::Tray(TrayEvent::Wake));
    });
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(name()),
            None::<&AnyObject>,
            None,
            &block,
        )
    };
    Some(InstanceLock {
        _file: file,
        _observer: observer,
    })
}
