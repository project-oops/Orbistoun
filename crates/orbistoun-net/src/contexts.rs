//! Library contexts the guest only compares and passes back (D151): integers from a base - one,
//! unless a measured library numbers its own from elsewhere - each retired once.

use std::collections::BTreeSet;
use std::sync::Mutex;

/// One kind of context: the ids issued and not yet retired.
pub(crate) struct Contexts {
    state: Mutex<(u32, BTreeSet<u32>)>,
}

impl Contexts {
    pub(crate) const fn new() -> Self {
        Self::from(1)
    }

    /// Ids numbered from `first`.
    pub(crate) const fn from(first: u32) -> Self {
        Self {
            state: Mutex::new((first, BTreeSet::new())),
        }
    }

    /// A fresh id, the next from the base; `None` only if the table is poisoned.
    pub(crate) fn issue(&self) -> Option<u32> {
        let mut state = self.state.lock().ok()?;
        let id = state.0;
        state.0 += 1;
        state.1.insert(id);
        Some(id)
    }

    /// Whether `id` was issued and is not yet retired.
    pub(crate) fn live(&self, id: u64) -> bool {
        let Ok(id) = u32::try_from(id) else {
            return false;
        };
        self.state.lock().is_ok_and(|state| state.1.contains(&id))
    }

    /// Retires `id`, answering whether it was live.
    pub(crate) fn retire(&self, id: u64) -> bool {
        let Ok(id) = u32::try_from(id) else {
            return false;
        };
        self.state.lock().is_ok_and(|mut state| state.1.remove(&id))
    }
}
