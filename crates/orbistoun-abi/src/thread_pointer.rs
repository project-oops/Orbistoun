//! Installing the thread pointer.
//!
//! Guest code reads thread-local variables through the `fs` segment base. Until that
//! base points at a real block, every `fs:`-relative access reads whatever the host left
//! there - which is not zero, not the guest's, and not detectable from inside the guest.
//!
//! # Why this is the last piece of loading rather than the first
//!
//! Nothing needed it. No commercial executable examined declares a single thread-local
//! relocation, so the layout work (D061) sat finished and unused. Threads change that:
//! every guest thread needs its own block and its own pointer, so the mechanism has to
//! exist before threading rather than alongside it.
//!
//! # It is not portable, and pretending otherwise would be the mistake
//!
//! Writing the `fs` base is a privileged operation made available to user code by an
//! optional processor feature (`FSGSBASE`) that the operating system must also enable.
//! Where it is available this is two instructions; where it is not there is no equivalent
//! this crate can reach, and the honest answer is to say so rather than to install
//! something that looks right.
//!
//! An emulator that silently ran a guest with a wrong thread pointer would produce
//! failures nothing could attribute, which is exactly what principle 3 exists to prevent.
//!
//! # The processor check is necessary and not sufficient
//!
//! `CPUID` says whether the processor *has* the feature; the operating system must also
//! have enabled it, and there is no way to observe that from user code. Executing the
//! instruction when it is disabled raises an illegal-instruction fault rather than
//! failing quietly - which the worker's fault reporter names, and which is why every
//! install is read back rather than assumed.

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
/// `FSGSBASE` is leaf 7, sub-leaf 0, bit 0 of `EBX`. The operating system must also have
/// enabled it, which this cannot observe - so a positive answer here means "the processor
/// can", not "it will work", and the write below is what settles it.
#[cfg(target_arch = "x86_64")]
pub fn processor_supports_base_writes() -> bool {
    // Safe on this target: the intrinsic is a pure query with no memory effects, and
    // leaf 7 predates every processor feature this project already requires.
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
/// # Errors
///
/// When the processor or the platform provides no way to do it.
///
/// # Safety
///
/// `address` must be the thread pointer of a live thread-local block that outlives every
/// guest access through it, and the block must be laid out as
/// `orbistoun_loader::tls` describes - the first word at `address` holding `address`
/// itself. A wrong value here makes every thread-local read in the guest return something
/// plausible and wrong.
///
/// Affects **only the calling thread**. Each guest thread installs its own.
#[cfg(target_arch = "x86_64")]
pub unsafe fn install(address: u64) -> Result<(), Unsupported> {
    if !processor_supports_base_writes() {
        return Err(Unsupported::NoProcessorSupport);
    }
    // SAFETY: the caller guarantees the address is a live thread-local block laid out as
    // this crate expects. `wrfsbase` is a single instruction affecting only this thread's
    // segment base, and the processor feature was just checked.
    //
    // Emitted as raw bytes rather than by mnemonic: the intrinsic is unstable on this
    // toolchain, and spelling `wrfsbase` needs the assembler to have the feature enabled
    // for the whole translation unit - which would let the compiler emit it elsewhere.
    // The encoding is fixed, so the bytes are as clear as the name and depend on nothing.
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
/// Used to check an install rather than to trust it: the write is a single instruction
/// with no result, so the only way to know it took effect is to read it back.
#[cfg(target_arch = "x86_64")]
pub fn current() -> Option<u64> {
    if !processor_supports_base_writes() {
        return None;
    }
    let base: u64;
    // SAFETY: a pure read of this thread's own segment base, guarded by the same feature
    // check the write uses. Raw bytes for the same reason as the write.
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

    #[test]
    fn an_install_can_be_read_back_or_is_refused() {
        // The property that matters: either it worked and the base reads back exactly, or
        // it was refused. What must never happen is a silent success that left the base
        // unchanged - a guest running on that reads plausible, wrong thread-locals and
        // nothing can attribute the failure.
        let mut block = [0_u64; 8];
        let address = block.as_mut_ptr() as usize as u64;
        let restore = current();

        // SAFETY: `block` is a live, correctly aligned allocation that outlives this test,
        // and nothing here reads through the segment base afterwards.
        let outcome = unsafe { install(address) };

        match outcome {
            Ok(()) => {
                assert_eq!(
                    current(),
                    Some(address),
                    "an install that reports success must be observable"
                );
                if let Some(previous) = restore {
                    // Put the host's own base back: this thread belongs to the test
                    // harness, and leaving it pointing at a stack array would break
                    // whatever runs next on it.
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

    #[test]
    fn support_is_reported_consistently() {
        // `current` and `install` must agree about whether this is possible, or a caller
        // gets a base it cannot read back and no explanation.
        assert_eq!(
            processor_supports_base_writes(),
            current().is_some(),
            "the reader and the feature check must agree"
        );
    }
}
