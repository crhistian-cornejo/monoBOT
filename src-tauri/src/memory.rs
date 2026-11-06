//! Hands freed heap pages back to the system. When a speech model is released
//! or the last agent run ends, the allocator keeps much of what was freed for
//! reuse, and that memory still counts toward the app's footprint until the
//! allocator is asked to let it go.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Asking the allocator to give pages back walks its zones; once in a while
/// is enough.
const MIN_GAP_SECS: u64 = 30;

static LAST_RELEASE: AtomicU64 = AtomicU64::new(0);

/// Returns free pages to the system soon, off the calling thread, at most
/// once every `MIN_GAP_SECS`.
pub fn release_soon() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    if !claim(&LAST_RELEASE, now) {
        return;
    }
    tauri::async_runtime::spawn_blocking(release_now);
}

/// Takes the turn to release at `now` unless another release ran recently.
fn claim(last: &AtomicU64, now: u64) -> bool {
    let previous = last.load(Ordering::Acquire);
    if previous != 0 && now.saturating_sub(previous) < MIN_GAP_SECS {
        return false;
    }
    last.compare_exchange(previous, now, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

fn release_now() {
    #[cfg(target_os = "macos")]
    // SAFETY: a null zone asks every malloc zone to release what it can; a
    // goal of 0 means as much as possible. It only frees unused pages.
    unsafe {
        malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: glibc's malloc_trim only returns free memory at the top of the
    // heap and in free chunks to the system.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_at_most_once_per_gap() {
        let last = AtomicU64::new(0);
        assert!(claim(&last, 1_000));
        assert!(!claim(&last, 1_000 + MIN_GAP_SECS - 1));
        assert!(claim(&last, 1_000 + MIN_GAP_SECS));
    }

    #[test]
    fn releasing_now_does_not_crash() {
        release_now();
    }
}
