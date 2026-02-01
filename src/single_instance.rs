//! Single instance module.
//!
//! Ensures only one instance of the application is running using Windows Mutex.

use std::sync::atomic::{AtomicIsize, Ordering};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK},
    },
};

// Note: CreateMutexW requires both Win32_System_Threading and Win32_Security features

/// Unique mutex name for the application.
const MUTEX_NAME: &str = "Global\\LayoutIndicatorMutex_UniqueInstance_Rust";

/// Handle to the mutex stored as atomic isize for thread safety.
static MUTEX_HANDLE: AtomicIsize = AtomicIsize::new(0);

/// Checks if another instance is already running.
///
/// Creates a named mutex. If the mutex already exists (created by another instance),
/// returns true.
pub fn is_already_running() -> bool {
    let mutex_name: Vec<u16> = MUTEX_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr()));

        match handle {
            Ok(h) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    // Mutex already exists - another instance is running
                    let _ = CloseHandle(h);
                    true
                } else {
                    // Mutex created successfully - this is the first instance
                    MUTEX_HANDLE.store(h.0 as isize, Ordering::SeqCst);
                    false
                }
            }
            Err(_) => {
                // Failed to create mutex - assume another instance is running
                true
            }
        }
    }
}

/// Releases the mutex when the application exits.
pub fn release_mutex() {
    let handle_value = MUTEX_HANDLE.swap(0, Ordering::SeqCst);
    if handle_value != 0 {
        unsafe {
            let handle = HANDLE(handle_value as *mut std::ffi::c_void);
            let _ = CloseHandle(handle);
        }
    }
}

/// Shows a message box informing the user that the application is already running.
pub fn show_already_running_message() {
    let title: Vec<u16> = "Layout Indicator"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let message: Vec<u16> =
        "Layout Indicator is already running.\n\nLook for the icon in the system tray."
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
