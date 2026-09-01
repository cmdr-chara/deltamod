use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// Trusted time source used for lease authority and durable timestamps.
pub trait TrustedClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Store-owned wrapper that never returns a value below an observed durable
/// floor, even when the underlying wall clock moves backwards.
pub(crate) struct MonotonicClock {
    source: Arc<dyn TrustedClock>,
    floor_ms: AtomicU64,
}

impl MonotonicClock {
    pub(crate) fn new(source: Arc<dyn TrustedClock>) -> Self {
        Self {
            source,
            floor_ms: AtomicU64::new(0),
        }
    }

    pub(crate) fn raise_floor(&self, floor_ms: u64) -> u64 {
        self.floor_ms
            .fetch_max(floor_ms, Ordering::SeqCst)
            .max(floor_ms)
    }
}

impl TrustedClock for MonotonicClock {
    fn now_ms(&self) -> u64 {
        self.raise_floor(self.source.now_ms())
    }
}

/// Production clock backed by wall-clock time since the Unix epoch.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl TrustedClock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
            })
    }
}

/// Deterministic trusted clock for crash and lease-fencing tests.
#[derive(Clone, Debug)]
pub struct ManualClock {
    now_ms: Arc<AtomicU64>,
}

impl ManualClock {
    #[must_use]
    pub fn new(now_ms: u64) -> Self {
        Self {
            now_ms: Arc::new(AtomicU64::new(now_ms)),
        }
    }

    pub fn set(&self, now_ms: u64) {
        self.now_ms.store(now_ms, Ordering::SeqCst);
    }

    pub fn advance(&self, delta_ms: u64) -> Option<u64> {
        self.now_ms
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(delta_ms)
            })
            .ok()
            .and_then(|previous| previous.checked_add(delta_ms))
    }
}

impl TrustedClock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }
}
