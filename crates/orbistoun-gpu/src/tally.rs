//! The bytes a holder of host memory keeps, published through atomics as it changes, so the run's
//! memory census reads every holder without taking a lock: a census waiting on a holder's lock
//! waits on whatever that holder's thread waits on, which can be the thread the census runs on.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LockResult, Mutex, MutexGuard, PoisonError, TryLockError, TryLockResult};

/// One holder's bytes, as last published.
#[derive(Debug, Default)]
pub struct Tally(AtomicU64);

impl Tally {
    /// Nothing held.
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// The bytes last published.
    #[must_use]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Publishes `bytes` as the holder's whole.
    pub fn set(&self, bytes: u64) {
        self.0.store(bytes, Ordering::Relaxed);
    }

    /// Counts `bytes` more.
    pub fn add(&self, bytes: u64) {
        self.0.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Counts `bytes` fewer, never below nothing.
    pub fn sub(&self, bytes: u64) {
        let _ = self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |held| {
                Some(held.saturating_sub(bytes))
            });
    }
}

/// A mutex whose contents publish what they hold to `N` tallies each time it is let go, measured
/// by `measure` while it is still held: every change is published by the thread that made it, and
/// a census reads [`Tallied::tallies`] without the lock.
pub struct Tallied<T, const N: usize> {
    inner: Mutex<T>,
    tallies: [Tally; N],
    measure: fn(&T) -> [u64; N],
}

impl<T, const N: usize> std::fmt::Debug for Tallied<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tallied")
            .field("tallies", &self.tallies)
            .finish_non_exhaustive()
    }
}

impl<T, const N: usize> Tallied<T, N> {
    /// `value`, published as `measure` measures it each time it is let go.
    pub const fn new(value: T, measure: fn(&T) -> [u64; N]) -> Self {
        Self {
            inner: Mutex::new(value),
            tallies: [const { Tally::new() }; N],
            measure,
        }
    }

    /// Locks the contents as [`Mutex::lock`] does; they are measured again when the guard drops.
    ///
    /// # Errors
    ///
    /// When a thread panicked holding it, as [`Mutex::lock`].
    pub fn lock(&self) -> LockResult<TalliedGuard<'_, T, N>> {
        match self.inner.lock() {
            Ok(guard) => Ok(TalliedGuard { guard, owner: self }),
            Err(poisoned) => Err(PoisonError::new(TalliedGuard {
                guard: poisoned.into_inner(),
                owner: self,
            })),
        }
    }

    /// Locks the contents as [`Mutex::try_lock`] does, never waiting.
    ///
    /// # Errors
    ///
    /// When another thread holds it, or one panicked holding it, as [`Mutex::try_lock`].
    pub fn try_lock(&self) -> TryLockResult<TalliedGuard<'_, T, N>> {
        match self.inner.try_lock() {
            Ok(guard) => Ok(TalliedGuard { guard, owner: self }),
            Err(TryLockError::Poisoned(poisoned)) => {
                Err(TryLockError::Poisoned(PoisonError::new(TalliedGuard {
                    guard: poisoned.into_inner(),
                    owner: self,
                })))
            }
            Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
        }
    }

    /// What the contents held when they were last let go, without the lock.
    pub fn tallies(&self) -> [u64; N] {
        std::array::from_fn(|at| self.tallies[at].get())
    }
}

/// A [`Tallied`] held; publishes what it holds when dropped.
pub struct TalliedGuard<'a, T, const N: usize> {
    guard: MutexGuard<'a, T>,
    owner: &'a Tallied<T, N>,
}

impl<T, const N: usize> std::fmt::Debug for TalliedGuard<'_, T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TalliedGuard").finish_non_exhaustive()
    }
}

impl<T, const N: usize> std::ops::Deref for TalliedGuard<'_, T, N> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T, const N: usize> std::ops::DerefMut for TalliedGuard<'_, T, N> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

impl<T, const N: usize> Drop for TalliedGuard<'_, T, N> {
    fn drop(&mut self) {
        let measured = (self.owner.measure)(&self.guard);
        for (tally, bytes) in self.owner.tallies.iter().zip(measured) {
            tally.set(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Tallied, Tally};

    /// What the contents hold is published as the guard drops, and read while another thread holds
    /// the lock.
    #[test]
    fn a_tallied_mutex_publishes_as_it_is_let_go() {
        static HELD: Tallied<Vec<u8>, 1> = Tallied::new(Vec::new(), |held| [held.len() as u64]);
        HELD.lock().expect("not poisoned").extend([0; 100]);
        assert_eq!(HELD.tallies(), [100]);
        let guard = HELD.lock().expect("not poisoned");
        let read = std::thread::spawn(|| HELD.tallies()).join().expect("read");
        assert_eq!(read, [100], "read while held");
        drop(guard);
    }

    /// A tally never counts below nothing.
    #[test]
    fn a_tally_saturates() {
        let tally = Tally::new();
        tally.add(5);
        tally.sub(9);
        assert_eq!(tally.get(), 0);
    }
}
