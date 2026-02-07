//! System tray module.
//!
//! Provides system tray icon with context menu for the application.

use crate::autostart::{disable_autostart, enable_autostart, is_autostart_enabled};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use windows::core::w;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

/// Callback type for tray actions.
pub type TrayCallback = Arc<dyn Fn() + Send + Sync>;

/// Tray icon manager.
pub struct TrayIconManager {
    tray_icon: Option<TrayIcon>,
    on_show: Option<TrayCallback>,
    on_hide: Option<TrayCallback>,
    on_exit: Option<TrayCallback>,
    visible: AtomicBool,
    menu_show_id: Option<tray_icon::menu::MenuId>,
    menu_hide_id: Option<tray_icon::menu::MenuId>,
    menu_autostart_id: Option<tray_icon::menu::MenuId>,
    menu_autostart_item: Option<CheckMenuItem>,
    menu_about_id: Option<tray_icon::menu::MenuId>,
    menu_exit_id: Option<tray_icon::menu::MenuId>,
}

impl TrayIconManager {
    /// Creates a new tray icon manager.
    pub fn new() -> Self {
        Self {
            tray_icon: None,
            on_show: None,
            on_hide: None,
            on_exit: None,
            visible: AtomicBool::new(true),
            menu_show_id: None,
            menu_hide_id: None,
            menu_autostart_id: None,
            menu_autostart_item: None,
            menu_about_id: None,
            menu_exit_id: None,
        }
    }

    /// Sets callbacks for tray actions.
    pub fn set_callbacks(
        &mut self,
        on_show: TrayCallback,
        on_hide: TrayCallback,
        on_exit: TrayCallback,
    ) {
        self.on_show = Some(on_show);
        self.on_hide = Some(on_hide);
        self.on_exit = Some(on_exit);
    }

    /// Creates the tray icon image.
    fn create_icon() -> Icon {
        // Create a simple 32x32 icon with "EN" text
        // Using RGBA format
        let size = 32u32;
        let mut rgba = vec![0u8; (size * size * 4) as usize];

        // Fill with green color (#55FF55) for EN indicator
        for y in 0..size {
            for x in 0..size {
                let idx = ((y * size + x) * 4) as usize;
                // Create a circle
                let cx = size as f32 / 2.0;
                let cy = size as f32 / 2.0;
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                let radius = size as f32 / 2.0 - 2.0;

                if dist <= radius {
                    // Green color
                    rgba[idx] = 0x55; // R
                    rgba[idx + 1] = 0xFF; // G
                    rgba[idx + 2] = 0x55; // B
                    rgba[idx + 3] = 255; // A
                } else {
                    // Transparent
                    rgba[idx] = 0;
                    rgba[idx + 1] = 0;
                    rgba[idx + 2] = 0;
                    rgba[idx + 3] = 0;
                }
            }
        }

        Icon::from_rgba(rgba, size, size).expect("Failed to create tray icon")
    }

    /// Starts the tray icon.
    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Create menu items
        let menu_show = MenuItem::new("Show indicators", true, None);
        let menu_hide = MenuItem::new("Hide indicators", true, None);
        let menu_autostart = CheckMenuItem::new("Autostart", true, is_autostart_enabled(), None);
        let menu_about = MenuItem::new("About", true, None);
        let menu_exit = MenuItem::new("Exit", true, None);

        // Store menu IDs and items
        self.menu_show_id = Some(menu_show.id().clone());
        self.menu_hide_id = Some(menu_hide.id().clone());
        self.menu_autostart_id = Some(menu_autostart.id().clone());
        self.menu_autostart_item = Some(menu_autostart.clone());
        self.menu_about_id = Some(menu_about.id().clone());
        self.menu_exit_id = Some(menu_exit.id().clone());

        // Create menu
        let menu = Menu::new();
        menu.append(&menu_show)?;
        menu.append(&menu_hide)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&menu_autostart)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&menu_about)?;
        menu.append(&menu_exit)?;

        // Create tray icon
        let icon = Self::create_icon();
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("Layout Indicator")
            .with_menu(Box::new(menu))
            .build()?;

        self.tray_icon = Some(tray);

        Ok(())
    }

    /// Processes menu events. Should be called from the main event loop.
    pub fn process_menu_events(&self) {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if Some(&event.id) == self.menu_show_id.as_ref() {
                self.visible.store(true, Ordering::SeqCst);
                if let Some(ref cb) = self.on_show {
                    cb();
                }
            } else if Some(&event.id) == self.menu_hide_id.as_ref() {
                self.visible.store(false, Ordering::SeqCst);
                if let Some(ref cb) = self.on_hide {
                    cb();
                }
            } else if Some(&event.id) == self.menu_autostart_id.as_ref() {
                // Toggle autostart and update checkbox
                if is_autostart_enabled() {
                    disable_autostart();
                    if let Some(ref item) = self.menu_autostart_item {
                        item.set_checked(false);
                    }
                } else {
                    enable_autostart();
                    if let Some(ref item) = self.menu_autostart_item {
                        item.set_checked(true);
                    }
                }
            } else if Some(&event.id) == self.menu_about_id.as_ref() {
                show_about_dialog();
            } else if Some(&event.id) == self.menu_exit_id.as_ref() {
                if let Some(ref cb) = self.on_exit {
                    cb();
                }
            }
        }
    }

    /// Returns whether indicators are visible.
    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    /// Sets visibility state.
    #[allow(dead_code)]
    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
    }

    /// Stops the tray icon.
    pub fn stop(&mut self) {
        self.tray_icon = None;
    }
}

/// Shows the About dialog with application information.
fn show_about_dialog() {
    let version = env!("CARGO_PKG_VERSION");
    let text = format!(
        "LangTip v{version}\n\nKeyboard layout indicator for Windows\n\nhttps://github.com/alexhmt/LangTip"
    );
    let text_wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let text_pcwstr = windows::core::PCWSTR(text_wide.as_ptr());

    unsafe {
        let _ = MessageBoxW(None, text_pcwstr, w!("About LangTip"), MB_OK | MB_ICONINFORMATION);
    }
}

impl Default for TrayIconManager {
    fn default() -> Self {
        Self::new()
    }
}
