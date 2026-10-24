//! Where in guest memory the dispatch about to run binds its window and buffers.
//!
//! The command processor knows each binding's guest range and hands the executor only the words,
//! so it notes the ranges here first, by module, and a backend's diagnostics read them for the
//! dispatch they are running: a capture records them, a watch finds the words a dispatch changed in
//! a range. The note is kept in place, so noting allocates only when a dispatch binds more buffers
//! than any before it.

use std::sync::Mutex;

/// A dispatch's guest ranges, each its first byte and its length in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoundRanges {
    /// The window at binding 1, where the program addresses one.
    pub window: Option<(u64, u64)>,
    /// Each buffer's, by slot; an alias's is the range it was asked for.
    pub buffers: Vec<(u64, u64)>,
}

/// The latest note: the module hash the ranges were noted for, and the ranges.
#[derive(Debug)]
struct Note {
    module: u64,
    ranges: BoundRanges,
}

impl Note {
    const fn new() -> Self {
        Self {
            module: 0,
            ranges: BoundRanges {
                window: None,
                buffers: Vec::new(),
            },
        }
    }

    fn set(&mut self, module: u64, window: Option<(u64, u64)>, buffers: &[(u64, u64)]) {
        self.module = module;
        self.ranges.window = window;
        self.ranges.buffers.clear();
        self.ranges.buffers.extend_from_slice(buffers);
    }

    fn get(&self, module: u64) -> Option<BoundRanges> {
        let placed = self.ranges.window.is_some() || !self.ranges.buffers.is_empty();
        (self.module == module && placed).then(|| self.ranges.clone())
    }
}

static NOTED: Mutex<Note> = Mutex::new(Note::new());

/// Notes the ranges of the next dispatch of the module hashed `module`.
pub fn note(module: u64, window: Option<(u64, u64)>, buffers: &[(u64, u64)]) {
    NOTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .set(module, window, buffers);
}

/// The ranges noted for a dispatch of the module hashed `module`; `None` where the latest note was
/// for another module, or none was made.
pub fn noted(module: u64) -> Option<BoundRanges> {
    NOTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(module)
}

#[cfg(test)]
mod tests {
    use super::{BoundRanges, Note};

    /// A note answers for its own module only, and the next note replaces it whole.
    #[test]
    fn a_note_answers_for_its_module() {
        let mut note = Note::new();
        assert_eq!(note.get(0), None, "nothing noted");
        note.set(7, Some((0x1000, 0x40)), &[(0x2000, 8), (0x3000, 16)]);
        assert_eq!(note.get(8), None);
        assert_eq!(
            note.get(7),
            Some(BoundRanges {
                window: Some((0x1000, 0x40)),
                buffers: vec![(0x2000, 8), (0x3000, 16)],
            })
        );
        note.set(7, None, &[(0x4000, 4)]);
        assert_eq!(
            note.get(7),
            Some(BoundRanges {
                window: None,
                buffers: vec![(0x4000, 4)],
            })
        );
    }
}
