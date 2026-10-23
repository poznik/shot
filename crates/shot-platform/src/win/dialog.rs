//! The native folder dialog for the settings window's `Изменить…`.

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::HSTRING;

use super::FormRef;

/// Shows the folder dialog titled `title` over `owner`, starting in `start`, and waits for it.
/// Returns the chosen folder, or `None` when the dialog was cancelled.
pub fn pick_folder(
    owner: Option<FormRef>,
    title: &str,
    start: Option<&Path>,
) -> windows::core::Result<Option<PathBuf>> {
    unsafe {
        // The dialog needs COM on this thread; it may be there already.
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let result = show(owner, title, start);
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

unsafe fn show(
    owner: Option<FormRef>,
    title: &str,
    start: Option<&Path>,
) -> windows::core::Result<Option<PathBuf>> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
        dialog.SetTitle(&HSTRING::from(title))?;
        if let Some(start) = start.filter(|p| p.is_dir())
            && let Ok(item) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(start), None)
        {
            let _ = dialog.SetFolder(&item);
        }
        // Cancelling fails with `ERROR_CANCELLED`.
        if dialog.Show(owner.map(|o| o.hwnd())).is_err() {
            return Ok(None);
        }
        let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0 as *const _));
        Ok(path)
    }
}
