//! LangTip - Keyboard layout indicator for Windows.

// Hide console window in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod config;
mod hotkeys;
mod indicator;
mod keyboard_hook;
mod monitors;
mod single_instance;
mod sound;
mod tray;

use config::{AppConfig, ConfigManager};
use hotkeys::HotkeyManager;
use indicator::{get_enabled_positions, IndicatorWindow};
use keyboard_hook::{get_current_layout, KeyboardLayoutHook, LayoutInfo};
use monitors::get_monitors;
use single_instance::{is_already_running, release_mutex, show_already_running_message};
use sound::play_layout_sound;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tray::TrayIconManager;
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE},
};

// Global flags
static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);
static VISIBLE: AtomicBool = AtomicBool::new(true);

// Config check interval
const CONFIG_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Creates indicator windows based on config.
fn create_indicators(config: &AppConfig) -> Vec<IndicatorWindow> {
    let monitors = get_monitors();
    let positions = get_enabled_positions(config);
    let mut indicators = Vec::new();

    for monitor in &monitors {
        for position in &positions {
            if let Some(window) = IndicatorWindow::new(*position, config, monitor.clone()) {
                indicators.push(window);
            }
        }
    }

    indicators
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("LangTip starting...");

    if is_already_running() {
        log::warn!("Another instance is already running");
        show_already_running_message();
        return;
    }

    let config_manager = ConfigManager::new();
    let mut config = config_manager.load();
    log::info!("Configuration loaded from {:?}", config_manager.path());

    // Create indicator windows
    let mut indicators = create_indicators(&config);
    log::info!("Created {} indicator windows", indicators.len());

    // Track config file modification time for hot reload
    let mut last_config_mtime = config_manager.get_modified_time();
    let mut last_config_check = Instant::now();

    // Channel for layout change events (from hook thread to main thread)
    let (layout_tx, layout_rx): (mpsc::Sender<LayoutInfo>, Receiver<LayoutInfo>) = mpsc::channel();

    // Create tray icon
    let mut tray = TrayIconManager::new();

    tray.set_callbacks(
        Arc::new(move || {
            VISIBLE.store(true, Ordering::SeqCst);
            // Note: actual show will happen in main loop
            log::debug!("Tray: show requested");
        }),
        Arc::new(move || {
            VISIBLE.store(false, Ordering::SeqCst);
            log::debug!("Tray: hide requested");
        }),
        Arc::new(move || {
            SHOULD_EXIT.store(true, Ordering::SeqCst);
        }),
    );

    if let Err(e) = tray.start() {
        log::error!("Failed to start tray icon: {}", e);
    }

    // Get initial layout BEFORE creating hook to prevent false trigger
    let initial_layout = get_current_layout();
    log::info!("Initial layout: {}", initial_layout.name);

    // Set up keyboard layout hook - callback just sends to channel
    let layout_callback = Arc::new(move |layout: LayoutInfo| {
        log::debug!("Hook callback: {}", layout.name);
        if let Err(e) = layout_tx.send(layout) {
            log::error!("Failed to send layout event: {}", e);
        }
    });

    let mut keyboard_hook = KeyboardLayoutHook::new(layout_callback, &initial_layout.name);
    keyboard_hook.start();
    log::info!("Keyboard hook started");

    // Set up hotkey manager
    let mut hotkey_manager = HotkeyManager::new(config.hotkeys.clone());
    hotkey_manager.set_callbacks(
        Arc::new(move || {
            let current = VISIBLE.load(Ordering::SeqCst);
            VISIBLE.store(!current, Ordering::SeqCst);
            log::debug!("Hotkey: toggle visibility -> {}", !current);
        }),
        Arc::new(move || {
            SHOULD_EXIT.store(true, Ordering::SeqCst);
        }),
    );
    hotkey_manager.start();
    log::info!("Hotkey manager started");

    // Show initial indicators
    let mut last_layout = initial_layout.name.clone();
    let mut last_show_time = Instant::now();
    let mut last_hide_time = Instant::now() - Duration::from_secs(10); // Long ago
    let mut indicators_shown = true; // Track if indicators are currently shown
    let hide_cooldown = Duration::from_millis(500); // Ignore events for 500ms after hide

    for indicator in &indicators {
        indicator.update_text(&initial_layout.name, initial_layout.is_russian);
        indicator.show();
    }

    log::info!("LangTip running");

    // Main message loop
    let mut hide_delay = Duration::from_millis(config.hide_delay_ms as u64);
    let mut config_sound = config.sound.clone();
    let mut msg = MSG::default();
    let mut was_visible = VISIBLE.load(Ordering::SeqCst);

    loop {
        if SHOULD_EXIT.load(Ordering::SeqCst) {
            break;
        }

        tray.process_menu_events();

        // Process layout change events from hook thread
        match layout_rx.try_recv() {
            Ok(layout) => {
                // Ignore events during cooldown after hide (prevents false triggers)
                if last_hide_time.elapsed() < hide_cooldown {
                    log::debug!("Ignoring event during hide cooldown: {}", layout.name);
                    continue;
                }

                log::debug!(
                    "Received layout event: {}, current: {}",
                    layout.name,
                    last_layout
                );
                if layout.name != last_layout {
                    log::info!("Layout: {} -> {}", last_layout, layout.name);
                    last_layout = layout.name.clone();
                    last_show_time = Instant::now();
                    log::debug!("Timer reset");

                    // Update indicators (from main thread - correct!)
                    for indicator in &indicators {
                        indicator.update_text(&layout.name, layout.is_russian);
                    }

                    // Play sound
                    play_layout_sound(layout.is_russian, &config_sound);

                    // Show indicators
                    if VISIBLE.load(Ordering::SeqCst) {
                        for indicator in &indicators {
                            indicator.show();
                        }
                        indicators_shown = true;
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                log::error!("Layout channel disconnected");
                break;
            }
        }

        // Handle visibility toggle from hotkey/tray
        let is_visible = VISIBLE.load(Ordering::SeqCst);
        if is_visible != was_visible {
            if is_visible {
                for indicator in &indicators {
                    indicator.show();
                }
                last_show_time = Instant::now();
                indicators_shown = true;
            } else {
                for indicator in &indicators {
                    indicator.hide();
                }
                indicators_shown = false;
                last_hide_time = Instant::now(); // Start cooldown
            }
            was_visible = is_visible;
        }

        // Auto-hide check - only hide once
        if indicators_shown && last_show_time.elapsed() >= hide_delay {
            log::debug!(
                "Auto-hide triggered after {}ms",
                last_show_time.elapsed().as_millis()
            );
            for indicator in &indicators {
                indicator.hide();
            }
            indicators_shown = false;
            last_hide_time = Instant::now(); // Start cooldown
        }

        // Update fade animations
        for indicator in &indicators {
            indicator.update_fade();
        }

        // Check for config file changes (hot reload)
        if last_config_check.elapsed() >= CONFIG_CHECK_INTERVAL {
            last_config_check = Instant::now();

            if let Some(current_mtime) = config_manager.get_modified_time() {
                let config_changed = match last_config_mtime {
                    Some(last) => current_mtime != last,
                    None => true,
                };

                if config_changed {
                    log::info!("Config file changed, reloading...");
                    last_config_mtime = Some(current_mtime);

                    // Reload config
                    config = config_manager.load();

                    // Update derived values
                    hide_delay = Duration::from_millis(config.hide_delay_ms as u64);
                    config_sound = config.sound.clone();

                    // Recreate indicators with new config
                    drop(indicators); // Destroy old windows
                    indicators = create_indicators(&config);

                    // Update with current layout and show
                    let current_layout = get_current_layout();
                    last_layout = current_layout.name.clone();
                    for indicator in &indicators {
                        indicator.update_text(&current_layout.name, current_layout.is_russian);
                        if VISIBLE.load(Ordering::SeqCst) {
                            indicator.show();
                        }
                    }
                    indicators_shown = VISIBLE.load(Ordering::SeqCst);
                    last_show_time = Instant::now();

                    log::info!("Config reloaded, {} indicators recreated", indicators.len());
                }
            }
        }

        // Process Windows messages
        unsafe {
            if PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                std::thread::sleep(Duration::from_millis(16));
            }
        }
    }

    log::info!("LangTip shutting down...");

    keyboard_hook.stop();
    hotkey_manager.stop();
    tray.stop();
    release_mutex();

    log::info!("LangTip stopped");
}
