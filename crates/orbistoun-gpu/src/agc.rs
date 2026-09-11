//! The current generation's graphics API - libSceAgc and libSceAgcDriver.
//!
//! # Why this exists as names and nothing else
//!
//! This crate declared `libSceGnmDriver` - the **previous** generation's interface - and
//! nothing at all for the one titles on this generation actually call. A survey of what
//! guests ask for found six `Agc` functions being called with no declaration anywhere in
//! the project, so they were reported as `unknown::` and could not be counted, named or
//! stubbed (D500).
//!
//! # Where these names come from
//!
//! **A real module's own import table.** PPSA02664's executable imports fifty-one names
//! from `libSceAgc` and five from `libSceAgcDriver`; five further `libSceAgcDriver` names
//! appear in the recorded corpus from other modules. Every name below is one of those.
//! None is derived from a pattern and hoped to match - which is the same provenance
//! `orbistoun-input` documents for `libScePad`, and the strongest available without a
//! console.
//!
//! # Status: names confirmed, arities not
//!
//! The asymmetry is the one `orbistoun-input` states and it holds for the same reason: a
//! wrong arity degrades a call trace and cannot break a call - it is carried only into
//! reports and trace shape, never into the call path - while a wrong *name* is a NID that
//! matches no import and a shim nothing can reach.
//!
//! **Every arity here is 6, and that is not a claim that these take six arguments.** Six is
//! the trampoline's full capture. With nothing established, recording every argument
//! register loses no information, where guessing low silently discards the arguments - and
//! for a command-buffer interface the arguments are buffer addresses and sizes, which is
//! precisely what a trace of it is for.
//!
//! Nothing here is implemented. That is deliberate: `docs/ROADMAP.md` puts the first frame
//! at Phase 6, which has not begun, and principle 6 puts a subsystem after the address
//! space and threads. What this buys now is that a guest reaching the graphics interface is
//! **named and counted** instead of vanishing into `unknown::`, and that the loud stub
//! policy answers it rather than a placeholder a caller might read as a handle (D125).

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAgc" {
        "sceAgcAcbAcquireMem" => 6,
        "sceAgcAcbDispatchIndirect" => 6,
        "sceAgcAcbDmaData" => 6,
        "sceAgcAcbEventWrite" => 6,
        "sceAgcAcbResetQueue" => 6,
        "sceAgcAcbWaitRegMem" => 6,
        "sceAgcAcbWriteData" => 6,
        "sceAgcCbDispatch" => 6,
        "sceAgcCbNop" => 6,
        "sceAgcCbReleaseMem" => 6,
        "sceAgcCbSetShRegisterRangeDirect" => 6,
        "sceAgcCbSetShRegistersDirect" => 6,
        "sceAgcCreatePrimState" => 6,
        // **Four, not six: the knowledge base already recorded an arity for this one** and
        // the declaration defers to it. `declared_arity_and_recorded_arity_never_disagree`
        // is the guard that said so, and it was right to - the record is the older claim.
        "sceAgcCreateShader" => 4,
        "sceAgcDcbAcquireMem" => 6,
        "sceAgcDcbDispatchIndirect" => 6,
        "sceAgcDcbDmaData" => 6,
        "sceAgcDcbDrawIndex" => 6,
        "sceAgcDcbDrawIndexAuto" => 6,
        "sceAgcDcbDrawIndexIndirect" => 6,
        "sceAgcDcbDrawIndirect" => 6,
        "sceAgcDcbEventWrite" => 6,
        "sceAgcDcbPopMarker" => 6,
        "sceAgcDcbPushMarker" => 6,
        "sceAgcDcbResetQueue" => 6,
        "sceAgcDcbSetBaseIndirectArgs" => 6,
        "sceAgcDcbSetCxRegistersIndirect" => 6,
        "sceAgcDcbSetFlip" => 6,
        "sceAgcDcbSetIndexBuffer" => 6,
        "sceAgcDcbSetIndexCount" => 6,
        "sceAgcDcbSetIndexSize" => 6,
        "sceAgcDcbSetNumInstances" => 6,
        "sceAgcDcbSetShRegistersIndirect" => 6,
        "sceAgcDcbSetUcRegistersIndirect" => 6,
        "sceAgcDcbStallCommandBufferParser" => 6,
        "sceAgcDcbWaitRegMem" => 6,
        "sceAgcDcbWaitUntilSafeForRendering" => 6,
        "sceAgcDcbWriteData" => 6,
        "sceAgcDmaDataPatchSetDstAddressOrOffset" => 6,
        "sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate" => 6,
        "sceAgcGetRegisterDefaults2" => 6,
        "sceAgcGetRegisterDefaults2Internal" => 6,
        "sceAgcQueueEndOfPipeActionPatchAddress" => 6,
        "sceAgcSetCxRegIndirectPatchAddRegisters" => 6,
        "sceAgcSetCxRegIndirectPatchSetAddress" => 6,
        "sceAgcSetShRegIndirectPatchAddRegisters" => 6,
        "sceAgcSetShRegIndirectPatchSetAddress" => 6,
        "sceAgcSetUcRegIndirectPatchAddRegisters" => 6,
        "sceAgcSetUcRegIndirectPatchSetAddress" => 6,
        "sceAgcSuspendPoint" => 6,
        "sceAgcWaitRegMemPatchAddress" => 6,
    }
}

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// The `libSceAgc` error family's "bad argument" answer, from the guest's own validating wrapper
/// which null-checks rdi/rsi/rdx and answers this for any of them (D556). Returned when a required
/// pointer argument is null, rather than writing through it.
const BAD_ARGUMENT: u64 = 0x8a6c_000a;

