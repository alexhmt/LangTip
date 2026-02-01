//! Sound module for layout change notifications.
//!
//! Uses Windows Beep API to play short sounds with different frequencies
//! for different layouts.

use crate::config::SoundConfig;
use windows::Win32::System::Diagnostics::Debug::Beep;

/// Plays a sound when the layout changes.
///
/// # Arguments
/// * `is_russian` - true for Russian layout, false for English
/// * `config` - Sound configuration
pub fn play_layout_sound(is_russian: bool, config: &SoundConfig) {
    if !config.enabled {
        return;
    }

    let freq = if is_russian {
        config.frequency_ru
    } else {
        config.frequency_en
    };

    // Windows Beep: frequency 37-32767 Hz
    unsafe {
        let _ = Beep(freq, config.duration_ms);
    }
}
