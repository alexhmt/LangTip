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

use config::ConfigManager;
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

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("LangTip starting...");

    if is_already_running() {
        log::warn!("Another instance is already running");
        show_already_running_message();
        return;
    }

    let config_manager = ConfigManager::new();
    let config = config_manager.load();
    log::info!("Configuration loaded");

    // Create indicator windows
    let monitors = get_monitors();
    log::info!("Found {} monitors", monitors.len());

    let positions = get_enabled_positions(&config);
    let mut indicators: Vec<IndicatorWindow> = Vec::new();

    for monitor in &monitors {
        for position in &positions {
            if let Some(window) = IndicatorWindow::new(*position, &config, monitor.clone()) {
                indicators.push(window);
            }
        }
    }

    log::info!("Created {} indicator windows", indicators.len());

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
    let hide_delay = Duration::from_millis(config.hide_delay_ms as u64);
    let config_sound = config.sound.clone();
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
