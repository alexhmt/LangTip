//! Indicator window module.
//!
//! Creates and manages indicator windows that display the current keyboard layout.

use crate::config::{parse_hex_color, AppConfig};
use crate::monitors::MonitorInfo;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HFONT,
            InvalidateRect, SelectObject, SetBkMode, SetTextColor, TextOutW, PAINTSTRUCT,
            TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW,
            IsWindow, RegisterClassW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
            ShowWindow, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, LWA_ALPHA,
            LWA_COLORKEY, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WM_DESTROY, WM_PAINT,
            WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            WS_POPUP,
        },
    },
};

/// Position of indicator on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

/// Class name for indicator windows (null-terminated UTF-16).
static CLASS_NAME_W: &[u16] = &[
    'L' as u16, 'a' as u16, 'y' as u16, 'o' as u16, 'u' as u16, 't' as u16, 'I' as u16, 'n' as u16,
    'd' as u16, 'i' as u16, 'c' as u16, 'a' as u16, 't' as u16, 'o' as u16, 'r' as u16, 0,
];
static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Window state stored in GWLP_USERDATA.
struct WindowState {
    text: String,
    is_russian: bool,
    #[allow(dead_code)]
    font_size: u32,
    color_en: (u8, u8, u8),
    color_ru: (u8, u8, u8),
    font: HFONT,
}

/// Registers the window class.
fn register_class() -> bool {
    if CLASS_REGISTERED.load(Ordering::SeqCst) {
        return true;
    }

    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(CLASS_NAME_W.as_ptr()),
            ..Default::default()
        };

        if RegisterClassW(&wc) != 0 {
            CLASS_REGISTERED.store(true, Ordering::SeqCst);
            true
        } else {
            log::error!("RegisterClassW failed");
            false
        }
    }
}

/// Window procedure for indicator windows.
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            log::debug!("WM_PAINT for hwnd {:?}", hwnd.0);
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            // Get window state from GWLP_USERDATA
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let state = &*(ptr as *const WindowState);

                // Clear background with black (will be transparent due to LWA_COLORKEY)
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let black_brush = CreateSolidBrush(COLORREF(0)); // Black = transparent
                FillRect(hdc, &rect, black_brush);
                let _ = DeleteObject(black_brush);

                // Set transparent background for text
                let _ = SetBkMode(hdc, TRANSPARENT);

                // Use cached font
                let old_font = SelectObject(hdc, state.font);

                // Set text color
                let (r, g, b) = if state.is_russian {
                    state.color_ru
                } else {
                    state.color_en
                };
                SetTextColor(
                    hdc,
                    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16)),
                );

                // Draw text
                let text_wide: Vec<u16> = state.text.encode_utf16().collect();
                let _ = TextOutW(hdc, 10, 5, &text_wide);

                // Restore old font (don't delete cached font)
                SelectObject(hdc, old_font);
            }

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            // Cleanup: get state pointer, delete font, free memory
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let state = Box::from_raw(ptr as *mut WindowState);
                let _ = DeleteObject(state.font);
                // state is dropped here, freeing memory
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// A wrapper around HWND that is Send + Sync.
/// Safety: We only use the handle from the main thread and for atomic operations.
#[derive(Debug)]
struct HwndWrapper(isize);

// SAFETY: The handle is only used for window operations which are thread-safe
// when properly synchronized (which we do through the Mutex).
unsafe impl Send for HwndWrapper {}
unsafe impl Sync for HwndWrapper {}

impl HwndWrapper {
    fn new(hwnd: HWND) -> Self {
        Self(hwnd.0 as isize)
    }

    fn as_hwnd(&self) -> HWND {
        HWND(self.0 as *mut std::ffi::c_void)
    }

    #[allow(dead_code)]
    fn raw(&self) -> isize {
        self.0
    }
}

/// Fade animation step size per update (higher = faster fade).
const FADE_STEP: u8 = 25;

/// A single indicator window.
pub struct IndicatorWindow {
    hwnd: HwndWrapper,
    #[allow(dead_code)]
    position: Position,
    #[allow(dead_code)]
    monitor: MonitorInfo,
    #[allow(dead_code)]
    font_size: u32,
    /// Maximum alpha value derived from config opacity (0–255).
    max_alpha: u8,
    alpha: AtomicU8,
    target_alpha: AtomicU8,
}

