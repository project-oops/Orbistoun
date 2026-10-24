//! What a pipeline's caches hold of the host's memory, for the run report's breakdown.

use super::Pipeline;

/// The caches [`Pipeline::held`] measures, by name and in its order.
pub(crate) const HELD: [&str; 5] = [
    "texel cache",
    "detiled images",
    "draw buffer cache",
    "dispatch modules",
    "translation store",
];

impl Pipeline {
    /// The bytes each of its caches holds, in `HELD`'s order: texels read from guest memory and
    /// those detiled from them, guest draw buffers, dispatch modules, and the kept translations.
    /// Each is counted as it changes, so this is cheap enough to ask whenever the pipeline is let
    /// go.
    #[must_use]
    pub fn held(&self) -> [u64; 5] {
        [
            self.texels.bytes() as u64,
            self.detiled.bytes() as u64,
            self.buffers.bytes() as u64,
            self.dispatch_modules.bytes() as u64,
            self.store
                .as_ref()
                .map_or(0, crate::translations::TranslationStore::held_bytes),
        ]
    }
}

/// The bytes the caches every pipeline shares hold, by name: texels re-picked from those read, and
/// mip levels joined into chains. Read without their locks.
#[must_use]
pub(crate) fn shared_held() -> [(&'static str, u64); 2] {
    [
        ("swizzled texels", super::swizzled_bytes()),
        ("mip chains", super::kept_levels_bytes()),
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    /// The shared caches are read while another thread holds both.
    #[test]
    fn the_shared_caches_are_read_while_they_are_locked() {
        let (locked, both_locked) = channel();
        let (release, released) = channel::<()>();
        let holder = std::thread::spawn(move || {
            let swizzled = super::super::swizzled_kept()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let levels = super::super::kept_levels()
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            locked.send(()).expect("the test waits");
            let _ = released.recv_timeout(Duration::from_secs(30));
            drop((swizzled, levels));
        });
        both_locked.recv().expect("the holder locks");
        let (answer, answered) = channel();
        std::thread::spawn(move || {
            let _ = answer.send(super::shared_held());
        });
        let census = answered.recv_timeout(Duration::from_secs(5));
        let _ = release.send(());
        holder.join().expect("the holder lets go");
        assert!(census.is_ok(), "the census waited on a lock");
    }
}
