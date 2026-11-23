use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the unix epoch.
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The crate's poisoning policy: every mutex here guards state that stays
/// valid if a holder panics (queues, maps, records), so a poisoned lock is
/// recovered rather than spread to every other thread of the host.
pub(crate) trait LockExt<T> {
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One line on stderr, which the spawner points at `host.log`.
macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("{} artalde-host: {}", $crate::util::now(), format_args!($($arg)*))
    };
}
pub(crate) use log;
