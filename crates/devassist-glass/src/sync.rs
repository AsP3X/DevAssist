// Copied from Quill (pencraft) app/src/app.rs at 6c6a0cc.
// Only the function `lock`; the module comment is new.

//! Locking shared state.

use std::sync::{Mutex, PoisonError};

/// Locks a mutex even if a panicking thread left it poisoned (the data is a plain picture).
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
