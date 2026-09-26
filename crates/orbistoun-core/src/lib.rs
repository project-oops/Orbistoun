//! orbistoun's domain layer - IO-free types shared by every other crate.
//!
//! The bottom of the dependency graph: guest error codes, opaque handles, and the ABI
//! primitives that cross the guest/host boundary. It holds types with no behaviour beyond
//! conversion - no IO, no parsing, no knowledge of a specific vendor library. Guest error
//! codes are modelled here once because a wrong error code is the most common cause of a
//! guest hanging long after the call; see [`GuestError`].

mod error;
mod handle;

pub mod category;
pub mod entropy;
pub mod klog;
pub mod machine;
pub mod park;
pub mod route;
pub mod said;
pub mod stop;

pub use error::{
    GuestError, GuestResult, PLACEHOLDER_BASE, VENDOR_ERROR_BASE, errno, placeholder_named,
};
pub use handle::{Handle, HandleAllocator};
pub use stop::{StopReason, stop};

/// Guest page size, fixed by the platform ABI: a host with 16K pages still presents 4K
/// semantics to the guest.
pub const GUEST_PAGE_SIZE: u64 = 4096;

/// Integer argument registers the guest calling convention passes in.
///
/// System V x86-64: `rdi, rsi, rdx, rcx, r8, r9`. Arguments from the seventh onwards are on
/// the guest stack and are not presented to an implementation here.
pub const GUEST_ARG_REGISTERS: usize = 6;

/// Floating-point argument registers the guest calling convention passes in.
///
/// System V x86-64: `xmm0` through `xmm7`. A `double` or `float` argument travels here and
/// never in the integer registers (D268). Only the low 64 bits of each are carried: a
/// `double`, with a `float` in its low half; no function declared here takes a vector.
pub const GUEST_FLOAT_REGISTERS: usize = 8;

/// A host implementation of a guest function.
///
/// Takes the argument registers as the boundary spilled them and returns what the guest sees
/// in `rax`. Plain Rust: the calling convention is handled at the one boundary that owns it.
/// Declared in this dependency-free crate so subsystems and the thunk layer share the shape.
pub type GuestFn = fn(args: &[u64; GUEST_ARG_REGISTERS]) -> u64;

/// A guest function that speaks in floating-point registers.
///
/// A second type so that [`GuestFn`] carries no unused floating-point array. Returns the raw
/// bits for `xmm0` rather than an `f64`, so `sqrtf` can answer a `float` in the same register
/// (D268).
pub type GuestFloatFn =
    fn(ints: &[u64; GUEST_ARG_REGISTERS], floats: &[u64; GUEST_FLOAT_REGISTERS]) -> u64;

/// Alignment required of a direct-memory allocation, in bytes.
///
/// Guest allocators assume it and corrupt themselves silently if handed less, so it is
/// asserted at the allocation site.
pub const DIRECT_MEMORY_ALIGN: u64 = 16 * 1024;
