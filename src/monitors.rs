//! Monitor information module.
//!
//! Provides functions to get information about connected monitors
//! using Windows API (EnumDisplayMonitors).

use std::mem;
use windows::Win32::{
    Foundation::{BOOL, LPARAM, RECT},
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
};

/// MONITORINFOF_PRIMARY constant (equals 1).
const MONITORINFOF_PRIMARY: u32 = 1;

/// Information about a monitor.
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// X coordinate of the top-left corner (full monitor area).
    pub x: i32,
    /// Y coordinate of the top-left corner (full monitor area).
    pub y: i32,
    /// Monitor width in pixels (full monitor area).
    pub width: i32,
    /// Monitor height in pixels (full monitor area).
    pub height: i32,
    /// X coordinate of the work area (excludes taskbar and app bars).
    pub work_x: i32,
    /// Y coordinate of the work area (excludes taskbar and app bars).
    pub work_y: i32,
    /// Work area width in pixels.
    pub work_width: i32,
    /// Work area height in pixels.
    pub work_height: i32,
    /// Whether this is the primary monitor.
    pub is_primary: bool,
}

impl MonitorInfo {
    /// X coordinate of the right edge.
    #[allow(dead_code)]
    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    /// Y coordinate of the bottom edge.
    #[allow(dead_code)]
    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }
}

/// Gets a list of all connected monitors.
pub fn get_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();

    unsafe extern "system" fn callback(
        h_monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(lparam.0 as *mut Vec<MonitorInfo>);

        let mut mi = MONITORINFO {
            cbSize: mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        if GetMonitorInfoW(h_monitor, &mut mi).as_bool() {
            let rect = mi.rcMonitor;
            let work = mi.rcWork;
            monitors.push(MonitorInfo {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                work_x: work.left,
                work_y: work.top,
                work_width: work.right - work.left,
                work_height: work.bottom - work.top,
                is_primary: (mi.dwFlags & MONITORINFOF_PRIMARY) != 0,
            });
        }

        BOOL::from(true)
    }

    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(callback),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }

    // Sort: primary monitor first
    monitors.sort_by_key(|m| (!m.is_primary, m.x, m.y));

    monitors
}

/// Gets information about the primary monitor.
#[allow(dead_code)]
pub fn get_primary_monitor() -> Option<MonitorInfo> {
    get_monitors().into_iter().find(|m| m.is_primary)
}

/// Gets the number of connected monitors.
#[allow(dead_code)]
pub fn get_monitor_count() -> usize {
    get_monitors().len()
}
