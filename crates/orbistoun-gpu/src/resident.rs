//! Dispatch images kept on the device from one dispatch to the next.
//!
//! A dispatch image is read from guest memory, detiled on the host and uploaded, and an image a
//! dispatch stores to is read back and tiled into guest memory after it runs. Most of a frame's
//! images are the same memory from one dispatch to the next - a pass's output is the next pass's
//! input - so each is kept on the device under a key naming where it lies and how it is viewed,
//! and taken again while guest memory there is unchanged.
//!
//! Coherence is the host's write-watch (D824, D984), as a texture's is:
//!
//! - *Kept.* An image is kept once a dispatch that bound it ran, from a mark of its bytes taken
//!   before they were read; a stored image from a mark taken after its write-back, which put into
//!   guest memory exactly what the device holds.
//! - *Taken.* An image kept is taken while nothing has written its bytes since its mark: a
//!   preparation settles the bytes first (a frame pending over them written back, a deferred copy
//!   carried out), and each of those is a write the mark sees. The device's own stores into guest
//!   memory are noted written where they land (D964).
//! - *Written while it ran.* A stored image whose bytes something else wrote while its dispatch ran
//!   is not kept: the write-back would hide that write from the mark taken after it.
//! - *Let go.* The registry, not the device, decides what is kept: the oldest images past a budget
//!   are let go, and the device is told which. A device that no longer holds an image it is asked
//!   for refuses the dispatch, and every image is let go before it is prepared again.
//!
//! This module decides; the backend holds the images. Nothing here names a host graphics API.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a backend answers a dispatch with when an image it is asked to take is not kept: the
/// command processor lets every image go and prepares the dispatch again.
pub const NOT_RESIDENT: &str = "a dispatch image the device was asked to keep is not kept";

/// The most bytes of images kept at once, before the oldest are let go: each is held on the device
/// and again in the host-visible buffer it travels through, so twice this in all.
pub const KEPT_BYTES_MOST: u64 = 3 << 29;

/// One image's place in a dispatch: its key, the generation of its contents, and whether its
/// texels come with it, to be uploaded and kept, or the device takes the image it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Use {
    /// Where it lies and how it is viewed.
    pub key: u64,
    /// Which contents of it: a fresh read makes a new generation.
    pub generation: u64,
    /// Whether its texels were read for this dispatch.
    pub fresh: bool,
}

/// A dispatch's images' places, by binding role, and the images the device is to let go first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Uses {
    /// The first texture slot's image.
    pub fetched: Option<Use>,
    /// The further texture slots' images, in slot order.
    pub more_fetched: Vec<Option<Use>>,
    /// The images it stores to, in order.
    pub stored: Vec<Option<Use>>,
    /// The images its program selects from tables at run time, in their places (D983).
    pub selected: Vec<Option<Use>>,
}

impl Uses {
    /// Every place taken, with whether it is a stored image.
    fn each(&self) -> impl Iterator<Item = (Use, bool)> + '_ {
        self.fetched
            .iter()
            .chain(self.more_fetched.iter().flatten())
            .chain(self.selected.iter().flatten())
            .map(|used| (*used, false))
            .chain(self.stored.iter().flatten().map(|used| (*used, true)))
    }

    /// Whether any image is taken rather than read: the dispatch then needs the device to hold it.
    #[must_use]
    pub fn takes_any(&self) -> bool {
        self.each().any(|(used, _)| !used.fresh)
    }
}

/// What a mark of guest memory is asked: the host's write-watch, or a test's stand-in.
pub trait Watch {
    /// A point in time for `span`; `None` where it cannot be watched.
    fn mark(&self, span: (u64, u64)) -> Option<u64>;
    /// Whether `span` was written after `since`; `None` where that cannot be said.
    fn written_since(&self, span: (u64, u64), since: u64) -> Option<bool>;
}

/// The host's write-watch, unguarded: a guard would make the bytes read-only, and the command
/// processor writes them back at every dispatch that stores to them.
#[derive(Debug, Clone, Copy, Default)]
pub struct HostWatch;

