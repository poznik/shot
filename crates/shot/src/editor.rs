//! The editor: `shot-core` edits and draws it, `shot-platform` gives it a window; this
//! ties both to the clipboard and to files.

use std::cell::Cell;
use std::fs::File;
use std::io::{self, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use shot_core::draw::{IntSize, Pixmap};
use shot_core::editor::{EDGE, Editor, Input, Open, Request, view};
use shot_core::geom::{Point, Rect};
use shot_core::image::{BgrxView, Decoded, decode_png};
use shot_core::naming::{LocalTime, capture_file_name, replace_png, save_captures};
use shot_core::preview::failure_reason;
use shot_core::settings::Destination;
use shot_platform::sys::{
    self, Clipboard, EditorHandler, EditorNotifier, EditorRef, EditorWindow, WindowEvent,
};

use crate::app::{BASE_DPI, OS, ms};
use crate::log::log;

/// A capture to open.
pub enum Source {
    /// Saved in a file, which saving overwrites.
    File(PathBuf),
    /// Only on the clipboard: kept as PNG while its card is up. Saving makes a new file.
    Memory { png: Vec<u8>, time: LocalTime },
}

/// What the editor needs from the app.
pub struct Context {
    pub clipboard: Rc<Clipboard>,
    /// Where a capture without a file is saved.
    pub out_dir: PathBuf,
    pub destination: Destination,
    pub dark: bool,
    /// The theme follows the system's.
    pub follow_system: bool,
}

/// An open editor, for the app.
pub struct EditorUi {
    window: EditorRef,
    /// The editor asks whether to save its edits.
    asking: Rc<Cell<bool>>,
}

impl EditorUi {
    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    pub fn window(&self) -> EditorRef {
        self.window
    }

    /// The editor asks whether to save its edits before it closes.
    pub fn asking(&self) -> bool {
        self.asking.get()
    }
}

/// Opens `source` in a new editor window on the monitor under the cursor. `clicked`: when the
/// user asked for it, for the log.
pub fn open(source: Source, context: Context, clicked: Instant) -> Option<EditorUi> {
    let (decoded, name, file, time) = match source {
        Source::File(path) => {
            let decoded = File::open(&path).and_then(|f| decode_png(BufReader::new(f)));
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (decoded, name, Some(path), sys::local_time())
        }
        Source::Memory { png, time } => (
            decode_png(Cursor::new(png)),
            capture_file_name(time, None),
            None,
            time,
        ),
    };
    let decoded = match decoded {
        Ok(d) => d,
        Err(e) => {
            log!("editor: cannot read {name}: {e}");
            return None;
        }
    };
    let t_decoded = clicked.elapsed();
    let Decoded {
        width,
        height,
        data,
        dpi,
    } = decoded;
    let image = Pixmap::from_vec(data, IntSize::from_wh(width, height)?)?;

    let monitors = sys::monitors();
    let (x, y) = sys::cursor_pos();
    let cursor = Point::new(x, y);
    let monitor = monitors.iter().min_by_key(|m| m.rect.distance_sq(cursor))?;
    let scale = monitor.scale;
    let work = monitor.work;
    let (w, h) = view::content_size(
        (width, height),
        (work.width as u32, work.height as u32),
        scale,
    );
    let client = Rect::new(
        work.x + (work.width - w as i32) / 2,
        work.y + (work.height - h as i32) / 2,
        w as i32,
        h as i32,
    );
    let image_scale = dpi.map_or(scale, |d| d / BASE_DPI);
    let now = Instant::now();
    let editor = Editor::new(
        Open {
            image,
            image_scale,
            name,
            has_file: file.is_some(),
            window: (w, h),
            scale,
            dark: context.dark,
            destination: context.destination,
            os: OS,
        },
        now,
    );
    let title = editor.title();
    let asking = Rc::new(Cell::new(false));
    let session = Session {
        editor,
        clipboard: context.clipboard,
        file,
        out_dir: context.out_dir,
        time,
        dpi: image_scale * BASE_DPI,
        scratch: Vec::new(),
        title: title.clone(),
        notifier: None,
        saving: None,
        queued: None,
        renamed: None,
        opened: Some((clicked, t_decoded)),
        asking: asking.clone(),
    };
    match EditorWindow::open(
        &title,
        client,
        scale,
        context.dark,
        context.follow_system,
        Box::new(session),
    ) {
        Ok(window) => Some(EditorUi { window, asking }),
        Err(e) => {
            log!("editor window: {e}");
            None
        }
    }
}

/// One editor window's state.
struct Session {
    editor: Editor,
    clipboard: Rc<Clipboard>,
    /// The file saving overwrites; `None` until a capture that was only copied is saved.
    file: Option<PathBuf>,
    out_dir: PathBuf,
    /// When the capture was taken: names its file when it gets one.
    time: LocalTime,
    /// The density the result is saved with.
    dpi: f32,
    /// Canvas memory kept between frames: fresh memory costs its zeroing on every frame.
    scratch: Vec<u8>,
    title: String,
    notifier: Option<EditorNotifier>,
    /// The save running in the background; it returns the new file, if it made one.
    saving: Option<JoinHandle<io::Result<Option<PathBuf>>>>,
    /// The result of a save asked for while another ran: it starts when that one is done.
    queued: Option<Pixmap>,
    /// The name of a file a save made, for the editor once the last save is done.
    renamed: Option<String>,
    /// For the log of the first frame: when the editor was asked for, and how long reading the
    /// image took.
    opened: Option<(Instant, std::time::Duration)>,
    /// Shared with [`EditorUi`].
    asking: Rc<Cell<bool>>,
}

impl EditorHandler for Session {
    fn event(&mut self, window: &mut EditorWindow, event: WindowEvent) {
        let now = Instant::now();
        if self.notifier.is_none() {
            self.notifier = Some(window.notifier());
        }
        let mut whole = false;
        match event {
            WindowEvent::Input(input) => {
                whole = matches!(input, Input::Resize { .. });
                self.editor.handle(input, now);
            }
            WindowEvent::Timer => self.editor.tick(now),
            WindowEvent::Notify => self.take_save(now),
        }
        for request in self.editor.take_requests() {
            self.run(window, request, now);
        }
        self.draw(window, whole);
        let title = self.editor.title();
        if title != self.title {
            window.set_title(&title);
            self.title = title;
        }
        window.set_cursor(self.editor.cursor());
        self.asking.set(self.editor.asking());
        window.set_timer(
            self.editor
                .next_deadline()
                .map(|t| t.saturating_duration_since(Instant::now())),
        );
        if let Some((clicked, decoded)) = self.opened.take() {
            let (w, h) = window.size();
            log!(
                "editor: {w}x{h}, image read {:.1} ms, first frame {:.1} ms",
                ms(decoded),
                ms(clicked.elapsed())
            );
        }
    }
}

impl Session {
    fn run(&mut self, window: &EditorWindow, request: Request, now: Instant) {
        match request {
            Request::Copy => {
                if self.copy(&self.editor.result()) {
                    self.editor.copied(now);
                }
            }
            Request::Save => {
                let result = self.editor.result();
                self.copy(&result);
                if self.saving.is_some() {
                    self.queued = Some(result);
                } else {
                    self.start_save(result);
                }
            }
            Request::SaveAndClose { file, copy } => {
                let result = self.editor.result();
                if copy {
                    self.copy(&result);
                }
                if file {
                    // A save still running writes the same file: it finishes first.
                    self.queued = None;
                    if let Some(saving) = self.saving.take()
                        && let Ok(Ok(Some(path))) = saving.join()
                    {
                        self.file = Some(path);
                    }
                    if let Err(e) = save(
                        &result,
                        self.file.clone(),
                        &self.out_dir,
                        self.time,
                        self.dpi,
                    ) {
                        log!("editor: not saved: {e}");
                        self.editor.failed(failure_reason(&e), now);
                        return;
                    }
                }
                window.close();
            }
            Request::Close => window.close(),
        }
    }

    /// Puts `image` on the clipboard; false when that failed.
    fn copy(&self, image: &Pixmap) -> bool {
        match self.clipboard.set_image(view_of(image), self.dpi) {
            Ok(()) => true,
            Err(e) => {
                log!("editor: clipboard failed: {e}");
                false
            }
        }
    }

    fn start_save(&mut self, image: Pixmap) {
        let (file, dir, time, dpi) = (self.file.clone(), self.out_dir.clone(), self.time, self.dpi);
        let notifier = self.notifier;
        self.saving = Some(thread::spawn(move || {
            let result = save(&image, file, &dir, time, dpi);
            if let Some(n) = notifier {
                n.notify();
            }
            result
        }));
    }

    /// A background save may be done: tells the editor, or starts the save that waited for it.
    fn take_save(&mut self, now: Instant) {
        if !self.saving.as_ref().is_some_and(JoinHandle::is_finished) {
            return;
        }
        let Some(saving) = self.saving.take() else {
            return;
        };
        let result = saving
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("the save stopped")));
        match result {
            Ok(new) => {
                if let Some(path) = new {
                    self.renamed = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    self.file = Some(path);
                }
                if let Some(next) = self.queued.take() {
                    return self.start_save(next);
                }
                self.editor.saved(self.renamed.take(), now);
            }
            Err(e) => {
                log!("editor: not saved: {e}");
                // A later save may still go through.
                if let Some(next) = self.queued.take() {
                    return self.start_save(next);
                }
                self.editor.failed(failure_reason(&e), now);
            }
        }
    }

    /// Draws the window's changed parts, or all of it, into the window's back buffer and shows
    /// them.
    fn draw(&mut self, window: &mut EditorWindow, whole: bool) {
        let (w, h) = window.size();
        let all = Rect::new(0, 0, w as i32, h as i32);
        let damage = self.editor.take_damage();
        let damage = if whole { vec![all] } else { damage };
        for r in damage {
            let Some(r) = r.intersect(&all) else {
                continue;
            };
            // The editor draws a margin of `EDGE` around the part, which is not shown: shapes
            // that cross the part's edge come out the same as in a whole frame.
            let area = r.inflate(EDGE);
            let mut data = std::mem::take(&mut self.scratch);
            data.resize(area.area() as usize * 4, 0);
            let Some(mut canvas) = IntSize::from_wh(area.width as u32, area.height as u32)
                .and_then(|size| Pixmap::from_vec(data, size))
            else {
                continue;
            };
            self.editor.render(&mut canvas, area);
            if let Some(back) = window.canvas() {
                let row = area.width as usize * 4;
                let src = canvas.data();
                for y in 0..r.height as usize {
                    let from = (y + EDGE as usize) * row + EDGE as usize * 4;
                    let to = (r.y as usize + y) * back.stride + r.x as usize * 4;
                    let len = r.width as usize * 4;
                    back.data[to..to + len].copy_from_slice(&src[from..from + len]);
                }
            }
            self.scratch = canvas.take();
            window.present(r);
        }
    }
}

/// Writes `image` over `file`, or into a new file in `dir` when there is none; returns the new
/// file.
fn save(
    image: &Pixmap,
    file: Option<PathBuf>,
    dir: &Path,
    time: LocalTime,
    dpi: f32,
) -> io::Result<Option<PathBuf>> {
    let t0 = Instant::now();
    let view = view_of(image);
    let result = match file {
        Some(path) => replace_png(&path, view, dpi).map(|()| (path, false)),
        None => save_captures(dir, time, &[(view, dpi)])
            .pop()
            .unwrap_or_else(|| Err(io::Error::other("nothing saved")))
            .map(|path| (path, true)),
    };
    let (path, new) = result?;
    log!(
        "editor: saved {} ({}x{}) in {:.1} ms",
        path.display(),
        image.width(),
        image.height(),
        ms(t0.elapsed())
    );
    Ok(new.then_some(path))
}

/// A result of the editor as the clipboard and PNG take it: opaque BGRA rows.
fn view_of(image: &Pixmap) -> BgrxView<'_> {
    BgrxView {
        width: image.width(),
        height: image.height(),
        stride: image.width() as usize * 4,
        data: image.data(),
    }
}