// SAFETY: IndicatorWindow operations on HWND are thread-safe when properly synchronized
unsafe impl Send for IndicatorWindow {}
unsafe impl Sync for IndicatorWindow {}

impl IndicatorWindow {
    /// Creates a new indicator window.
    pub fn new(position: Position, config: &AppConfig, monitor: MonitorInfo) -> Option<Self> {
        if !register_class() {
            log::error!("Failed to register window class");
            return None;
        }

        let is_center = position == Position::Center;
        let font_size = if is_center {
            config.font_size_center
        } else {
            config.font_size_corner
        };

        // Calculate window size based on font size
        let width = (font_size * 3) as i32;
        let height = (font_size as f32 * 1.5) as i32;

        // Calculate position
        let margin = config.margin;
        let (x, y) = calculate_position(position, &monitor, width, height, margin);

        // Convert opacity percent (0–100) to alpha byte (0–255)
        let max_alpha = ((config.opacity.min(100) as f32 / 100.0) * 255.0) as u8;

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();

            // WS_EX_TRANSPARENT makes window click-through (mouse events pass to windows below)
            let hwnd_result = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
                PCWSTR(CLASS_NAME_W.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                x,
                y,
                width,
                height,
                HWND::default(),
                None,
                hinstance,
                None,
            );

            let hwnd = match hwnd_result {
                Ok(h) => h,
                Err(_) => {
                    log::error!("Failed to create window");
                    return None;
                }
            };

            if hwnd.0.is_null() {
                log::error!("Failed to create window - null handle");
                return None;
            }

            log::debug!(
                "Created window hwnd {:?} at ({}, {}) size {}x{}",
                hwnd.0,
                x,
                y,
                width,
                height
            );

            // Set color key: black (0x000000) = transparent, plus alpha for fade effects
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY | LWA_ALPHA);

