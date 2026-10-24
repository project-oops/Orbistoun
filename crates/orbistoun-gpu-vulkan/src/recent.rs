//! The run's latest guest dispatches, kept so a loss of the device writes out the history that led
//! to it: `ORBISTOUN_CAPTURE_RECENT`.
//!
//! Each dispatch that completes is copied, from the inputs it was handed, into a ring bounded by a
//! count and by [`MOST_BYTES`]. A module is held once however many dispatches ran it. A dispatch
//! whose inputs alone pass the byte bound is not copied; the ring records that it was passed over,
//! so the history says where it has a gap. When a dispatch loses the device, every dispatch the
//! ring holds is written to `dispatch-<program>-r<ordinal>/`, and [`INDEX`] lists them in the order
//! they ran, the passed-over ones with them. Unset, nothing is copied.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::capture::{Capture, Reason};

/// The most bytes the ring holds, modules included.
pub(crate) const MOST_BYTES: u64 = 256 << 20;

/// The file in the frames directory that lists what a loss wrote from the ring.
pub(crate) const INDEX: &str = "dispatch-recent.txt";

/// One of the latest dispatches.
#[derive(Debug)]
pub(crate) enum Entry {
    /// Kept: its inputs, its module left out and named by its hash.
    Kept(Capture<'static>, u64),
    /// Passed over: its ordinal, program and the bytes its inputs would have taken.
    Skipped(Option<u64>, Option<u64>, u64),
}

/// The latest dispatches, the oldest first, within a count and a byte bound.
#[derive(Debug)]
pub(crate) struct Ring {
    most: usize,
    most_bytes: u64,
    entries: VecDeque<Entry>,
    modules: HashMap<u64, Vec<u32>>,
    bytes: u64,
    /// Copies made, the bytes they copied and the time they took, over the run.
    cost: (u64, u64, Duration),
}

impl Ring {
    /// A ring of at most `most` dispatches and `most_bytes` bytes.
    pub(crate) fn new(most: usize, most_bytes: u64) -> Self {
        Self {
            most: most.max(1),
            most_bytes,
            entries: VecDeque::new(),
            modules: HashMap::new(),
            bytes: 0,
            cost: (0, 0, Duration::ZERO),
        }
    }

    /// Keeps a copy of `capture`, whose module hashes to `module`, letting the oldest go to make
    /// room; or records it passed over where it could not fit alone. Answers whether it was kept.
    pub(crate) fn push(&mut self, capture: &Capture<'_>, module: u64) -> bool {
        let module_bytes = capture.module.len() as u64 * 4;
        let data = capture.bytes() - module_bytes;
        let wanted = |ring: &Self| {
            data + if ring.modules.contains_key(&module) {
                0
            } else {
                module_bytes
            }
        };
        if data + module_bytes > self.most_bytes {
            if self.entries.len() >= self.most {
                self.drop_oldest();
            }
            self.entries.push_back(Entry::Skipped(
                capture.ordinal,
                capture.program,
                data + module_bytes,
            ));
            return false;
        }
        while self.entries.len() >= self.most || self.bytes + wanted(self) > self.most_bytes {
            self.drop_oldest();
        }
        let started = Instant::now();
        let copied = wanted(self);
        if let std::collections::hash_map::Entry::Vacant(vacant) = self.modules.entry(module) {
            vacant.insert(capture.module.to_vec());
            self.bytes += module_bytes;
        }
        self.entries
            .push_back(Entry::Kept(capture.owned_without_module(), module));
        self.bytes += data;
        self.cost.0 += 1;
        self.cost.1 += copied;
        self.cost.2 += started.elapsed();
        true
    }

    /// Lets the oldest entry go, and its module where no later entry ran it.
    fn drop_oldest(&mut self) {
        let Some(oldest) = self.entries.pop_front() else {
            return;
        };
        if let Entry::Kept(capture, module) = oldest {
            self.bytes -= capture.bytes();
            let still_run = self
                .entries
                .iter()
                .any(|entry| matches!(entry, Entry::Kept(_, held) if *held == module));
            if !still_run && let Some(words) = self.modules.remove(&module) {
                self.bytes -= words.len() as u64 * 4;
            }
        }
    }

