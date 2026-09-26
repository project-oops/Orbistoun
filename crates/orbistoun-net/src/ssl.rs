//! `libSceSsl` - TLS.
//!
//! `sceSslInit` and `sceSslTerm` bracket a context `sceHttpInit` is handed. The TLS itself is the
//! host's, carried by the HTTP transfer (`http.rs`), so a context is an id that is live between
//! the two calls and holds nothing. The certificate inspectors are declared and unserved.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceSsl" {
        "sceSslFreeSslCertName" => 6,
        "sceSslGetCaCerts" => 6,
        "sceSslGetIssuerName" => 6,
        "sceSslGetMemoryPoolStats" => 6,
        "sceSslGetNameEntryCount" => 6,
        "sceSslGetNameEntryInfo" => 6,
        "sceSslGetSerialNumber" => 6,
        "sceSslGetSubjectName" => 6,
        // (pool size)
        "sceSslInit" => 1,
        // (context)
        "sceSslTerm" => 1,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceSslInit", init), ("sceSslTerm", term)]
}

fn live() -> std::sync::MutexGuard<'static, HashSet<u32>> {
    static LIVE: std::sync::OnceLock<Mutex<HashSet<u32>>> = std::sync::OnceLock::new();
    LIVE.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// `sceSslInit(poolSize)`: a positive context id.
fn init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    live().insert(id);
    u64::from(id)
}

/// `sceSslTerm(context)`.
fn term(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if live().remove(&(args[0] as u32)) {
        0
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}