            // Make window topmost
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);

            // Create window state
            let color_en = parse_hex_color(&config.colors.en);
            let color_ru = parse_hex_color(&config.colors.ru);

            // Create cached font
            let font_name: Vec<u16> = config
                .font_family
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let font = CreateFontW(
                font_size as i32,
                0,
                0,
                0,
                700, // FW_BOLD
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                PCWSTR(font_name.as_ptr()),
            );

            // Store state in GWLP_USERDATA
            let state = Box::new(WindowState {
                text: "EN".to_string(),
                is_russian: false,
                font_size,
                color_en,
                color_ru,
                font,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

            Some(Self {
                hwnd: HwndWrapper::new(hwnd),
                position,
                monitor,
                font_size,
                max_alpha,
                alpha: AtomicU8::new(0),
                target_alpha: AtomicU8::new(0),
            })
        }
    }

    /// Updates the indicator text.
    pub fn update_text(&self, text: &str, is_russian: bool) {
        unsafe {
            let ptr = GetWindowLongPtrW(self.hwnd.as_hwnd(), GWLP_USERDATA);
            if ptr != 0 {
                let state = &mut *(ptr as *mut WindowState);
                state.text = text.to_string();
                state.is_russian = is_russian;
            }

            // Trigger repaint
            let _ = InvalidateRect(self.hwnd.as_hwnd(), None, true);
        }
    }

    /// Shows the window with fade-in animation.
    /// Call `update_fade()` repeatedly to animate.
    pub fn show(&self) {
        self.target_alpha.store(self.max_alpha, Ordering::SeqCst);
        unsafe {
            let hwnd = self.hwnd.as_hwnd();
            log::debug!("show() hwnd={:?}", hwnd.0);

            // Show window
            let _ = ShowWindow(hwnd, SW_SHOW);

            // Bring to top
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);

            // Force repaint
            let _ = InvalidateRect(hwnd, None, true);
        }
    }

    /// Hides the window with fade-out animation.
    /// Call `update_fade()` repeatedly to animate. Window hides when alpha reaches 0.
    pub fn hide(&self) {
        self.target_alpha.store(0, Ordering::SeqCst);
        log::debug!("hide() hwnd={:?}", self.hwnd.as_hwnd().0);
    }

    /// Updates the fade animation. Returns true if animation is still in progress.
    /// Should be called from the main loop (~60fps).
    pub fn update_fade(&self) -> bool {
        let current = self.alpha.load(Ordering::SeqCst);
        let target = self.target_alpha.load(Ordering::SeqCst);

        if current == target {
            return false; // Animation complete
        }

        let new_alpha = if current < target {
            // Fade in
            current.saturating_add(FADE_STEP).min(target)
        } else {
            // Fade out
            current.saturating_sub(FADE_STEP).max(target)
        };

        self.alpha.store(new_alpha, Ordering::SeqCst);

        unsafe {
            let hwnd = self.hwnd.as_hwnd();
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), new_alpha, LWA_COLORKEY | LWA_ALPHA);

            // Hide window completely when fully transparent
            if new_alpha == 0 {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }

        true // Animation in progress
    }

    /// Sets the alpha value directly (bypasses animation).
    #[allow(dead_code)]
    pub fn set_alpha(&self, alpha: u8) {
        let clamped = alpha.min(self.max_alpha);
        self.alpha.store(clamped, Ordering::SeqCst);
        self.target_alpha.store(clamped, Ordering::SeqCst);
        unsafe {
            let hwnd = self.hwnd.as_hwnd();
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), clamped, LWA_COLORKEY | LWA_ALPHA);
            if clamped > 0 {
                let _ = ShowWindow(hwnd, SW_SHOW);
            } else {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    /// Returns the current alpha value.
    #[allow(dead_code)]
    pub fn get_alpha(&self) -> u8 {
        self.alpha.load(Ordering::SeqCst)
    }

    /// Returns the target alpha value.
    #[allow(dead_code)]
    pub fn get_target_alpha(&self) -> u8 {
        self.target_alpha.load(Ordering::SeqCst)
    }

    /// Returns true if fade animation is in progress.
    #[allow(dead_code)]
    pub fn is_animating(&self) -> bool {
        self.alpha.load(Ordering::SeqCst) != self.target_alpha.load(Ordering::SeqCst)
    }

    /// Returns whether the window is valid.
    #[allow(dead_code)]
    pub fn is_valid(&self) -> bool {
        unsafe { IsWindow(self.hwnd.as_hwnd()).as_bool() }
    }
}

impl Drop for IndicatorWindow {
    fn drop(&mut self) {
        unsafe {
            let hwnd = self.hwnd.as_hwnd();
            if IsWindow(hwnd).as_bool() {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

/// Calculates the window position based on the position enum and monitor.
/// Uses work area (rcWork) which excludes taskbar and app bars.
fn calculate_position(
    position: Position,
    monitor: &MonitorInfo,
    width: i32,
    height: i32,
    margin: i32,
) -> (i32, i32) {
    match position {
        Position::TopLeft => (monitor.work_x + margin, monitor.work_y + margin),
        Position::TopRight => (
            monitor.work_x + monitor.work_width - width - margin,
            monitor.work_y + margin,
        ),
        Position::BottomLeft => (
            monitor.work_x + margin,
            monitor.work_y + monitor.work_height - height - margin,
        ),
        Position::BottomRight => (
            monitor.work_x + monitor.work_width - width - margin,
            monitor.work_y + monitor.work_height - height - margin,
        ),
        Position::Center => (
            monitor.work_x + (monitor.work_width - width) / 2,
            monitor.work_y + (monitor.work_height - height) / 2,
        ),
    }
}

/// Gets the enabled positions from config.
pub fn get_enabled_positions(config: &AppConfig) -> Vec<Position> {
    let mut positions = Vec::new();
    if config.positions.top_left {
        positions.push(Position::TopLeft);
    }
    if config.positions.top_right {
        positions.push(Position::TopRight);
    }
    if config.positions.bottom_left {
        positions.push(Position::BottomLeft);
    }
    if config.positions.bottom_right {
        positions.push(Position::BottomRight);
    }
    if config.positions.center {
        positions.push(Position::Center);
    }
    positions
}
