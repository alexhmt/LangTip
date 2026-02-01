//! Keyboard hook module.
//!
//! Tracks keyboard layout changes using Windows hooks:
//! - SetWinEventHook for window focus changes
//! - SetWindowsHookEx with WH_KEYBOARD_LL for modifier key releases

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        Input::KeyboardAndMouse::GetKeyboardLayout,
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW,
            GetWindowThreadProcessId, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYUP, WM_QUIT,
            WM_SYSKEYUP,
        },
    },
};

// WinEvent constants
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

// Windows event constants
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;

// Virtual key codes for modifiers
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12; // Alt
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;

// Language IDs
const LANG_EN_US: u32 = 0x409;
const LANG_RU: u32 = 0x419;

// Debounce interval in milliseconds
const DEBOUNCE_MS: u64 = 100;

/// Layout information.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutInfo {
    /// Layout name (EN, RU, or hex code).
    pub name: String,
    /// Whether this is Russian layout.
    pub is_russian: bool,
}

/// Callback type for layout changes.
pub type LayoutCallback = Arc<dyn Fn(LayoutInfo) + Send + Sync>;

/// Wrapper for HHOOK to make it Send + Sync
#[derive(Debug, Clone, Copy)]
struct HhookWrapper(isize);

// SAFETY: HHOOK is just a handle, thread-safe when properly synchronized
unsafe impl Send for HhookWrapper {}
unsafe impl Sync for HhookWrapper {}

impl HhookWrapper {
    fn new(hook: HHOOK) -> Self {
        Self(hook.0 as isize)
    }

    fn as_hhook(&self) -> HHOOK {
        HHOOK(self.0 as *mut std::ffi::c_void)
    }
}

/// Global state for the hook callback.
struct HookState {
    callback: Option<LayoutCallback>,
    last_layout: String,
    keyboard_hook: Option<HhookWrapper>,
    thread_id: u32,
    start_time: Instant,
}

static HOOK_STATE: Mutex<Option<HookState>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
// Last event timestamp in ms since start (for debounce)
static LAST_EVENT_MS: AtomicU64 = AtomicU64::new(0);

/// Gets the current keyboard layout.
pub fn get_current_layout() -> LayoutInfo {
    unsafe {
        let hwnd = GetForegroundWindow();
        let thread_id = GetWindowThreadProcessId(hwnd, None);
        let hkl = GetKeyboardLayout(thread_id);
        let lang_id = (hkl.0 as u32) & 0xFFFF;

        let (name, is_russian) = match lang_id {
            LANG_EN_US => ("EN".to_string(), false),
            LANG_RU => ("RU".to_string(), true),
            _ => (format!("{:X}", lang_id), false),
        };

        LayoutInfo { name, is_russian }
    }
}

/// Checks for layout change and calls callback if changed.
/// Returns true if callback was called.
fn check_layout_change() -> bool {
    let layout = get_current_layout();

    // Get callback outside of lock to avoid holding lock during callback
    let callback = {
        let mut state = HOOK_STATE.lock();
        if let Some(ref mut s) = *state {
            if layout.name != s.last_layout {
                log::debug!("Layout: {} -> {}", s.last_layout, layout.name);
                s.last_layout = layout.name.clone();
                s.callback.clone()
            } else {
                None
            }
        } else {
            None
        }
    };

    // Call callback outside of lock
    if let Some(cb) = callback {
        cb(layout);
        true
    } else {
        false
    }
}

/// Checks layout with debounce - ignores rapid events.
fn check_layout_change_debounced() {
    // Get current time in ms
    let now_ms = {
        let state = HOOK_STATE.lock();
        if let Some(ref s) = *state {
            s.start_time.elapsed().as_millis() as u64
        } else {
            return;
        }
    };

    // Check debounce
    let last = LAST_EVENT_MS.load(Ordering::SeqCst);
    if now_ms.saturating_sub(last) < DEBOUNCE_MS {
        log::trace!(
            "Debounced event ({}ms since last)",
            now_ms.saturating_sub(last)
        );
        return;
    }

    // Update last event time
    LAST_EVENT_MS.store(now_ms, Ordering::SeqCst);

    check_layout_change();
}

