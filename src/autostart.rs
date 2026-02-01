//! Autostart module.
//!
//! Manages autostart through Windows Registry.

use windows::{
    core::PCWSTR,
    Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
    },
};

const APP_NAME: &str = "LayoutIndicator";
const REG_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Gets the path to the executable.
fn get_exe_path() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Checks if autostart is enabled.
pub fn is_autostart_enabled() -> bool {
    let reg_path: Vec<u16> = REG_PATH.encode_utf16().chain(std::iter::once(0)).collect();
    let app_name: Vec<u16> = APP_NAME.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut key: HKEY = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(reg_path.as_ptr()),
            0,
            KEY_READ,
            &mut key,
        );

        if result.is_err() {
            return false;
        }

        let query_result = RegQueryValueExW(key, PCWSTR(app_name.as_ptr()), None, None, None, None);

        let _ = RegCloseKey(key);
        query_result.is_ok()
    }
}

/// Enables autostart.
pub fn enable_autostart() -> bool {
    let Some(exe_path) = get_exe_path() else {
        return false;
    };

    let reg_path: Vec<u16> = REG_PATH.encode_utf16().chain(std::iter::once(0)).collect();
    let app_name: Vec<u16> = APP_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let exe_path_wide: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut key: HKEY = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(reg_path.as_ptr()),
            0,
            KEY_WRITE,
            &mut key,
        );

        if result.is_err() {
            return false;
        }

        let set_result = RegSetValueExW(
            key,
            PCWSTR(app_name.as_ptr()),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                exe_path_wide.as_ptr() as *const u8,
                exe_path_wide.len() * 2,
            )),
        );

        let _ = RegCloseKey(key);
        set_result.is_ok()
    }
}

/// Disables autostart.
pub fn disable_autostart() -> bool {
    let reg_path: Vec<u16> = REG_PATH.encode_utf16().chain(std::iter::once(0)).collect();
    let app_name: Vec<u16> = APP_NAME.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut key: HKEY = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(reg_path.as_ptr()),
            0,
            KEY_WRITE,
            &mut key,
        );

        if result.is_err() {
            return false;
        }

        let delete_result = RegDeleteValueW(key, PCWSTR(app_name.as_ptr()));
        let _ = RegCloseKey(key);

        // Success if deleted or didn't exist
        delete_result.is_ok()
    }
}
