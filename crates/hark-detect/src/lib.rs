//! Meeting detection: which meeting app (if any) is running a call right now.
//!
//! Pure logic (pattern matching, debouncing, audio activity) lives in small
//! modules with unit tests; OS window enumeration is behind `list_visible_windows`.

mod audio_activity;
mod debounce;
mod patterns;
mod windows_enum;

pub use audio_activity::AudioActivity;
pub use debounce::Debouncer;
pub use patterns::{Detected, Pattern, PatternError, PatternSet, WindowInfo};
pub use windows_enum::list_visible_windows;

/// Convenience: enumerate windows and match against `set`.
pub fn detect_now(set: &PatternSet) -> Option<Detected> {
    set.match_windows(&list_visible_windows())
}