    /// What it holds, the oldest first.
    pub(crate) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// A kept entry as a whole capture, its module put back.
    pub(crate) fn whole<'r>(&'r self, capture: &'r Capture<'static>, module: u64) -> Capture<'r> {
        Capture {
            module: std::borrow::Cow::Borrowed(
                self.modules.get(&module).map_or(&[][..], Vec::as_slice),
            ),
            reason: Reason::Recent,
            ..capture.clone()
        }
    }

    /// The bytes it holds now.
    pub(crate) const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Copies made over the run, the bytes they copied and the time they took.
    pub(crate) const fn cost(&self) -> (u64, u64, Duration) {
        self.cost
    }
}

/// The count a value of `ORBISTOUN_CAPTURE_RECENT` asks for; `None` for none, and an error naming
/// a value that is not a count.
pub(crate) fn parse(value: Option<&str>) -> Result<Option<usize>, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|&count| count > 0)
            .map(Some)
            .ok_or_else(|| value.to_owned()),
    }
}

/// The run's ring, made from the environment on first use; `None` where it is not asked for.
fn ring() -> Option<&'static std::sync::Mutex<Ring>> {
    static RING: std::sync::OnceLock<Option<std::sync::Mutex<Ring>>> = std::sync::OnceLock::new();
    RING.get_or_init(|| {
        let asked = parse(orbistoun_env::CAPTURE_RECENT.get().as_deref()).unwrap_or_else(|value| {
            tracing::warn!(
                "{} is {value:?}, which is not a count; no dispatch is kept",
                orbistoun_env::CAPTURE_RECENT.name
            );
            None
        })?;
        tracing::info!(
            "keeping the latest {asked} dispatches, at most {} MiB, for a loss of the device to write out",
            MOST_BYTES >> 20
        );
        Some(std::sync::Mutex::new(Ring::new(asked, MOST_BYTES)))
    })
    .as_ref()
}

/// How often the copying's cost is said, in copies.
const COST_SAID_EVERY: u64 = 4096;

/// The cost of the copies a ring made, as a line says it.
fn cost_line(ring: &Ring) -> String {
    let (copies, bytes, took) = ring.cost();
    let each = took.as_micros() / u128::from(copies.max(1));
    format!(
        "keeping the latest dispatches: {copies} copied, {} MiB, {:.1} ms in all, {each} us each",
        bytes >> 20,
        took.as_secs_f64() * 1000.0
    )
}

/// Whether the run keeps its latest dispatches.
pub(crate) fn kept() -> bool {
    ring().is_some()
}

/// Keeps a copy of `capture`, of the module hashed `module`, among the latest, where the run asks
/// for them.
pub(crate) fn keep(capture: &Capture<'_>, module: u64) {
    let Some(ring) = ring() else {
        return;
    };
    let mut ring = ring
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !ring.push(capture, module) {
        tracing::debug!(
            "the run's dispatch {} is not kept among the latest: its inputs are {} bytes, past {} MiB",
            capture.ordinal.unwrap_or_default(),
            capture.bytes(),
            MOST_BYTES >> 20
        );
    }
    let copies = ring.cost().0;
    if copies > 0 && copies % COST_SAID_EVERY == 0 {
        tracing::info!("{}", cost_line(&ring));
    }
}

