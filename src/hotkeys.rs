//! Global hotkeys module.
//!
//! Manages global hotkeys for the application using Windows API.

use crate::config::HotkeyConfig;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
    },
    UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_HOTKEY,
        WM_QUIT,
    },
};

/// Callback type for hotkeys.
pub type HotkeyCallback = Arc<dyn Fn() + Send + Sync>;

/// Hotkey ID constants.
const HOTKEY_TOGGLE: i32 = 1;
const HOTKEY_EXIT: i32 = 2;

/// Virtual key codes.
const VK_MAP: &[(&str, u32)] = &[
    ("a", 0x41),
    ("b", 0x42),
    ("c", 0x43),
    ("d", 0x44),
    ("e", 0x45),
    ("f", 0x46),
    ("g", 0x47),
    ("h", 0x48),
    ("i", 0x49),
    ("j", 0x4A),
    ("k", 0x4B),
    ("l", 0x4C),
    ("m", 0x4D),
    ("n", 0x4E),
    ("o", 0x4F),
    ("p", 0x50),
    ("q", 0x51),
    ("r", 0x52),
    ("s", 0x53),
    ("t", 0x54),
    ("u", 0x55),
    ("v", 0x56),
    ("w", 0x57),
    ("x", 0x58),
    ("y", 0x59),
    ("z", 0x5A),
    ("0", 0x30),
    ("1", 0x31),
    ("2", 0x32),
    ("3", 0x33),
    ("4", 0x34),
    ("5", 0x35),
    ("6", 0x36),
    ("7", 0x37),
    ("8", 0x38),
    ("9", 0x39),
    ("f1", 0x70),
    ("f2", 0x71),
    ("f3", 0x72),
    ("f4", 0x73),
    ("f5", 0x74),
    ("f6", 0x75),
    ("f7", 0x76),
    ("f8", 0x77),
    ("f9", 0x78),
    ("f10", 0x79),
    ("f11", 0x7A),
    ("f12", 0x7B),
    ("space", 0x20),
    ("enter", 0x0D),
    ("escape", 0x1B),
    ("tab", 0x09),
];

/// Parses a hotkey string like "ctrl+alt+l" into modifiers and virtual key.
fn parse_hotkey(hotkey: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let lower = hotkey.to_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    if parts.is_empty() {
        return None;
    }
    // Note: 'lower' must live long enough since 'parts' borrows from it

    let mut modifiers = HOT_KEY_MODIFIERS(0);
    let mut vk: Option<u32> = None;

    for part in parts {
        let part = part.trim();
        match part {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            key => {
                // Find virtual key code
                for (name, code) in VK_MAP {
                    if *name == key {
                        vk = Some(*code);
                        break;
                    }
                }
            }
        }
    }

    vk.map(|vk| (modifiers, vk))
}

/// Global state for hotkey manager.
struct HotkeyState {
    callbacks: HashMap<i32, HotkeyCallback>,
    registered_hotkeys: Vec<i32>,
}

static HOTKEY_STATE: Mutex<Option<HotkeyState>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);
static HOTKEY_THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// Hotkey manager.
pub struct HotkeyManager {
    config: HotkeyConfig,
    on_toggle: Option<HotkeyCallback>,
    on_exit: Option<HotkeyCallback>,
    thread: Option<JoinHandle<()>>,
}

impl HotkeyManager {
    /// Creates a new hotkey manager.
    pub fn new(config: HotkeyConfig) -> Self {
        Self {
            config,
            on_toggle: None,
            on_exit: None,
            thread: None,
        }
    }

    /// Sets callbacks for hotkeys.
    pub fn set_callbacks(&mut self, on_toggle: HotkeyCallback, on_exit: HotkeyCallback) {
        self.on_toggle = Some(on_toggle);
        self.on_exit = Some(on_exit);
    }

    /// Starts the hotkey manager.
    pub fn start(&mut self) {
        if !self.config.enabled || RUNNING.load(Ordering::SeqCst) {
            return;
        }

        let toggle_hotkey = self.config.toggle.clone();
        let exit_hotkey = self.config.exit.clone();
        let on_toggle = self.on_toggle.clone();
        let on_exit = self.on_exit.clone();

        // Initialize state
        {
            let mut state = HOTKEY_STATE.lock();
            let mut callbacks = HashMap::new();
            if let Some(cb) = on_toggle {
                callbacks.insert(HOTKEY_TOGGLE, cb);
            }
            if let Some(cb) = on_exit {
                callbacks.insert(HOTKEY_EXIT, cb);
            }
            *state = Some(HotkeyState {
                callbacks,
                registered_hotkeys: Vec::new(),
            });
        }

        RUNNING.store(true, Ordering::SeqCst);

        let thread = thread::spawn(move || {
            hotkey_message_loop(toggle_hotkey, exit_hotkey);
        });

        self.thread = Some(thread);
    }

    /// Stops the hotkey manager.
    pub fn stop(&mut self) {
        if !RUNNING.load(Ordering::SeqCst) {
            return;
        }

        RUNNING.store(false, Ordering::SeqCst);

        // Post WM_QUIT to exit the message loop
        let thread_id = HOTKEY_THREAD_ID.load(Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }

        // Clear state
        let mut state = HOTKEY_STATE.lock();
        *state = None;
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Message loop for hotkey processing.
fn hotkey_message_loop(toggle_hotkey: String, exit_hotkey: String) {
    unsafe {
        let thread_id = GetCurrentThreadId();
        HOTKEY_THREAD_ID.store(thread_id, Ordering::SeqCst);

        // Register hotkeys
        let mut registered = Vec::new();

        if let Some((modifiers, vk)) = parse_hotkey(&toggle_hotkey) {
            if RegisterHotKey(HWND::default(), HOTKEY_TOGGLE, modifiers, vk).is_ok() {
                registered.push(HOTKEY_TOGGLE);
                log::debug!("Registered toggle hotkey: {}", toggle_hotkey);
            } else {
                log::warn!("Failed to register toggle hotkey: {}", toggle_hotkey);
            }
        }

        if let Some((modifiers, vk)) = parse_hotkey(&exit_hotkey) {
            if RegisterHotKey(HWND::default(), HOTKEY_EXIT, modifiers, vk).is_ok() {
                registered.push(HOTKEY_EXIT);
                log::debug!("Registered exit hotkey: {}", exit_hotkey);
            } else {
                log::warn!("Failed to register exit hotkey: {}", exit_hotkey);
            }
        }

        // Update state with registered hotkeys
        {
            let mut state = HOTKEY_STATE.lock();
            if let Some(ref mut s) = *state {
                s.registered_hotkeys = registered;
            }
        }

        // Message loop
        let mut msg = MSG::default();
        while RUNNING.load(Ordering::SeqCst) {
            let result = GetMessageW(&mut msg, HWND::default(), 0, 0);
            if result.0 == 0 || result.0 == -1 {
                break;
            }

            if msg.message == WM_HOTKEY {
                let hotkey_id = msg.wParam.0 as i32;
                let callback = {
                    let state = HOTKEY_STATE.lock();
                    state
                        .as_ref()
                        .and_then(|s| s.callbacks.get(&hotkey_id).cloned())
                };
                if let Some(cb) = callback {
                    cb();
                }
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Unregister hotkeys
        {
            let state = HOTKEY_STATE.lock();
            if let Some(ref s) = *state {
                for hotkey_id in &s.registered_hotkeys {
                    let _ = UnregisterHotKey(HWND::default(), *hotkey_id);
                }
            }
        }
    }
}
