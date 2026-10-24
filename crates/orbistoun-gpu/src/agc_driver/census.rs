//! What the command processor holds of the host's memory, for the run report's breakdown.
//!
//! Every holder publishes its bytes through atomics as it changes - the mutexes holding them as
//! [`crate::tally::Tallied`], a kept copy of a target's memory as it is made and dropped - so the
//! census reads them all without a lock. It runs inside a perf window, on whichever thread closes
//! it, while that thread may hold any of these: a census that waited on one could wait on itself.

use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    Submission, aside, delivered_modules, held_streams, live_pipeline, stalled_stream,
    undelivered_modules,
};

/// The bytes each holder keeps, by name: the live pipeline's caches and those every pipeline
/// shares, the translated modules kept for the backend, the copies of colour targets' memory kept
/// to compare or write back, and the streams a waiting queue holds. Takes no lock.
#[must_use]
pub fn host_held() -> Vec<(&'static str, u64)> {
    let mut held: Vec<(&'static str, u64)> = crate::pipeline::HELD
        .into_iter()
        .zip(live_pipeline().tallies())
        .collect();
    held.extend(crate::pipeline::shared_held());
    held.push((
        "modules kept for the backend",
        undelivered_modules().tallies()[0] + delivered_modules().tallies()[0],
    ));
    held.push((
        "colour target copies",
        TARGET_COPIES.load(Ordering::Relaxed),
    ));
    held.push((
        "streams held at a wait",
        stalled_stream().tallies()[0] + held_streams().tallies()[0] + aside().tallies()[0],
    ));
    held
}

/// The bytes of modules' words kept by resource.
pub(super) fn modules_bytes(modules: &super::KeptModules) -> [u64; 1] {
    [modules.values().map(|m| m.len() as u64 * 4).sum()]
}

/// The bytes a command stream held at a wait keeps: its own, and its submission's modules and
/// commands.
pub(super) fn stream_bytes(bytes: &[u8], submission: &Submission) -> u64 {
    let modules: usize = submission.modules.values().map(Vec::capacity).sum();
    (bytes.len() + modules * 4 + submission.commands.capacity() * size_of::<crate::RenderCommand>())
        as u64
}

/// The bytes of colour targets' memory kept on the host while they live: the last written or read
/// and those before it, and what each deferred write-back keeps of what memory held - each shared
/// copy once, as each is one [`KeptBytes`].
static TARGET_COPIES: AtomicU64 = AtomicU64::new(0);

/// A colour target's memory kept on the host, counted in [`TARGET_COPIES`] while it lives.
#[derive(Debug)]
pub(super) struct KeptBytes {
    bytes: Vec<u8>,
    counted: u64,
}

impl KeptBytes {
    pub(super) fn new(bytes: Vec<u8>) -> Self {
        let counted = bytes.len() as u64;
        TARGET_COPIES.fetch_add(counted, Ordering::Relaxed);
        Self { bytes, counted }
    }

    /// The bytes, no longer counted: storage to fill again.
    pub(super) fn into_inner(mut self) -> Vec<u8> {
        uncount(std::mem::take(&mut self.counted));
        std::mem::take(&mut self.bytes)
    }
}

impl std::ops::Deref for KeptBytes {
    type Target = Vec<u8>;

    fn deref(&self) -> &Vec<u8> {
        &self.bytes
    }
}

impl Drop for KeptBytes {
    fn drop(&mut self) {
        uncount(self.counted);
    }
}

fn uncount(bytes: u64) {
    let _ = TARGET_COPIES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |held| {
        Some(held.saturating_sub(bytes))
    });
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use super::super::{
        aside, deferred, delivered_modules, earlier_written, held_streams, last_written,
        live_pipeline, stalled_stream, undelivered_modules,
    };

    /// The census answers while another thread holds every lock around what it counts.
    #[test]
    fn the_census_answers_while_every_holder_is_locked() {
        let _serial = super::super::tests::serial();
        let (locked, all_locked) = channel();
        let (release, released) = channel::<()>();
        let holder = std::thread::spawn(move || {
            let pipeline = live_pipeline()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let pending = undelivered_modules()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let delivered = delivered_modules()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let stalled = stalled_stream()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let held = held_streams()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let aside = aside().lock().unwrap_or_else(PoisonError::into_inner);
            let last = last_written()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let earlier = earlier_written()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let copies = deferred().lock().unwrap_or_else(PoisonError::into_inner);
            locked.send(()).expect("the test waits");
            let _ = released.recv_timeout(Duration::from_secs(30));
            drop((
                pipeline, pending, delivered, stalled, held, aside, last, earlier, copies,
            ));
        });
        all_locked.recv().expect("the holder locks");
        let (answer, answered) = channel();
        std::thread::spawn(move || {
            let _ = answer.send(super::host_held());
        });
        let census = answered.recv_timeout(Duration::from_secs(5));
        let _ = release.send(());
        holder.join().expect("the holder lets go");
        let census = census.expect("the census waited on a lock");
        assert!(
            census
                .iter()
                .any(|&(name, _)| name == "colour target copies")
        );
    }

    /// A kept copy is counted while it lives, once however many share it, and not once its bytes
    /// are taken back as storage.
    #[test]
    fn a_kept_copy_is_counted_while_it_lives() {
        // Other tests keep copies meanwhile; this one's are large enough to tell apart.
        const BYTES: usize = 64 << 20;
        let counted = || super::TARGET_COPIES.load(std::sync::atomic::Ordering::Relaxed);
        let before = counted();
        let kept = std::sync::Arc::new(super::KeptBytes::new(vec![0; BYTES]));
        let shared = std::sync::Arc::clone(&kept);
        assert!(counted() >= before + BYTES as u64 / 2, "counted once kept");
        drop(kept);
        let storage = std::sync::Arc::try_unwrap(shared)
            .expect("the last holder")
            .into_inner();
        assert_eq!(storage.len(), BYTES);
        assert!(
            counted() < before + BYTES as u64 / 2,
            "and not once taken back"
        );
    }
}
