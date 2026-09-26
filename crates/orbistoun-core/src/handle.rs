//! Opaque guest handles.

/// An opaque identifier handed to the guest in place of a host pointer.
///
/// A guest cannot corrupt host memory through an integer, and a stale handle produces
/// [`crate::GuestError::InvalidHandle`] instead of a use-after-free. Values start at one so
/// zero is always invalid, since guests zero-initialise handle fields and then check them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handle(u32);

impl Handle {
    /// Wraps a raw non-zero value received back from the guest.
    ///
    /// Returns `None` for zero, which is never a valid handle.
    pub const fn from_raw(raw: u32) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }

    /// The raw value to hand to the guest.
    pub const fn as_raw(self) -> u32 {
        self.0
    }
}

/// Allocates sequential handles for one subsystem.
///
/// Per-subsystem so that passing an audio handle to a file call is caught rather than hidden
/// in a shared number space.
#[derive(Debug)]
pub struct HandleAllocator {
    next: u32,
}

impl HandleAllocator {
    /// Creates an allocator whose first handle is one.
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// Issues the next handle, or `None` once the space is exhausted.
    ///
    /// Handles are never recycled: reuse makes a stale-handle bug look like a valid access to the
    /// wrong object.
    pub const fn alloc(&mut self) -> Option<Handle> {
        match self.next.checked_add(1) {
            Some(next) => {
                let h = Handle(self.next);
                self.next = next;
                Some(h)
            }
            None => None,
        }
    }
}

impl Default for HandleAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Handle, HandleAllocator};

    /// Zero never wraps into a handle.
    #[test]
    fn zero_is_never_a_handle() {
        assert!(Handle::from_raw(0).is_none());
        assert!(Handle::from_raw(1).is_some());
    }

    /// Handles are issued sequentially from one.
    #[test]
    fn handles_are_sequential_from_one() {
        let mut a = HandleAllocator::new();
        assert_eq!(a.alloc().map(Handle::as_raw), Some(1));
        assert_eq!(a.alloc().map(Handle::as_raw), Some(2));
    }
}
