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

/// Reads a little-endian dword from guest memory at `at`, under the identity mapping (D014).
///
/// # Safety
///
/// `at` must address four bytes of readable guest memory.
unsafe fn peek_u32(at: u64) -> u32 {
    let Ok(at) = usize::try_from(at) else {
        return 0;
    };
    // SAFETY: the caller guarantees four readable guest bytes at `at`; unaligned for the field's sake.
    unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u32>(at)) }
}

/// The PS-input interpolant table is thirty-two quadwords.
const INTERPOLANT_ENTRIES: u64 = 32;

/// Writes the default PS-input interpolant table at `at`: entry `i` is `(i << 32) | (0x191 + i)`.
///
/// **Measured (obSCEne `166-agc/link-shaders`, sweep `20260912-003916`).** It is both
/// `sceAgcCreateInterpolantMapping`'s default output and the interpolant half of
/// `sceAgcLinkShaders`' link state - the first two entries came back `0x191` and `0x1_0000_0192`,
/// which is exactly this rule. `0x191` is `SPI_PS_INPUT_CNTL_0`, so the default routes input `i`
/// from attribute slot `i`.
///
/// # Safety
///
/// `at` must address `INTERPOLANT_ENTRIES * 8` (256) bytes of guest-owned, writable memory.
unsafe fn write_default_interpolants(at: u64) {
    for i in 0..INTERPOLANT_ENTRIES {
        let entry = (i << 32) | (0x191 + i);
        // SAFETY: the caller guarantees 256 writable bytes at `at`; `i * 8 < 256` by construction.
        unsafe { poke_u64(at.wrapping_add(i * 8), entry) };
    }
}

/// Writes the low five bits of the dword at `sec_state + 0x14` to `topology`, preserving the rest.
///
/// **Measured (obSCEne `166-agc/update-prim-state`).** The probe reads
/// `*(uint32_t *)(sec_state + 0x14) & 0x1f` back after both create and update, so the primitive
/// topology lives in those five bits. Read-modify-write rather than a bare store, because the guest
/// owns the other twenty-seven bits.
///
/// # Safety
///
/// `sec_state` must address a readable, writable guest buffer of at least `0x18` bytes.
unsafe fn set_topology(sec_state: u64, topology: u32) {
    let field = sec_state.wrapping_add(0x14);
    // SAFETY: the caller guarantees `sec_state + 0x14` is a readable guest dword.
    let prev = unsafe { peek_u32(field) };
    // SAFETY: same field, writable by the same guarantee.
    unsafe { poke_u32(field, (prev & !0x1f) | (topology & 0x1f)) };
}

/// `sceAgcCreateInterpolantMapping(mapping, vs, ps)`.
///
/// Fills `mapping` (arg0) with the 32-quadword PS-input table and returns `0`. With `vs`/`ps` both
/// null the table is the default `(i << 32) | (0x191 + i)` (measured). obSCEne's non-null case
/// "maps VS exports (`vs+0x38`) to PS inputs (`ps+0x30`)", but the *only* table hardware was
/// measured writing is this default (the link-shaders capture), so the vs/ps-specific remap is not
/// modelled - writing the default there rather than an invented remap keeps to principle 3.
fn create_interpolant_mapping(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mapping = args[0];
    if mapping == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `mapping` is the guest-owned 256-byte out-buffer the call fills (obSCEne agc.c).
    unsafe { write_default_interpolants(mapping) };
    OK
}

/// `sceAgcUpdateInterpolantMapping(mapping, vs, ps)`.
///
/// Rewrites the active mapping table in place and returns `0` (obSCEne `166-agc/update-interpolant`).
/// Modelled as the same default fill: an update with no measured remap re-establishes the default
/// table rather than inventing a change.
fn update_interpolant_mapping(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mapping = args[0];
    if mapping == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `mapping` is the guest-owned 256-byte table being updated in place.
    unsafe { write_default_interpolants(mapping) };
    OK
}

/// `sceAgcCreatePrimState(prim_state, sec_state, null, vs, topology)`.
///
/// Records the primitive topology (arg4) in the low five bits of `sec_state + 0x14` and returns `0`
/// (obSCEne `166-agc/update-prim-state`, which reads it back there). The `prim_state` buffer's own
/// contents are not written: nothing measured says what the routing block holds, so it is left as
/// the guest prepared it rather than filled with an invented layout.
fn create_prim_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (prim_state, sec_state, topology) = (args[0], args[1], args[4] as u32);
    if prim_state == 0 || sec_state == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `sec_state` is the guest-owned 64-byte secondary-state buffer; `+0x14` is a dword.
    unsafe { set_topology(sec_state, topology) };
    OK
}

/// `sceAgcUpdatePrimState(prim_state, sec_state, topology)`.
///
/// Updates the topology (arg2) in the low five bits of `sec_state + 0x14` and returns `0` - the
/// measured behaviour (`topo-orig` 4 becomes `topo-updated` 1 in `166-agc/update-prim-state`). The
/// routing word at `prim_state + 0xc` that obSCEne notes is also touched is not written, its value
/// being unmeasured.
fn update_prim_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (prim_state, sec_state, topology) = (args[0], args[1], args[2] as u32);
    if prim_state == 0 || sec_state == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `sec_state` is the guest-owned buffer; `+0x14` is a readable, writable dword.
    unsafe { set_topology(sec_state, topology) };
    OK
}