/// Writes a little-endian quadword into guest memory at `at`, under the identity mapping (D014).
///
/// # Safety
///
/// `at` must address eight bytes of guest-owned, writable memory. The AGC out-parameters are
/// buffers the guest supplies and the call is contracted to fill, so the guest owns them by
/// construction.
unsafe fn poke_u64(at: u64, value: u64) {
    let Ok(at) = usize::try_from(at) else {
        return;
    };
    // SAFETY: the caller guarantees `at` addresses eight writable guest bytes; unaligned because
    // these object fields carry no alignment promise.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), value);
    }
}

/// Writes a little-endian dword into guest memory at `at`, under the identity mapping (D014).
///
/// # Safety
///
/// `at` must address four bytes of guest-owned, writable memory - see [`poke_u64`].
unsafe fn poke_u32(at: u64, value: u32) {
    let Ok(at) = usize::try_from(at) else {
        return;
    };
    // SAFETY: the caller guarantees `at` addresses four writable guest bytes; unaligned for the
    // same reason as the quadword write.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u32>(at), value);
    }
}

/// `sceAgcCreateShader(out, header, bytecode, flags)`.
///
/// **The object model is measured, not invented (obSCEne `166-agc/create-shader`, answering
/// REQ-3c5e; sweeps `20260911-002219`/`010855`).** The call writes the *header's own address* into
/// `*out` - the shader object **is** the guest-supplied header region (`shader-obj-ptr == arg1`,
/// distance zero: guest-adjacent, no allocation) - then, within that object, sets `+0x10 = bytecode`
/// (a pointer to arg2, distance zero from it), `+0x30 = 0`, and `+0x50 = 0`, and returns `0`.
///
/// Those three are the only fields the guest is observed to read at the D556 fault site
/// (`8b 46 50` reads `+0x50`, `48 8b 46 30` reads `+0x30`), and `+0x10` is the bytecode pointer the
/// object carries. The object spans `0x130` bytes and hardware changed `0x2c` of them; the bytes
/// beyond these three stay as the guest prepared them, because nothing measured says what they hold
/// - writing an invented value there is exactly what principle 3 forbids. A null `out` or `header`
/// gets the wrapper's own `0x8a6c000a` rather than a fault.
fn create_shader(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, header, bytecode) = (args[0], args[1], args[2]);
    if out == 0 || header == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `out` is the guest stack slot the call fills with the object pointer (D556).
    unsafe { poke_u64(out, header) };
    // SAFETY: `header` is the guest-owned object region (>= 0x130 bytes); `+0x10` is the bytecode
    // pointer the object carries (measured, 3c5e).
    unsafe { poke_u64(header.wrapping_add(0x10), bytecode) };
    // SAFETY: same region; `+0x30` is the measured quadword the guest reads (0).
    unsafe { poke_u64(header.wrapping_add(0x30), 0) };
    // SAFETY: same region; `+0x50` is the measured dword the guest reads the low byte of (0).
    unsafe { poke_u32(header.wrapping_add(0x50), 0) };
    OK
}

/// Implementations this crate provides for `libSceAgc`.
///
/// Only `sceAgcCreateShader` so far - the one call whose object model obSCEne measured end to end
/// (3c5e). The command **builders** (`sceAgcDcb*`) stay declared-only until a capture grounds each
/// one's encoding, and the shader-linkage calls (`sceAgcCreateInterpolantMapping`,
/// `sceAgcLinkShaders`, the prim-state pair) are implemented as the guest reaches them, so each is
/// built against its observed call rather than blind (e4f1 gives their behaviours to build to).
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceAgcCreateShader", create_shader)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The measured object model is what the handler writes.** A guest hands `out` (a slot) and
    /// `header` (its object buffer); after the call, `*out` is the header address and the object's
    /// three measured fields are set, so a guest reading `+0x30`/`+0x50` sees the zeros hardware
    /// wrote rather than the garbage an unwritten out-parameter left (worklog 503/D556).
    #[test]
    fn create_shader_fills_the_object_as_hardware_did() {
        // A slot for the object pointer, and a 0x130-byte object buffer pre-poisoned with 0xff.
        let mut slot: u64 = 0;
        let mut object = [0xffu8; 0x130];
        let out = std::ptr::addr_of_mut!(slot) as u64;
        let header = object.as_mut_ptr() as u64;
        let bytecode = 0x4000_0000_1234u64;

        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = out;
        args[1] = header;
        args[2] = bytecode;

        assert_eq!(create_shader(&args), OK, "a well-formed call succeeds");
        assert_eq!(slot, header, "*out is the header address - the object is the header region");

        let read_u64 = |off: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&object[off..off + 8]);
            u64::from_le_bytes(b)
        };
        assert_eq!(read_u64(0x10), bytecode, "+0x10 is the bytecode pointer");
        assert_eq!(read_u64(0x30), 0, "+0x30 is the measured zero");
        let mut d = [0u8; 4];
        d.copy_from_slice(&object[0x50..0x54]);
        assert_eq!(u32::from_le_bytes(d), 0, "+0x50 is the measured zero dword");
    }

    /// **A null out-parameter is refused, not dereferenced.** The guest's own wrapper answers
    /// `0x8a6c000a` for a null pointer (D556); reproducing that is safer than writing through null.
    #[test]
    fn create_shader_refuses_a_null_out_parameter() {
        let args = [0u64; GUEST_ARG_REGISTERS];
        assert_eq!(create_shader(&args), BAD_ARGUMENT, "null out is the wrapper's bad-argument code");
    }
}