/// Checks if a virtual key code is a modifier key.
fn is_modifier_key(vk_code: u32) -> bool {
    matches!(
        vk_code,
        VK_SHIFT
            | VK_CONTROL
            | VK_MENU
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
    )
}

/// Low-level keyboard hook callback.
unsafe extern "system" fn keyboard_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
        let vk_code = kb.vkCode;

        // Check for modifier key release (layout changes after keyup)
        if is_modifier_key(vk_code)
            && (w_param.0 == WM_KEYUP as usize || w_param.0 == WM_SYSKEYUP as usize)
        {
            // Schedule a delayed layout check in separate thread
            thread::spawn(|| {
                thread::sleep(std::time::Duration::from_millis(50));
                check_layout_change_debounced();
            });
        }
    }

    let hook = HOOK_STATE
        .lock()
        .as_ref()
        .and_then(|s| s.keyboard_hook)
        .map(|h| h.as_hhook())
        .unwrap_or_default();
    CallNextHookEx(hook, n_code, w_param, l_param)
}

/// WinEvent hook callback for window focus changes.
unsafe extern "system" fn win_event_proc(
    _h_win_event_hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    check_layout_change_debounced();
}

/// Keyboard layout hook manager.
pub struct KeyboardLayoutHook {
    thread: Option<JoinHandle<()>>,
}

impl KeyboardLayoutHook {
    /// Creates a new keyboard layout hook.
    ///
    /// Pass the initial layout to prevent false triggering on startup.
    pub fn new(callback: LayoutCallback, initial_layout: &str) -> Self {
        {
            let mut state = HOOK_STATE.lock();
            *state = Some(HookState {
                callback: Some(callback),
                last_layout: initial_layout.to_string(),
                keyboard_hook: None,
                thread_id: 0,
                start_time: Instant::now(),
            });
        }

        Self { thread: None }
    }

    /// Starts the hook in a separate thread.
    pub fn start(&mut self) {
        if RUNNING.load(Ordering::SeqCst) {
            return;
        }

        RUNNING.store(true, Ordering::SeqCst);

        let thread = thread::spawn(|| {
            message_loop();
        });

        self.thread = Some(thread);
    }

    /// Stops the hook.
    pub fn stop(&mut self) {
        if !RUNNING.load(Ordering::SeqCst) {
            return;
        }

        RUNNING.store(false, Ordering::SeqCst);

        // Post WM_QUIT to exit the message loop
        let thread_id = HOOK_THREAD_ID.load(Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }

        // Clear state
        let mut state = HOOK_STATE.lock();
        *state = None;
    }
}

impl Drop for KeyboardLayoutHook {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Message loop for the hook thread.
fn message_loop() {
    unsafe {
        let thread_id = GetCurrentThreadId();
        HOOK_THREAD_ID.store(thread_id, Ordering::SeqCst);

        // Update state with thread ID
        {
            let mut state = HOOK_STATE.lock();
            if let Some(ref mut s) = *state {
                s.thread_id = thread_id;
            }
        }

        // Set up WinEvent hook for foreground window changes only
        // (removed EVENT_OBJECT_FOCUS - too noisy)
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

        let hook_foreground = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_proc),
            0,
            0,
            flags,
        );

        // Set up low-level keyboard hook
        let keyboard_hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            HINSTANCE::default(),
            0,
        );

        if let Ok(hook) = keyboard_hook {
            let mut state = HOOK_STATE.lock();
            if let Some(ref mut s) = *state {
                s.keyboard_hook = Some(HhookWrapper::new(hook));
            }
        }

        // Check initial layout (no debounce for initial)
        check_layout_change();

        // Message loop
        let mut msg = MSG::default();
        while RUNNING.load(Ordering::SeqCst) {
            let result = GetMessageW(&mut msg, HWND::default(), 0, 0);
            if result.0 == 0 || result.0 == -1 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup hooks
        {
            let mut state = HOOK_STATE.lock();
            if let Some(ref mut s) = *state {
                if let Some(hook) = s.keyboard_hook.take() {
                    let _ = UnhookWindowsHookEx(hook.as_hhook());
                }
            }
        }

        if !hook_foreground.is_invalid() {
            let _ = UnhookWinEvent(hook_foreground);
        }
    }
}
