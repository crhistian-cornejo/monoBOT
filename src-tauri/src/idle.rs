//! Frees a local model's memory once it has gone unused for a while. The
//! speech models hold hundreds of megabytes each; keeping them only while the
//! user is dictating or a call is speaking keeps the app light the rest of
//! the time, at the cost of a few seconds to load them again.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Counts uses, so a release scheduled after one use is skipped when another
/// use came before it ran.
pub struct IdleRelease {
    uses: AtomicU64,
}

impl IdleRelease {
    pub const fn new() -> Self {
        Self {
            uses: AtomicU64::new(0),
        }
    }

    /// Marks a use without scheduling a release, for work that is still
    /// running (an open microphone): it cancels any release already waiting.
    pub fn hold(&self) {
        self.uses.fetch_add(1, Ordering::AcqRel);
    }

    /// Marks a use and runs `release` after `after`, unless the model is used
    /// again first.
    pub fn touch(&'static self, after: Duration, release: impl FnOnce() + Send + 'static) {
        let this_use = self.uses.fetch_add(1, Ordering::AcqRel) + 1;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(after).await;
            if self.uses.load(Ordering::Acquire) == this_use {
                release();
            }
        });
    }

    #[cfg(test)]
    fn is_current(&self, this_use: u64) -> bool {
        self.uses.load(Ordering::Acquire) == this_use
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_use_cancels_the_waiting_release() {
        let idle = IdleRelease::new();
        idle.hold();
        let first = idle.uses.load(Ordering::Acquire);
        assert!(idle.is_current(first));
        idle.hold();
        assert!(!idle.is_current(first));
    }
}
