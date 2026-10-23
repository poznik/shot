//! The folder dialog of the settings window, `NSOpenPanel`. A second `shot` shows it (see
//! `shot --pick-folder`): the panel leaves ~3 MB in the process that showed it (MAC-006), and
//! they leave with that process, as the shell's do on Windows.

use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
    NSModalResponseOK, NSOpenPanel, NSRunningApplication,
};
use objc2_foundation::{NSOperationQueue, NSPoint, NSString, NSURL};

use super::{Error, FormRef, dev, window_list};

/// How far below the top of its window a sheet starts: the title bar, in points.
const TITLE_BAR: f64 = 28.0;

/// Shows the folder dialog with `message` over `owner`, a window of the Shot that asked, starting
/// in `start`, and waits for it. Returns the folder chosen, or `None` when the dialog was
/// cancelled.
pub fn pick_folder(
    owner: Option<FormRef>,
    message: &str,
    start: Option<&Path>,
) -> Result<Option<PathBuf>, Error> {
    let mtm = MainThreadMarker::new().ok_or_else(|| Error::new("not on the main thread"))?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    panel.setMessage(Some(&NSString::from_str(message)));
    panel.setPrompt(Some(&NSString::from_str("Выбрать")));
    if let Some(start) = start {
        let start = NSString::from_str(&start.to_string_lossy());
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&start)));
    }
    // Where a sheet of the owner would be: under its title bar, in the middle. `runModal` puts the
    // panel in the middle of the screen first: it moves once the modal loop runs.
    if let Some(owner) = owner.and_then(|o| window_list::window_frame(o.raw())) {
        let moved = panel.clone();
        let block = RcBlock::new(move || {
            let size = moved.frame().size;
            moved.setFrameTopLeftPoint(NSPoint::new(
                owner.origin.x + (owner.size.width - size.width) / 2.0,
                owner.origin.y + owner.size.height - TITLE_BAR,
            ));
        });
        // SAFETY: the block owns what it uses and runs on the main thread.
        unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&block) };
    }
    activate(&app);
    let _dev = dev::listen_panel();
    let response = panel.runModal();
    // With nothing selected the button chooses the folder the panel shows.
    let chosen = (response == NSModalResponseOK)
        .then(|| panel.URL().or_else(|| panel.directoryURL()))
        .flatten()
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()));
    // The keyboard goes back to the Shot that asked, not to the app before it.
    if let Some(parent) = NSRunningApplication::runningApplicationWithProcessIdentifier(
        std::os::unix::process::parent_id() as i32,
    ) {
        parent.activateWithOptions(NSApplicationActivationOptions::empty());
    }
    Ok(chosen)
}

/// The dialog takes the keyboard from the Shot that asked for it: that one is active, and
/// `activate()` would wait for it to yield.
#[allow(deprecated)]
fn activate(app: &NSApplication) {
    app.activateIgnoringOtherApps(true);
}
