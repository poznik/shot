//! Values under `HKEY_CURRENT_USER`: the taskbar theme and autostart.

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::PCWSTR;

pub fn get_dword(key: PCWSTR, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

pub fn get_string(key: PCWSTR, name: PCWSTR) -> Option<String> {
    let mut size = 0u32;
    unsafe {
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 1];
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        );
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

pub fn get_binary(key: PCWSTR, name: PCWSTR) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 64];
    let mut size = buf.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then(|| {
        buf.truncate(size as usize);
        buf
    })
}

pub fn set_string(key: PCWSTR, name: PCWSTR, value: &str) -> windows::core::Result<()> {
    let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
    }
    .ok()
}

/// Deletes a value; a value that is not there is not an error.
pub fn delete(key: PCWSTR, name: PCWSTR) -> windows::core::Result<()> {
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, name) };
    if status == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    status.ok()
}