/// The index of what a loss wrote: a line for each of the latest, the oldest first, naming its
/// directory or saying it was passed over.
fn index(ring: &Ring) -> String {
    let mut text =
        String::from("# the dispatches before the loss, oldest first: ordinal, program, capture\n");
    for entry in ring.entries() {
        let line = match entry {
            Entry::Kept(capture, module) => format!(
                "{} {} {}",
                capture.ordinal.unwrap_or_default(),
                program_text(capture.program),
                crate::capture::directory_name(
                    capture.program,
                    *module,
                    (Reason::Recent, capture.ordinal)
                )
            ),
            Entry::Skipped(ordinal, program, bytes) => format!(
                "{} {} not kept: {bytes} bytes of inputs",
                ordinal.unwrap_or_default(),
                program_text(*program)
            ),
        };
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// A program as the index names it.
fn program_text(program: Option<u64>) -> String {
    program.map_or_else(|| "none".to_owned(), |p| format!("{p:#x}"))
}

/// Writes every dispatch the ring holds, and its index, where the run keeps its frames, for a loss
/// of the device; nothing where the run keeps none.
pub(crate) fn write_on_loss() {
    let Some(ring) = ring() else {
        return;
    };
    let ring = ring
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for entry in ring.entries() {
        if let Entry::Kept(capture, module) = entry {
            crate::capture::write(&ring.whole(capture, *module), *module, capture.ordinal);
        }
    }
    let path = crate::capture::frame_dir().join(INDEX);
    match std::fs::write(&path, index(&ring)) {
        Ok(()) => tracing::info!(
            "wrote the {} dispatches before the loss ({} MiB held); {} lists them; {}",
            ring.entries().count(),
            ring.bytes() >> 20,
            path.display(),
            cost_line(&ring)
        ),
        Err(e) => tracing::warn!("{} could not be written: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Ring, index, parse};
    use crate::capture::{Capture, Reason};
    use std::borrow::Cow;

    /// A capture of `words` words of memory and a module of `module` words, at `ordinal`.
    fn capture(ordinal: u64, words: usize, module: usize) -> Capture<'static> {
        Capture {
            program: Some(0x3dc_9a00),
            ordinal: Some(ordinal),
            memory: Cow::Owned(vec![ordinal as u32; words]),
            module: Cow::Owned(vec![7; module]),
            ..Capture::of(
                (None, Reason::Asked),
                &[],
                &[],
                (&[], [1, 1, 1]),
                &crate::compute_images::DispatchImages::default(),
                &[],
            )
        }
    }

    fn ordinals(ring: &Ring) -> Vec<(u64, bool)> {
        ring.entries()
            .map(|entry| match entry {
                Entry::Kept(capture, _) => (capture.ordinal.unwrap_or_default(), true),
                Entry::Skipped(ordinal, _, _) => (ordinal.unwrap_or_default(), false),
            })
            .collect()
    }

    /// The ring keeps the latest by count, the oldest let go first.
    #[test]
    fn the_ring_keeps_the_latest_by_count() {
        let mut ring = Ring::new(3, 1 << 20);
        for ordinal in 1..=5 {
            assert!(ring.push(&capture(ordinal, 4, 2), 9));
        }
        assert_eq!(ordinals(&ring), [(3, true), (4, true), (5, true)]);
        assert_eq!(ring.bytes(), 3 * 16 + 8, "the module is held once");
    }

    /// The ring lets the oldest go to stay within its bytes, a module going with the last entry
    /// that ran it.
    #[test]
    fn the_ring_stays_within_its_bytes() {
        let mut ring = Ring::new(10, 100);
        assert!(ring.push(&capture(1, 10, 2), 1));
        assert!(ring.push(&capture(2, 10, 2), 2));
        assert_eq!(ring.bytes(), 96);
        assert!(ring.push(&capture(3, 4, 2), 2));
        assert_eq!(ordinals(&ring), [(2, true), (3, true)]);
        assert_eq!(
            ring.bytes(),
            40 + 8 + 16,
            "the first module went with its entry"
        );
    }

    /// A dispatch whose inputs alone pass the bound is not kept, and the ring says so where it
    /// fell.
    #[test]
    fn a_dispatch_too_large_to_keep_is_recorded_passed_over() {
        let mut ring = Ring::new(4, 100);
        assert!(ring.push(&capture(1, 4, 1), 1));
        assert!(!ring.push(&capture(2, 40, 1), 1));
        assert!(ring.push(&capture(3, 4, 1), 1));
        assert_eq!(ordinals(&ring), [(1, true), (2, false), (3, true)]);
        let index = index(&ring);
        assert!(
            index.contains("\n2 0x3dc9a00 not kept: 164 bytes of inputs\n"),
            "{index}"
        );
        assert!(
            index.contains("\n3 0x3dc9a00 dispatch-3dc9a00-r3\n"),
            "{index}"
        );
    }

    /// A kept entry is put back whole: its inputs and its module, as a recent capture.
    #[test]
    fn a_kept_entry_comes_back_whole() {
        let mut ring = Ring::new(2, 1 << 20);
        let original = capture(4, 3, 5);
        assert!(ring.push(&original, 11));
        let Some(Entry::Kept(kept, module)) = ring.entries().next() else {
            panic!("kept");
        };
        let whole = ring.whole(kept, *module);
        assert_eq!(
            whole,
            Capture {
                reason: Reason::Recent,
                ..original
            }
        );
    }

    /// The count is a positive number; unset or blank asks for none.
    #[test]
    fn the_count_is_a_positive_number() {
        assert_eq!(parse(None), Ok(None));
        assert_eq!(parse(Some(" ")), Ok(None));
        assert_eq!(parse(Some("32")), Ok(Some(32)));
        assert_eq!(parse(Some("0")), Err("0".to_owned()));
        assert_eq!(parse(Some("many")), Err("many".to_owned()));
    }
}
