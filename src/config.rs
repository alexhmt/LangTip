//! Configuration module for the layout indicator.
//!
//! Handles loading and saving application settings from JSON file.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// Sound configuration for layout change notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundConfig {
    /// Whether sound is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Frequency for English layout (Hz).
    #[serde(default = "default_freq_en")]
    pub frequency_en: u32,
    /// Frequency for Russian layout (Hz).
    #[serde(default = "default_freq_ru")]
    pub frequency_ru: u32,
    /// Duration in milliseconds.
    #[serde(default = "default_duration")]
    pub duration_ms: u32,
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency_en: 800,
            frequency_ru: 600,
            duration_ms: 50,
        }
    }
}

/// Hotkey configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Whether hotkeys are enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Toggle visibility hotkey (e.g., "ctrl+alt+l").
    #[serde(default = "default_toggle")]
    pub toggle: String,
    /// Exit hotkey (e.g., "ctrl+alt+q").
    #[serde(default = "default_exit")]
    pub exit: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            toggle: "ctrl+alt+l".to_string(),
            exit: "ctrl+alt+q".to_string(),
        }
    }
}

/// Colors configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorsConfig {
    /// Color for English layout (hex).
    #[serde(default = "default_color_en")]
    pub en: String,
    /// Color for Russian layout (hex).
    #[serde(default = "default_color_ru")]
    pub ru: String,
}

impl Default for ColorsConfig {
    fn default() -> Self {
        Self {
            en: "#55FF55".to_string(),
            ru: "#FF5555".to_string(),
        }
    }
}

/// Position visibility configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionsConfig {
    #[serde(default = "default_true")]
    pub top_left: bool,
    #[serde(default = "default_true")]
    pub top_right: bool,
    #[serde(default = "default_true")]
    pub bottom_left: bool,
    #[serde(default = "default_true")]
    pub bottom_right: bool,
    #[serde(default = "default_true")]
    pub center: bool,
}

impl Default for PositionsConfig {
    fn default() -> Self {
        Self {
            top_left: true,
            top_right: true,
            bottom_left: true,
            bottom_right: true,
            center: true,
        }
    }
}

/// Fade animation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FadeConfig {
    /// Fade duration in milliseconds.
    #[serde(default = "default_fade_duration")]
    pub duration_ms: u32,
    /// Number of animation steps.
    #[serde(default = "default_fade_steps")]
    pub steps: u32,
}

impl Default for FadeConfig {
    fn default() -> Self {
        Self {
            duration_ms: 200,
            steps: 10,
        }
    }
}

/// Main application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Font size for corner indicators.
    #[serde(default = "default_font_size_corner")]
    pub font_size_corner: u32,
    /// Font size for center indicator.
    #[serde(default = "default_font_size_center")]
    pub font_size_center: u32,
    /// Font family.
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// Update delay in milliseconds.
    #[serde(default = "default_update_delay")]
    pub update_delay_ms: u32,
    /// Hide delay in milliseconds.
    #[serde(default = "default_hide_delay")]
    pub hide_delay_ms: u32,
    /// Margin from screen edges.
    #[serde(default = "default_margin")]
    pub margin: i32,
    /// Colors configuration.
    #[serde(default)]
    pub colors: ColorsConfig,
    /// Positions configuration.
    #[serde(default)]
    pub positions: PositionsConfig,
    /// Fade animation configuration.
    #[serde(default)]
    pub fade: FadeConfig,
    /// Sound configuration.
    #[serde(default)]
    pub sound: SoundConfig,
    /// Hotkeys configuration.
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            font_size_corner: 32,
            font_size_center: 64,
            font_family: "Arial".to_string(),
            update_delay_ms: 250,
            hide_delay_ms: 5000,
            margin: 20,
            colors: ColorsConfig::default(),
            positions: PositionsConfig::default(),
            fade: FadeConfig::default(),
            sound: SoundConfig::default(),
            hotkeys: HotkeyConfig::default(),
        }
    }
}

// Default value functions for serde
fn default_true() -> bool {
    true
}
fn default_freq_en() -> u32 {
    800
}
fn default_freq_ru() -> u32 {
    600
}
fn default_duration() -> u32 {
    50
}
fn default_toggle() -> String {
    "ctrl+alt+l".to_string()
}
fn default_exit() -> String {
    "ctrl+alt+q".to_string()
}
fn default_color_en() -> String {
    "#55FF55".to_string()
}
fn default_color_ru() -> String {
    "#FF5555".to_string()
}
fn default_font_size_corner() -> u32 {
    32
}
fn default_font_size_center() -> u32 {
    64
}
fn default_font_family() -> String {
    "Arial".to_string()
}
fn default_update_delay() -> u32 {
    250
}
fn default_hide_delay() -> u32 {
    5000
}
fn default_margin() -> i32 {
    20
}
fn default_fade_duration() -> u32 {
    200
}
fn default_fade_steps() -> u32 {
    10
}

/// Configuration manager.
pub struct ConfigManager {
    config_path: PathBuf,
}

impl ConfigManager {
    /// Creates a new configuration manager.
    pub fn new() -> Self {
        let config_path = Self::get_config_path();
        Self { config_path }
    }

    /// Gets the path to the configuration file.
    fn get_config_path() -> PathBuf {
        // Try to use the directory where the executable is located
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                return exe_dir.join("config.json");
            }
        }
        // Fallback to current directory
        PathBuf::from("config.json")
    }

    /// Loads configuration from file.
    ///
    /// If the file doesn't exist, creates it with default values.
    pub fn load(&self) -> AppConfig {
        if !self.config_path.exists() {
            let config = AppConfig::default();
            let _ = self.save(&config);
            return config;
        }

        match fs::read_to_string(&self.config_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                log::warn!("Failed to parse config: {}, using defaults", e);
                AppConfig::default()
            }),
            Err(e) => {
                log::warn!("Failed to read config: {}, using defaults", e);
                AppConfig::default()
            }
        }
    }

    /// Saves configuration to file.
    pub fn save(&self, config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(config)?;
        fs::write(&self.config_path, content)?;
        Ok(())
    }

    /// Gets the modification time of the config file.
    pub fn get_modified_time(&self) -> Option<SystemTime> {
        fs::metadata(&self.config_path)
            .ok()
            .and_then(|m| m.modified().ok())
    }

    /// Returns the config file path.
    pub fn path(&self) -> &PathBuf {
        &self.config_path
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses a hex color string to RGB values.
pub fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
        (r, g, b)
    } else {
        (255, 255, 255)
    }
}
