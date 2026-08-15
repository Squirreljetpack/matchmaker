use std::sync::{Mutex, MutexGuard, OnceLock};

use nucleo::Matcher;

/// The process-wide scoring matcher, shared by the picker and all overlays.
///
/// [`Matcher::new`] eagerly allocates its scratch buffers (~135KB), so a
/// single instance is reused for every scoring call in the process. The lock
/// is held only for the duration of one scoring call; all users run on the
/// render thread, so the lock is uncontended.
static MATCHER: OnceLock<Mutex<Matcher>> = OnceLock::new();

/// Locks and returns the process-wide scoring matcher.
pub fn matcher() -> MutexGuard<'static, Matcher> {
    MATCHER
        .get_or_init(|| Mutex::new(Matcher::new(nucleo::Config::DEFAULT)))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Sets the config of the process-wide scoring matcher.
///
/// Call before starting a picker run; the config persists for the process.
pub fn set_matcher_config(config: nucleo::Config) {
    matcher().config = config;
}