/// The stage-routing quadword `sceAgcLinkShaders` writes at `link_state + 0x108`.
///
/// Measured `0x0000_0002_0000_029b` (obSCEne `166-agc/link-shaders`, and stated in e4f1).
const LINK_STAGE_ROUTING: u64 = 0x0000_0002_0000_029b;

/// `sceAgcLinkShaders(link_state, sec_state, null, vs, ps, ...)`.
///
/// Fills `link_state` (arg0): the 256-byte interpolant table at `+0x0` (the same default the mapping
/// calls write) and the stage-routing quadword `0x0000_0002_0000_029b` at `+0x108`, then returns
/// `0`. **Measured** in `166-agc/link-shaders` (sweep `20260912-003916`): `link-state-interp`
/// began `0x191`, `0x1_0000_0192`, and `link-state-routing` carried `0x0000_0002_0000_029b` eight
/// bytes past the interpolants. The rest of the 32-byte routing block is unmeasured and left as the
/// guest prepared it.
fn link_shaders(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let link_state = args[0];
    if link_state == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `link_state` is the guest-owned out-buffer (>= 0x110 bytes) the call fills.
    unsafe { write_default_interpolants(link_state) };
    // SAFETY: same buffer; `+0x108` is the measured routing quadword's slot.
    unsafe { poke_u64(link_state.wrapping_add(0x108), LINK_STAGE_ROUTING) };
    OK
}

/// Implementations this crate provides for `libSceAgc`.
///
/// `sceAgcCreateShader` (object model, 3c5e) and the five shader-linkage calls e4f1 asked for, whose
/// behaviours obSCEne measured once the 3D-draw sweeps settled (9a41, sweep `20260912-003916`): the
/// interpolant mapping pair, the primitive-state pair, and the shader linker. The command
/// **builders** (`sceAgcDcb*`) stay declared-only until a capture grounds each one's encoding.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAgcCreateShader", create_shader),
        ("sceAgcCreateInterpolantMapping", create_interpolant_mapping),
        ("sceAgcUpdateInterpolantMapping", update_interpolant_mapping),
        ("sceAgcCreatePrimState", create_prim_state),
        ("sceAgcUpdatePrimState", update_prim_state),
        ("sceAgcLinkShaders", link_shaders),
    ]
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

    fn read_le_u64(buf: &[u8], off: usize) -> u64 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[off..off + 8]);
        u64::from_le_bytes(b)
    }

    /// **The interpolant table is the measured `(i << 32) | (0x191 + i)`.** The first two entries
    /// are what obSCEne read back from hardware (`0x191`, `0x1_0000_0192`), so a guest reading the
    /// mapping sees the same routing the console wrote.
    #[test]
    fn interpolant_mapping_writes_the_measured_default_table() {
        let mut table = [0xffu8; 256];
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = table.as_mut_ptr() as u64;

        assert_eq!(create_interpolant_mapping(&args), OK);
        assert_eq!(read_le_u64(&table, 0), 0x191, "entry 0 is SPI_PS_INPUT_CNTL_0");
        assert_eq!(read_le_u64(&table, 8), 0x1_0000_0192, "entry 1 is (1<<32)|0x192");
        assert_eq!(read_le_u64(&table, 31 * 8), (31u64 << 32) | (0x191 + 31), "entry 31");
    }

    /// **LinkShaders writes the interpolant table plus the routing quadword at +0x108.** Both halves
    /// are measured (`166-agc/link-shaders`): the interpolants at the front, `0x0000_0002_0000_029b`
    /// eight bytes past them.
    #[test]
    fn link_shaders_writes_interpolants_and_routing() {
        let mut link = [0u8; 0x120];
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = link.as_mut_ptr() as u64;

        assert_eq!(link_shaders(&args), OK);
        assert_eq!(read_le_u64(&link, 0), 0x191, "interpolant table at the front");
        assert_eq!(read_le_u64(&link, 8), 0x1_0000_0192, "second interpolant");
        assert_eq!(read_le_u64(&link, 0x108), LINK_STAGE_ROUTING, "stage routing at +0x108");
    }

    /// **Primitive topology lands in the low five bits of `sec_state + 0x14`, create then update.**
    /// A create with `DI_PT_TRILIST` (4) then an update to `DI_PT_POINTLIST` (1) reproduces the
    /// measured `topo-orig` 4 -> `topo-updated` 1, and the surrounding bits are preserved.
    #[test]
    fn prim_state_records_topology_in_sec_state() {
        let mut prim = [0u8; 64];
        let mut sec = [0u8; 64];
        // Poison the high bits of the topology dword to prove read-modify-write preserves them.
        sec[0x14..0x18].copy_from_slice(&0xabcd_ffe0u32.to_le_bytes());

        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = prim.as_mut_ptr() as u64;
        args[1] = sec.as_mut_ptr() as u64;
        args[4] = 4; // DI_PT_TRILIST

        assert_eq!(create_prim_state(&args), OK);
        let after_create = u32::from_le_bytes(sec[0x14..0x18].try_into().unwrap());
        assert_eq!(after_create & 0x1f, 4, "topology is DI_PT_TRILIST");
        assert_eq!(after_create & !0x1f, 0xabcd_ffe0, "the other bits are preserved");

        args[2] = 1; // DI_PT_POINTLIST, arg2 for the update
        assert_eq!(update_prim_state(&args), OK);
        let after_update = u32::from_le_bytes(sec[0x14..0x18].try_into().unwrap());
        assert_eq!(after_update & 0x1f, 1, "topology updated to DI_PT_POINTLIST");
        assert_eq!(after_update & !0x1f, 0xabcd_ffe0, "the other bits still preserved");
    }
}
