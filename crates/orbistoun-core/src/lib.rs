//! orbistoun's domain layer - IO-free types shared by every other crate.
//!
//! This crate is the bottom of the dependency graph. It defines the vocabulary
//! that the loader, the HLE modules, and the trace sink all speak: guest error
//! codes, opaque handles, and the ABI primitives that cross the guest/host
//! boundary.
//!
//! What lives here: types with no behaviour beyond conversion. What does NOT
//! live here: any IO, any parsing, any knowledge of a specific vendor
//! library. Adding a runtime dependency to this crate is a smell - push it up a
//! layer.
//!
//! # Why guest error codes are a domain type
//!
//! A stub that returns the wrong error code is the single most common cause of a
//! guest hanging thousands of frames later, so the codes are modelled once,
//! centrally, rather than sprinkled as integer literals across the HLE crates.
//! See [`GuestError`].

mod error;
mod handle;

pub mod klog;
pub mod machine;
pub mod park;
pub mod stop;

pub use error::{GuestError, GuestResult, VENDOR_ERROR_BASE, errno};
pub use handle::{Handle, HandleAllocator};
pub use stop::{StopReason, stop};

/// Guest page size. Fixed by the platform ABI, not by the host - a host with 16K
/// pages still has to present 4K semantics to the guest.
pub const GUEST_PAGE_SIZE: u64 = 4096;

/// Integer argument registers the guest calling convention passes in.
///
/// System V x86-64: `rdi, rsi, rdx, rcx, r8, r9`. A seventh argument onwards is on the
/// guest stack and is not presented to an implementation here.
pub const GUEST_ARG_REGISTERS: usize = 6;

/// Floating-point argument registers the guest calling convention passes in.
///
/// System V x86-64: `xmm0` through `xmm7`. A `double` or `float` argument travels here and
/// **never** in the integer registers, which is why a maths function saw nothing at all
/// until these were carried: the guest put 4.0 in `xmm0`, the handler read six integer
/// registers that did not contain it, and answered in `rax`, which the guest was not
/// reading. `sqrt(4)` returned 4 - the guest's own argument, still sitting in `xmm0`
/// because nothing had written it (D268).
///
/// Only the low 64 bits of each are carried. That is a `double`, and a `float` is its low
/// half; the upper halves belong to vector arguments, which no function declared here
/// takes and which would need their own accounting to present honestly.
pub const GUEST_FLOAT_REGISTERS: usize = 8;

/// A host implementation of a guest function.
///
/// Takes the argument registers as the boundary spilled them, returns what the guest
/// will see in `rax`. Plain Rust rather than `extern "sysv64"`: by the time one of these
/// is called the convention work is already done, and putting the ABI in this type would
/// spread it across every subsystem crate instead of keeping it at the one boundary that
/// owns it.
///
/// Declared here, in the crate with no dependencies, because both the subsystems that
/// write these and the thunk layer that calls them need to agree on the shape - and
/// neither should have to depend on the other to do it.
pub type GuestFn = fn(args: &[u64; GUEST_ARG_REGISTERS]) -> u64;

/// A guest function that speaks in floating-point registers.
///
/// # Why this is a second type rather than a wider first one
///
/// Almost nothing needs it. Widening [`GuestFn`] would put an unused floating-point array
/// in the signature of every implemented function, and the busiest import in the corpus is
/// called ninety-nine million times without ever touching one.
///
/// Returns the **raw bits** for `xmm0` rather than an `f64`, so this type says nothing
/// about how a subsystem interprets them - `sqrtf` answers a `float` in the same register
/// and would otherwise have to lie about its own return type (D268).
pub type GuestFloatFn =
    fn(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64;

/// Alignment required of a direct-memory allocation, in bytes.
///
/// Guest allocators assume this and will corrupt themselves silently if handed
/// something less aligned, which is why it is asserted at the allocation site
/// rather than trusted.
pub const DIRECT_MEMORY_ALIGN: u64 = 16 * 1024;
