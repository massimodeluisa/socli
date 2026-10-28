//! Wall time and a monotonic clock.
//!
//! `wasm32-unknown-unknown` has neither. `SystemTime::now` and `Instant::now` abort there.
//! The browser supplies both through two imports, `socli_millis` and `socli_mono`, and the
//! session freezes the wall time so ages stay put. Native builds read the process clock.

use std::ops::{Add, Sub};
use std::time::Duration;

use chrono::{DateTime, Utc};

/// Milliseconds since an arbitrary start. Enough for a four-second confirmation and a spinner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(u64);

impl Instant {
    pub fn now() -> Self {
        Self(mono_millis())
    }

    pub fn elapsed(self) -> Duration {
        Duration::from_millis(Self::now().0.saturating_sub(self.0))
    }

    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_millis(self.0.saturating_sub(earlier.0))
    }
}

impl Add<Duration> for Instant {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self {
        let millis = u64::try_from(rhs.as_millis()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlines_saturate_before_narrowing_large_durations() {
        assert_eq!(Instant(7) + Duration::MAX, Instant(u64::MAX));
        assert_eq!(
            Instant(u64::MAX) + Duration::from_millis(1),
            Instant(u64::MAX)
        );
        assert_eq!(Instant(7) + Duration::from_millis(3), Instant(10));
    }
}

impl Sub for Instant {
    type Output = Duration;

    fn sub(self, earlier: Self) -> Duration {
        self.saturating_duration_since(earlier)
    }
}

/// Unix milliseconds. Native reads the process clock. The browser reads `socli_millis`.
pub fn unix_millis() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: `socli_millis` is the `env` import `fn() -> f64`, and the reader worker implements it as `Date.now()`; this call passes no pointer.
        unsafe { socli_millis() as u64 }
    }
}

/// Unix seconds from [`unix_millis`].
pub fn unix_secs() -> u64 {
    unix_millis() / 1000
}

/** Native sleeps. The browser build has no threads, so it spins on the monotonic clock. */
pub fn pause(duration: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::sleep(duration);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let start = Instant::now();
        while start.elapsed() < duration {
            std::hint::spin_loop();
        }
    }
}

/// The wall clock. The browser session freezes what this returns before it draws.
pub fn utc_now() -> DateTime<Utc> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Utc::now()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let millis =
            // SAFETY: `socli_millis` is the `env` import `fn() -> f64`, and the reader worker implements it as `Date.now()`; this call passes no pointer.
            unsafe { socli_millis() };
        DateTime::from_timestamp_millis(millis as i64).unwrap_or(DateTime::UNIX_EPOCH)
    }
}

fn mono_millis() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant as StdInstant;
        static ORIGIN: OnceLock<StdInstant> = OnceLock::new();
        ORIGIN.get_or_init(StdInstant::now).elapsed().as_millis() as u64
    }
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: `socli_mono` is the `env` import `fn() -> f64`, and the reader worker implements it as `performance.now()`; this call passes no pointer.
        unsafe { socli_mono() as u64 }
    }
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    /* Date.now() and performance.now(), both in milliseconds. The page passes them in.
    The module name is required: without it rust-lld treats these as undefined symbols. */
    fn socli_millis() -> f64;
    fn socli_mono() -> f64;
}
