//! `sceNetPoolCreate` and `sceNetPoolDestroy`: the memory pools the network libraries account
//! against.
//!
//! The pool is the platform's bookkeeping for its own allocations; the host allocates for its own
//! sockets and transfers, so a pool is an id that is live between the two calls and holds nothing.
//! Declared with the rest of `libSceNet` in `socket.rs`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceNetPoolCreate", create), ("sceNetPoolDestroy", destroy)]
}

fn live() -> std::sync::MutexGuard<'static, HashSet<u32>> {
    static LIVE: std::sync::OnceLock<Mutex<HashSet<u32>>> = std::sync::OnceLock::new();
    LIVE.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// `sceNetPoolCreate(name, size, flags)`: a positive pool id.
fn create(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    live().insert(id);
    u64::from(id)
}

/// `sceNetPoolDestroy(pool)`.
fn destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if live().remove(&(args[0] as u32)) {
        0
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}
