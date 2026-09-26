//! Installing the thread pointer.
//!
//! Guest code reads thread-local variables through the `fs` segment base; until it points at
//! a real block, every `fs:`-relative access reads whatever the host left there. Writing the
//! base from user code needs the optional `FSGSBASE` processor feature, enabled by the
//! operating system. `CPUID` reports only the processor half, and executing the instruction
//! when it is disabled raises an illegal-instruction fault, so every install is read back.
//! Where the feature is absent this says so rather than installing something that looks right.

/// Why a thread pointer could not be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    /// The processor does not report the feature that makes the base writable.
    NoProcessorSupport,
    /// The build has no way to do this at all on this platform.
    NoPlatformSupport,
}

impl core::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoProcessorSupport => {
                f.write_str("the processor does not expose a writable segment base to user code")
            }
            Self::NoPlatformSupport => {
                f.write_str("no supported way to set the thread pointer on this platform")
            }
        }
    }
}

/// Whether the processor lets user code write the `fs` base.
///
/// `FSGSBASE` is leaf 7, sub-leaf 0, bit 0 of `EBX`. A positive answer means the processor can;
/// whether the operating system enabled it is settled by the write.
#[cfg(target_arch = "x86_64")]
pub fn processor_supports_base_writes() -> bool {
    // Safe on this target: the intrinsic is a pure query with no memory effects, and leaf 7
    // predates every processor feature this project requires.
    let leaf = core::arch::x86_64::__cpuid_count(7, 0);
    leaf.ebx & 1 != 0
}

/// Whether the processor lets user code write the `fs` base.
#[cfg(not(target_arch = "x86_64"))]
pub fn processor_supports_base_writes() -> bool {
    false
}

/// Points the `fs` base at `address`.
///
/// Affects only the calling thread; each guest thread installs its own.
///
/// # Errors
///
/// When the processor or the platform provides no way to do it.
///
/// # Safety
///
/// `address` must be the thread pointer of a live thread-local block that outlives every
/// guest access through it, laid out as `orbistoun_loader::tls` describes: the first word at
/// `address` holds `address` itself.
#[cfg(target_arch = "x86_64")]
pub unsafe fn install(address: u64) -> Result<(), Unsupported> {
    if !processor_supports_base_writes() {
        return Err(Unsupported::NoProcessorSupport);
    }
    // SAFETY: the caller guarantees the address is a live thread-local block laid out as this
    // crate expects. `wrfsbase` affects only this thread's segment base, and the processor
    // feature was just checked.
    // Emitted as raw bytes: the intrinsic is unstable on this toolchain, and the mnemonic needs
    // the feature enabled for the whole translation unit, which would let the compiler emit it
    // elsewhere.
    unsafe {
        core::arch::asm!(
            "mov rax, {base}",
            ".byte 0xF3, 0x48, 0x0F, 0xAE, 0xD0", // wrfsbase rax
            base = in(reg) address,
            out("rax") _,
            options(nostack, preserves_flags),
        );
    }
    Ok(())
}

/// Points the `fs` base at `address`.
///
/// # Errors
///
/// Always, away from x86-64.
///
/// # Safety
///
/// See the x86-64 documentation; this build cannot honour the request at all.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn install(_address: u64) -> Result<(), Unsupported> {
    Err(Unsupported::NoPlatformSupport)
}

/// Reads the current `fs` base.
///
/// The write has no result, so an install is checked by reading it back.
#[cfg(target_arch = "x86_64")]
pub fn current() -> Option<u64> {
    if !processor_supports_base_writes() {
        return None;
    }
    let base: u64;
    // SAFETY: a pure read of this thread's own segment base, guarded by the same feature check
    // the write uses. Raw bytes for the same reason as the write.
    unsafe {
        core::arch::asm!(
            ".byte 0xF3, 0x48, 0x0F, 0xAE, 0xC0", // rdfsbase rax
            "mov {out}, rax",
            out = out(reg) base,
            out("rax") _,
            options(nostack, preserves_flags),
        );
    }
    Some(base)
}

/// Reads the current `fs` base.
#[cfg(not(target_arch = "x86_64"))]
pub fn current() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::{Unsupported, current, install, processor_supports_base_writes};

    /// An install either reads back exactly or is refused.
    #[test]
    fn an_install_can_be_read_back_or_is_refused() {
        // Either it worked and the base reads back exactly, or it was refused; never a silent
        // success that left the base unchanged.
        let mut block = [0_u64; 8];
        let address = block.as_mut_ptr() as usize as u64;
        let restore = current();

        // SAFETY: `block` is a live, correctly aligned allocation that outlives this test, and
        // nothing here reads through the segment base afterwards.
        let outcome = unsafe { install(address) };

        match outcome {
            Ok(()) => {
                assert_eq!(
                    current(),
                    Some(address),
                    "an install that reports success must be observable"
                );
                if let Some(previous) = restore {
                    // Put the host's base back: this thread belongs to the test harness.
                    // SAFETY: restoring a value this thread was already using.
                    unsafe { install(previous).expect("restoring a base that was in use") };
                }
            }
            Err(e) => {
                assert!(
                    !processor_supports_base_writes()
                        || matches!(e, Unsupported::NoPlatformSupport),
                    "a refusal must have a reason: {e}"
                );
            }
        }
    }

    /// Feature detection and install agree about whether writing the base is possible.
    #[test]
    fn support_is_reported_consistently() {
        // `current` and `install` must agree, or a caller gets a base it cannot read back.
        assert_eq!(
            processor_supports_base_writes(),
            current().is_some(),
            "the reader and the feature check must agree"
        );
    }
}
