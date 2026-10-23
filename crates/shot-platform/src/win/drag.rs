//! Dragging a file out of a window, as from Explorer: the editor's drag-out button (SPEC §3.3).
//! The shell makes the data object of the file itself, so every drop target that takes files
//! from Explorer — a folder, the desktop, a chat — takes it from Shot too.

use std::cell::Cell;
use std::path::Path;

use shot_core::image::BgrxView;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IDataObject};
use windows::Win32::System::Ole::{DROPEFFECT_COPY, IDropSource, OleInitialize};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_LBUTTON;
use windows::Win32::UI::Shell::{
    BHID_DataObject, CLSID_DragDropHelper, IDragSourceHelper, IShellItem,
    SHCreateItemFromParsingName, SHDRAGIMAGE, SHDoDragDrop,
};
use windows::core::HSTRING;

use super::editor::pressed;

/// What the drag shows under the cursor: opaque BGRA rows, top first.
pub(super) struct DragImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl DragImage {
    pub(super) fn new(image: BgrxView) -> Self {
        let mut data = Vec::with_capacity(image.width as usize * image.height as usize * 4);
        for y in 0..image.height {
            for px in image.row(y).as_chunks::<4>().0 {
                data.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        DragImage {
            width: image.width,
            height: image.height,
            data,
        }
    }

    /// A bottom-up DIB section: the drag helper shows a top-down one upside down.
    fn bitmap(&self) -> windows::core::Result<HBITMAP> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.width as i32,
                biHeight: self.height as i32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
        let row = self.width as usize * 4;
        let out = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), self.data.len()) };
        for (y, src) in self.data.chunks_exact(row).enumerate() {
            let to = (self.height as usize - 1 - y) * row;
            out[to..to + row].copy_from_slice(src);
        }
        Ok(bitmap)
    }
}

thread_local! {
    static OLE: Cell<bool> = const { Cell::new(false) };
}

/// Drags the file at `path` out of `hwnd` until it is dropped or dropped off, with `image` under
/// the cursor. Call it while the left button is down; it returns once the drag is over. The
/// file is copied, never moved: the original stays where it is.
pub(super) fn drag_file(
    hwnd: HWND,
    path: &Path,
    image: Option<&DragImage>,
) -> windows::core::Result<()> {
    // Released before the drag got its turn: a drag would drop at once, on this window.
    if !pressed(VK_LBUTTON) {
        return Ok(());
    }
    // Once per thread, kept: a drop target may still read the data object after the drag.
    if !OLE.get() {
        unsafe { OleInitialize(None)? };
        OLE.set(true);
    }
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(&HSTRING::from(path), None)? };
    let data: IDataObject = unsafe { item.BindToHandler(None, &BHID_DataObject)? };
    if let Some(image) = image {
        // No image is no reason to give up the drag: the shell shows the file's icon instead.
        let _ = set_image(&data, image);
    }
    unsafe { SHDoDragDrop(Some(hwnd), &data, None::<&IDropSource>, DROPEFFECT_COPY)? };
    Ok(())
}

fn set_image(data: &IDataObject, image: &DragImage) -> windows::core::Result<()> {
    let helper: IDragSourceHelper =
        unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)? };
    let bitmap = image.bitmap()?;
    let (w, h) = (image.width as i32, image.height as i32);
    let shdi = SHDRAGIMAGE {
        sizeDragImage: SIZE { cx: w, cy: h },
        // The cursor at the middle of the bottom edge: the image is carried above it.
        ptOffset: POINT { x: w / 2, y: h },
        hbmpDragImage: bitmap,
        crColorKey: COLORREF(0xFFFF_FFFF),
    };
    let result = unsafe { helper.InitializeFromBitmap(&shdi, data) };
    // The helper owns the bitmap only once it took it.
    if result.is_err() {
        let _ = unsafe { DeleteObject(bitmap.into()) };
    }
    result
}
