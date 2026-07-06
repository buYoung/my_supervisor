//! `SystemClock` — time source, swappable for deterministic tests.

use chrono::{DateTime, Utc};

pub trait SystemClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Wall-clock implementation used in production wiring.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealClock;

impl SystemClock for RealClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
