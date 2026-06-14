//! Audio notification playback.
//!
//! Plays sounds on task completion, errors, and permission prompts.

/// Notification sound types.
#[derive(Debug, Clone, Copy)]
pub enum Sound {
    Complete,
    Error,
    Prompt,
}

/// Play a notification sound. Does nothing if audio is disabled.
pub fn play_sound(sound: Sound, enabled: bool, _volume: f64) {
    if !enabled {
        return;
    }

    match sound {
        Sound::Complete => play_bell(),
        Sound::Error => play_bell(),
        Sound::Prompt => play_bell(),
    }
}

/// Play the terminal bell (works cross-platform).
fn play_bell() {
    // Terminal bell — simplest cross-platform approach
    print!("\x07");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_play_sound_disabled_does_nothing() {
        // Should not panic
        play_sound(Sound::Complete, false, 0.5);
    }

    #[test]
    fn test_sound_variants() {
        // Verify all variants exist
        let sounds = [Sound::Complete, Sound::Error, Sound::Prompt];
        assert_eq!(sounds.len(), 3);
    }
}
