//! Dragging a file out of a window, as from Finder: the editor's drag-out button (SPEC §3.3).
//! The pasteboard gets the file's URL, as Finder gives it, so every drop target that takes files
//! from Finder — a folder, the desktop, a chat — takes it from Shot too.

use std::cell::{Cell, OnceCell};
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBitmapImageRep, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSImage, NSView, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSData, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use shot_core::image::{BgrxView, Compression, encode_png};

use super::Error;

thread_local! {
    /// The window whose drag runs: it hears when the drag ends.
    static DRAGGING: Cell<Option<isize>> = const { Cell::new(None) };
    static SOURCE: OnceCell<Retained<Source>> = const { OnceCell::new() };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ShotDragSource"]
    struct Source;

    unsafe impl NSObjectProtocol for Source {}

    unsafe impl NSDraggingSource for Source {
        /// Copied, never moved: the original stays where it is.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operations(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, _at: NSPoint, _operation: NSDragOperation) {
            if let Some(id) = DRAGGING.take() {
                super::editor::drag_ended(id, Ok(()));
            }
        }
    }
);

/// What the drag shows under the cursor: a PNG of it, and its size in points.
pub(super) struct DragImage {
    png: Vec<u8>,
    size: NSSize,
}

impl DragImage {
    /// `image` in px of a screen at `scale`.
    pub(super) fn new(image: BgrxView, scale: f32) -> Option<Self> {
        let mut png = Vec::new();
        encode_png(image, &mut png, Compression::Fast, None).ok()?;
        let s = f64::from(scale.max(1.0));
        Some(DragImage {
            png,
            size: NSSize::new(f64::from(image.width) / s, f64::from(image.height) / s),
        })
    }

    fn ns_image(&self) -> Option<Retained<NSImage>> {
        let rep = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&self.png))?;
        rep.setSize(self.size);
        let image = NSImage::initWithSize(NSImage::alloc(), self.size);
        image.addRepresentation(&rep);
        Some(image)
    }
}

/// Starts dragging the file at `path` out of `view`, the content of window `id`, with `image`
/// under the cursor; `event` is the press or the drag that starts it. AppKit runs the drag from
/// the event loop; the window hears of its end through [`super::editor::drag_ended`].
pub(super) fn drag_file(
    id: isize,
    view: &NSView,
    event: &NSEvent,
    path: &Path,
    image: Option<&DragImage>,
) -> Result<(), Error> {
    let mtm = MainThreadMarker::new().ok_or_else(|| Error::new("not the main thread"))?;
    let path = NSString::from_str(&path.to_string_lossy());
    let url = NSURL::fileURLWithPath(&path);
    let item = NSDraggingItem::initWithPasteboardWriter(
        NSDraggingItem::alloc(),
        ProtocolObject::from_ref(&*url),
    );
    // No thumbnail is no reason to give up the drag: Finder's icon of the file stands in.
    let contents = image
        .and_then(DragImage::ns_image)
        .unwrap_or_else(|| NSWorkspace::sharedWorkspace().iconForFile(&path));
    let size = contents.size();
    // The cursor at the middle of the bottom edge: the image is carried above it.
    let p = view.convertPoint_fromView(event.locationInWindow(), None);
    let y = if view.isFlipped() {
        p.y - size.height
    } else {
        p.y
    };
    let frame = NSRect::new(NSPoint::new(p.x - size.width / 2.0, y), size);
    let contents: &AnyObject = &contents;
    unsafe { item.setDraggingFrame_contents(frame, Some(contents)) };
    let source = SOURCE.with(|s| {
        s.get_or_init(|| unsafe { msg_send![Source::alloc(mtm), init] })
            .clone()
    });
    DRAGGING.set(Some(id));
    view.beginDraggingSessionWithItems_event_source(
        &NSArray::from_slice(&[&*item]),
        event,
        ProtocolObject::from_ref(&*source),
    );
    Ok(())
}

/// Window `id`'s drag, if one runs, is over without a word from AppKit: a new press came.
pub(super) fn abandon(id: isize) -> bool {
    if DRAGGING.get() == Some(id) {
        DRAGGING.set(None);
        true
    } else {
        false
    }
}