impl Watch for HostWatch {
    fn mark(&self, (base, len): (u64, u64)) -> Option<u64> {
        orbistoun_mem::watch::mark_all(&[(base, len)])
    }

    fn written_since(&self, (base, len): (u64, u64), since: u64) -> Option<bool> {
        orbistoun_mem::watch::written_since(base, len, since)
    }
}

/// Where a kept image's contents stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Read for a dispatch that has not answered yet, from a mark taken before the read.
    Read { since: u64 },
    /// Stored to by a dispatch whose write-back has not been made yet.
    Writing,
    /// On the device and the same as guest memory since the mark.
    Kept { since: u64 },
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    span: (u64, u64),
    bytes: u64,
    generation: u64,
    state: State,
    /// The last dispatch that bound it, for letting the oldest go.
    used: u64,
}

/// Every image the registry knows of, by key.
#[derive(Debug, Default)]
pub struct Registry {
    entries: HashMap<u64, Entry>,
    /// Bytes of the entries.
    bytes: u64,
    /// The dispatch being prepared, counted.
    dispatch: u64,
    /// Keys a place was given in the dispatch being prepared: one image is bound once.
    placed: HashSet<u64>,
    /// The next generation.
    generation: u64,
    /// Keys let go that the device has not been told of.
    dropped: Vec<u64>,
    /// The most bytes kept.
    budget: u64,
}

impl Registry {
    /// A registry keeping at most `budget` bytes.
    #[must_use]
    pub fn new(budget: u64) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }

    /// Starts the preparation of a dispatch: each image takes one place in it.
    pub fn begin(&mut self) {
        self.dispatch += 1;
        self.placed.clear();
    }

    /// The place of the image `key`, `bytes` of texels lying over `span`, in the dispatch being
    /// prepared, its bytes already settled: taken while kept and unwritten, else read afresh from
    /// a mark taken now, before the caller reads them. `None` where the image is not kept at all:
    /// its bytes cannot be watched, or it has a place in this dispatch already.
    pub fn place(
        &mut self,
        watch: &impl Watch,
        (key, span, bytes): (u64, (u64, u64), u64),
    ) -> Option<Use> {
        if !self.placed.insert(key) {
            return None;
        }
        let dispatch = self.dispatch;
        if let Some(entry) = self.entries.get_mut(&key)
            && entry.span == span
            && let State::Kept { since } = entry.state
            && watch.written_since(span, since) == Some(false)
        {
            entry.used = dispatch;
            return Some(Use {
                key,
                generation: entry.generation,
                fresh: false,
            });
        }
        let Some(since) = watch.mark(span) else {
            self.forget(key);
            return None;
        };
        self.generation += 1;
        let generation = self.generation;
        let old = self.entries.insert(
            key,
            Entry {
                span,
                bytes,
                generation,
                state: State::Read { since },
                used: dispatch,
            },
        );
        self.bytes = self.bytes - old.map_or(0, |old| old.bytes) + bytes;
        self.let_oldest_go();
        Some(Use {
            key,
            generation,
            fresh: true,
        })
    }

    /// Lets the least recently bound images go until the rest fit the budget, never one bound in
    /// the dispatch being prepared.
    fn let_oldest_go(&mut self) {
        while self.bytes > self.budget {
            let Some((&key, _)) = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.used < self.dispatch)
                .min_by_key(|(_, entry)| entry.used)
            else {
                return;
            };
            self.forget(key);
        }
    }

    /// Forgets `key`, telling the device to let it go.
    fn forget(&mut self, key: u64) {
        if let Some(entry) = self.entries.remove(&key) {
            self.bytes -= entry.bytes;
            self.dropped.push(key);
        }
    }

    /// The dispatch whose places are `uses` ran, or failed: each image read for it is kept from
    /// its mark; each it stored to waits for its write-back, unless something else wrote its bytes
    /// while it ran. A failure lets every image it bound go, whatever the device holds of them.
    pub fn ran(&mut self, watch: &impl Watch, uses: &Uses, ok: bool) {
        for (used, stored) in uses.each() {
            let Some(entry) = self.entries.get(&used.key).copied() else {
                continue;
            };
            if entry.generation != used.generation || !ok {
                self.forget(used.key);
                continue;
            }
            let since = match entry.state {
                State::Read { since } | State::Kept { since } => since,
                State::Writing => {
                    self.forget(used.key);
                    continue;
                }
            };
            let state = if stored {
                if watch.written_since(entry.span, since) != Some(false) {
                    self.forget(used.key);
                    continue;
                }
                State::Writing
            } else {
                State::Kept { since }
            };
            if let Some(entry) = self.entries.get_mut(&used.key) {
                entry.state = state;
            }
        }
    }

    /// The write-back of the dispatch whose places are `uses` is made: each image it stored to is
    /// kept from a mark taken now, guest memory holding what the device does.
    pub fn written_back(&mut self, watch: &impl Watch, uses: &Uses) {
        for used in uses.stored.iter().flatten() {
            let Some(entry) = self.entries.get(&used.key).copied() else {
                continue;
            };
            if entry.generation != used.generation || entry.state != State::Writing {
                continue;
            }
            match watch.mark(entry.span) {
                Some(since) => {
                    if let Some(entry) = self.entries.get_mut(&used.key) {
                        entry.state = State::Kept { since };
                    }
                }
                None => self.forget(used.key),
            }
        }
    }

    /// Lets every image go: the device no longer holds one it was asked for.
    pub fn forget_all(&mut self) {
        let keys: Vec<u64> = self.entries.keys().copied().collect();
        for key in keys {
            self.forget(key);
        }
    }

    /// The keys let go since the device was last told.
    pub fn take_dropped(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.dropped)
    }

    /// Bytes of the images it knows of.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }
}

