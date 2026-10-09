//! The machine's cores, shared by everything that grows and bakes in the
//! process. A build's worker holds a core while it grows or bakes
//! ([`Seat`]); a tool's parallel work borrows the idle ones for as long as
//! it runs ([`Lease`]). Plants grown side by side then share the machine
//! instead of crowding it with threads, and a plant growing alone uses all
//! of it. No result depends on how many threads work on it.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

/// Every core the process may use.
#[must_use]
pub fn available() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// The most threads one piece of a plant's work spreads over: more than
/// eight pays less than it costs in a step.
#[must_use]
pub fn per_task() -> usize {
    available().min(8)
}

fn idle() -> &'static AtomicIsize {
    static IDLE: OnceLock<AtomicIsize> = OnceLock::new();
    IDLE.get_or_init(|| AtomicIsize::new(isize::try_from(available()).unwrap_or(1)))
}

/// A core a worker holds while it grows or bakes.
pub struct Seat(());

impl Seat {
    #[must_use]
    pub fn take() -> Self {
        idle().fetch_sub(1, Ordering::Relaxed);
        Self(())
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        idle().fetch_add(1, Ordering::Relaxed);
    }
}

/// Idle cores borrowed for one piece of parallel work, given back when it
/// ends.
pub struct Lease {
    extra: isize,
}

impl Lease {
    /// Threads for work worth up to `want` of them: the calling one and as
    /// many idle cores as there are.
    #[must_use]
    pub fn take(want: usize) -> Self {
        let want = isize::try_from(want.saturating_sub(1)).unwrap_or(0);
        let idle = idle();
        let mut current = idle.load(Ordering::Relaxed);
        loop {
            let extra = current.min(want).max(0);
            if extra == 0 {
                return Self { extra: 0 };
            }
            match idle.compare_exchange_weak(
                current,
                current - extra,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Self { extra },
                Err(now) => current = now,
            }
        }
    }

    /// How many threads the work may use.
    #[must_use]
    pub fn threads(&self) -> usize {
        1 + usize::try_from(self.extra).unwrap_or(0)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.extra > 0 {
            idle().fetch_add(self.extra, Ordering::Relaxed);
        }
    }
}

/// Run `work` on every item on up to `threads` threads and return the
/// results in the items' order, so the output never depends on scheduling.
pub fn map<T: Send, R: Send>(
    items: Vec<T>,
    threads: usize,
    work: &(dyn Fn(T) -> R + Sync),
) -> Vec<R> {
    let count = items.len();
    if threads <= 1 || count < 2 {
        return items.into_iter().map(work).collect();
    }
    let queue = Mutex::new(items.into_iter().enumerate());
    let done: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(count));
    std::thread::scope(|scope| {
        for _ in 0..threads.min(count) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                    let Some((at, item)) = next else {
                        break;
                    };
                    let result = work(item);
                    done.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push((at, result));
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_else(PoisonError::into_inner);
    done.sort_by_key(|(at, _)| *at);
    done.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_keeps_the_items_order() {
        let items: Vec<u64> = (0..1000).collect();
        let squares = map(items, 4, &|item| item * item);
        assert!(
            squares
                .iter()
                .enumerate()
                .all(|(at, square)| *square == (at * at) as u64)
        );
    }

    #[test]
    fn a_lease_takes_only_idle_cores_and_gives_them_back() {
        // Other tests may hold cores at the same time; count from here.
        let lease = Lease::take(1);
        assert_eq!(lease.threads(), 1);
        drop(lease);
        let lease = Lease::take(usize::MAX);
        assert!(lease.threads() >= 1 && lease.threads() <= available().max(1));
        drop(lease);
    }
}