/// Whether images are kept: off where a diagnostic copies each dispatch's texels, which an image
/// taken from the device does not carry.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Keeps images from now on, or stops: set once, by whoever installs the dispatch executor.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether images are kept.
#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The run's registry.
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: std::sync::OnceLock<Mutex<Registry>> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::new(KEPT_BYTES_MOST)))
}

/// Runs `with` on the run's registry.
pub fn with_registry<T>(with: impl FnOnce(&mut Registry) -> T) -> T {
    with(
        &mut registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// The key of an image lying over `span` and viewed as `view` describes - its descriptor's words,
/// format, extent and channel order: one image of memory viewed two ways is two images.
#[must_use]
pub fn key(span: (u64, u64), view: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    span.hash(&mut hasher);
    view.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{Registry, State, Use, Uses, Watch};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A write-watch over spans by first byte: each mark a new epoch, each write the epoch it came
    /// after.
    #[derive(Default)]
    struct Fake {
        epoch: RefCell<u64>,
        written: RefCell<HashMap<u64, u64>>,
        unwatched: bool,
    }

    impl Fake {
        fn write(&self, base: u64) {
            let epoch = *self.epoch.borrow();
            self.written.borrow_mut().insert(base, epoch);
            *self.epoch.borrow_mut() += 1;
        }
    }

    impl Watch for Fake {
        fn mark(&self, _: (u64, u64)) -> Option<u64> {
            if self.unwatched {
                return None;
            }
            let mut epoch = self.epoch.borrow_mut();
            *epoch += 1;
            Some(*epoch)
        }

        fn written_since(&self, (base, _): (u64, u64), since: u64) -> Option<bool> {
            Some(
                self.written
                    .borrow()
                    .get(&base)
                    .is_some_and(|&at| at >= since),
            )
        }
    }

    const IMAGE: (u64, (u64, u64), u64) = (7, (0x1000, 0x100), 0x100);

    fn fetched(used: Use) -> Uses {
        Uses {
            fetched: Some(used),
            ..Uses::default()
        }
    }

    fn stored(used: Use) -> Uses {
        Uses {
            stored: vec![Some(used)],
            ..Uses::default()
        }
    }

    /// An image read for a dispatch that ran is taken by the next while its bytes are unwritten,
    /// and read afresh once they are written.
    #[test]
    fn an_image_read_is_taken_until_its_bytes_are_written() {
        let (watch, mut kept) = (Fake::default(), Registry::new(1 << 20));
        kept.begin();
        let first = kept.place(&watch, IMAGE).expect("watched");
        assert!(first.fresh);
        kept.ran(&watch, &fetched(first), true);
        kept.begin();
        let again = kept.place(&watch, IMAGE).expect("watched");
        assert_eq!(
            again,
            Use {
                fresh: false,
                ..first
            },
            "taken from the device"
        );
        kept.ran(&watch, &fetched(again), true);
        watch.write(0x1000);
        kept.begin();
        let read = kept.place(&watch, IMAGE).expect("watched");
        assert!(
            read.fresh && read.generation > first.generation,
            "read afresh"
        );
    }

    /// An image read for a dispatch that has not answered is not taken: a second preparation of
    /// the same dispatch reads it again.
    #[test]
    fn an_image_is_kept_only_once_its_dispatch_ran() {
        let (watch, mut kept) = (Fake::default(), Registry::new(1 << 20));
        kept.begin();
        let first = kept.place(&watch, IMAGE).expect("watched");
        kept.begin();
        assert!(kept.place(&watch, IMAGE).expect("watched").fresh);
        kept.begin();
        let failed = kept.place(&watch, IMAGE).expect("watched");
        kept.ran(&watch, &fetched(failed), false);
        assert!(
            kept.take_dropped().contains(&first.key),
            "the device lets it go"
        );
        kept.begin();
        assert!(kept.place(&watch, IMAGE).expect("watched").fresh);
    }

    /// An image stored to is kept from its write-back on, and not where something else wrote its
    /// bytes while it ran.
    #[test]
    fn a_stored_image_is_kept_from_its_write_back() {
        let (watch, mut kept) = (Fake::default(), Registry::new(1 << 20));
        kept.begin();
        let first = kept.place(&watch, IMAGE).expect("watched");
        kept.ran(&watch, &stored(first), true);
        assert_eq!(kept.entries[&first.key].state, State::Writing);
        // The write-back itself writes the bytes: the mark is taken after it.
        watch.write(0x1000);
        kept.written_back(&watch, &stored(first));
        kept.begin();
        let again = kept.place(&watch, IMAGE).expect("watched");
        assert!(!again.fresh, "taken");
        watch.write(0x1000);
        kept.ran(&watch, &stored(again), true);
        assert!(
            !kept.entries.contains_key(&first.key),
            "written while it ran"
        );
    }

    /// One image has one place in a dispatch, and memory that cannot be watched is never kept.
    #[test]
    fn an_image_takes_one_place_and_unwatched_memory_none() {
        let (watch, mut kept) = (Fake::default(), Registry::new(1 << 20));
        kept.begin();
        assert!(kept.place(&watch, IMAGE).is_some());
        assert!(kept.place(&watch, IMAGE).is_none(), "placed already");
        let unwatched = Fake {
            unwatched: true,
            ..Fake::default()
        };
        kept.begin();
        assert!(kept.place(&unwatched, IMAGE).is_none());
    }

    /// Past the budget the least recently bound image goes, never one of the dispatch being
    /// prepared, and the device is told.
    #[test]
    fn the_oldest_image_goes_past_the_budget() {
        let (watch, mut kept) = (Fake::default(), Registry::new(0x250));
        for (key, base) in [(1, 0x1000), (2, 0x2000)] {
            kept.begin();
            let used = kept
                .place(&watch, (key, (base, 0x100), 0x100))
                .expect("watched");
            kept.ran(&watch, &fetched(used), true);
        }
        kept.begin();
        let _ = kept.place(&watch, (3, (0x3000, 0x100), 0x100));
        assert_eq!(kept.take_dropped(), vec![1]);
        assert_eq!(kept.bytes(), 0x200);
        kept.begin();
        let _ = kept.place(&watch, (4, (0x4000, 0x400), 0x400));
        assert!(kept.entries.contains_key(&4), "the one being bound stays");
        assert!(!kept.entries.contains_key(&2) && !kept.entries.contains_key(&3));
    }
}
