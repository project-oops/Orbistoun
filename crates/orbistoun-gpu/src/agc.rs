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
//! **Real import tables, and a few names a measurement added.** Most names below are read
//! from a module's own import table: PPSA02664 imports fifty-one from `libSceAgc` and five
//! from `libSceAgcDriver`, and other modules of the corpus name five more `libSceAgcDriver`
//! functions. The rest are the shader-linkage builders - `sceAgcCreateShader` and the
//! interpolant/prim-state set - added because obSCEne's `-e4f1`/`-9a41` requests measured them
//! on hardware, not because they appear in that import table. So none is derived from a
//! pattern and hoped to match: each is either imported by a real guest or measured on a
//! console, the strongest provenance available here - the same `orbistoun-input` documents
//! for `libScePad`.
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
//! **Most of what is declared here is implemented; the rest are declarations** - the split is
//! asserted in `tests/dcb_wiring.rs`, so the number lives in one place a prose comment cannot drift
//! from. What the declarations buy is that a guest reaching the graphics interface is **named and
//! counted** instead of vanishing into `unknown::`, and that the loud stub policy answers it
//! rather than a placeholder a caller might read as a handle (D125).
//!
//! Ten are neither encoders nor skeletons but **measured returns**: the whole `sceAgc*Patch*` family
//! answers the `0x0` obSCEne measured for each (`166-agc/patch-*`, REQ-...4386 and ...3d1e) rather
//! than the placeholder the guest was carrying into a `memcpy`. They amend a packet in place, which
//! is a GPU-submission detail the CPU flow does not read, so they share one handler that returns the
//! measured success and writes nothing.
//!
//! Most are fully-measured encoders: each writes a packet whose bytes obSCEne measured on
//! hardware, wired only once those bytes were known across more than one input. `sceAgcDcbDrawIndex`
//! and `sceAgcDcbSetIndexSize`, which worklog 536 refused on a single input each, were later measured
//! across more inputs and wired - the two draw-index body dwords placed, the index-size mapping taken
//! from eight argument pairs. `sceAgcCbNop` is the simplest of them - a header-only packet that takes
//! no arguments, measured whole.
//!
//! The rest are **reservation skeletons** rather than full encoders: `sceAgcDcbSetCxRegistersIndirect`
//! (the producer the patch family fills), the Acquire/Release/DmaData/SetBase set, and the
//! REQ-...a70f cluster wired from sweep `20260915-203058` (`CbDispatch`, the dispatch- and
//! draw-indirect builders, `SetShRegistersIndirect`/`SetUcRegistersIndirect`,
//! `StallCommandBufferParser`, and the Acb twins). Each reserves the measured extent and writes only
//! the measured header - rebuilt from the opcode through [`packet::build::command_header`] - leaving
//! the body zero. They earn their place not by encoding a packet but by handing the guest a real
//! cursor where an unwired builder handed it a placeholder to `memcpy` through (D696, worklog 553,
//! 600, 618) - the exact bodies wait on more argument passes.
//!
//! **This paragraph used to say "nothing here is implemented".** It was wrong for long
//! enough that a gap analysis read it at its word and reported the surface as emptier than
//! it is. A document claiming *less* than the code does is the mirror of the failure
//! principle 3 names, and it misleads in the same direction: by being believed.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAgc" {
        "0x7d86501b8094ef57" => 1,
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
        // The shader-linkage set e4f1 asked for and 9a41 measured, implemented from those
        // behaviours below. Arity 6 is the trampoline's full capture, not a claim - the real
        // arities (mapping/vs/ps, link/sec/null/vs/ps) are recorded in the knowledge file's prose
        // rather than as a number that would disagree with this one.
        "sceAgcCreateInterpolantMapping" => 6,
        "sceAgcUpdateInterpolantMapping" => 6,
        "sceAgcUpdatePrimState" => 6,
        "sceAgcLinkShaders" => 6,
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
        "sceAgcDcbSetCxRegisterDirect" => 6,
        "sceAgcDcbSetCxRegistersIndirect" => 6,
        "sceAgcDcbSetFlip" => 6,
        "sceAgcDcbSetIndexBuffer" => 6,
        "sceAgcDcbSetIndexCount" => 6,
        "sceAgcDcbSetIndexSize" => 6,
        "sceAgcDcbSetNumInstances" => 6,
        "sceAgcDcbSetShRegistersIndirect" => 6,
        "sceAgcDcbSetUcRegisterDirect" => 6,
        "sceAgcDcbSetUcRegistersIndirect" => 6,
        "sceAgcDcbStallCommandBufferParser" => 6,
        "sceAgcDcbWaitRegMem" => 6,
        "sceAgcDcbWaitUntilSafeForRendering" => 6,
        "sceAgcDcbWriteData" => 6,
        "sceAgcDmaDataPatchSetDstAddressOrOffset" => 6,
        "sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate" => 6,
        "sceAgcGetIsTrinityMode" => 0,
        "sceAgcGetRegisterDefaults2" => 6,
        "sceAgcGetRegisterDefaults2Internal" => 6,
        "0x53bbd82b51d172db" => 2,
        "sceAgcInit" => 2,
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

use crate::packet;
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

/// Reads a little-endian quadword from guest memory at `at`, under the identity mapping (D014).
///
/// # Safety
///
/// `at` must address eight bytes of readable guest memory.
unsafe fn peek_u64(at: u64) -> u64 {
    let Ok(at) = usize::try_from(at) else {
        return 0;
    };
    // SAFETY: the caller guarantees eight readable guest bytes at `at`; unaligned because these
    // object fields carry no alignment promise - the same contract `peek_u32` states below.
    unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at)) }
}

/// `sceAgcCreateShader(out, header, bytecode, flags)`.
///
/// **The object model is measured, not invented (obSCEne `166-agc/create-shader`, answering
/// REQ-3c5e; sweeps `20260911-002219`/`010855`).** The call writes the *header's own address* into
/// `*out` - the shader object **is** the guest-supplied header region (`shader-obj-ptr == arg1`,
/// distance zero: guest-adjacent, no allocation) - then, within that object, sets `+0x10 = bytecode`
/// (a pointer to arg2, distance zero from it), `+0x30 = 0`, and `+0x50 = 0`, and returns `0`.
///
/// It also converts the relative sub-object offset at `+0x8` (e.g. `0xd8`, observed in retail shader
/// headers and measured in obSCEne `166-agc/create-shader` where `+0x8` becomes `header + 0xe0`/`0xd8`)
/// into an absolute pointer within the header object, so render-state marshalling routines can
/// dereference `[rsi + 0x8]->+0x28`.
fn create_shader(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, header, bytecode) = (args[0], args[1], args[2]);
    if out == 0 || header == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `out` is the guest stack slot the call fills with the object pointer (D556).
    unsafe { poke_u64(out, header) };
    // SAFETY: `header` is the guest-owned object region (>= 0x130 bytes).
    // obSCEne measured that hardware changes exactly 0x2c (44) bytes within the header
    // (obSCEne 166-agc/create-shader):
    // - +0x08: pointer to sub-table at header + off_8 + 8 (measured 0x7eeffb5a0 with off_8=0xd8)
    // - +0x10: bytecode pointer (arg2)
    // - +0x18..+0x38: the group-pointer array; each slot holding a relative offset is relocated to a
    //   header pointer (measured on the middle three, generalised to the array under the guard - see
    //   the loop below and worklog 748)
    // - Sub-table entries at sub_table[0..5]: relative offsets relocated to header pointers
    // Note: +0x50 and other fields are asset metadata (e.g. counts) and are not touched.
    unsafe { poke_u64(header.wrapping_add(0x10), bytecode) };

    // SAFETY: `header` is the guest-owned object region, at least 0x130 bytes (the extent obSCEne
    // measured), so the quadword at +0x8 is inside it.
    let off_8 = unsafe { peek_u64(header.wrapping_add(0x8)) };
    if off_8 != 0 && off_8 < 0x1000 {
        let sub_table = header.wrapping_add(off_8).wrapping_add(8);
        // SAFETY: the same +0x8 quadword just read, written back as an absolute pointer.
        unsafe { poke_u64(header.wrapping_add(0x8), sub_table) };
        for i in 0..5 {
            let entry_addr = sub_table.wrapping_add(i * 8);
            // SAFETY: `sub_table` is inside the guest's own header object - it is that object's
            // own relative offset, rejected above unless it is non-zero and under 0x1000 - and
            // `i * 8 < 40`, so the entry is within the region hardware itself dereferences here
            // (measured, obSCEne 166-agc/create-shader). Unaligned for the field's sake.
            let rel = unsafe { peek_u64(entry_addr) };
            if rel != 0 && rel < 0x1000 {
                // **Self-relative, like every offset in this object** (worklog 872): the entry's
                // own address plus its offset. Relative to the table base instead, PPSA25872's
                // slot tables landed eight bytes short on `0xffff` padding and its constant upload
                // wrote through a null extended buffer.
                // SAFETY: the same entry just read, written back as an absolute pointer.
                unsafe { poke_u64(entry_addr, entry_addr.wrapping_add(rel)) };
            }
        }
    }

    // The five group-pointer slots at +0x18..+0x38 (the array the guest walks with stride 8,
    // worklog 748). The measured shader (obSCEne 166-agc/create-shader) populated only the middle
    // three, so the list was those; but a field that holds a relative offset *must* be relocated to
    // become a valid pointer, and the guard below relocates exactly those and skips the rest. So
    // scanning the whole array preserves the measured three-of-five behaviour (the endpoints were 0
    // there, and 0 is skipped) and also handles headers that populate the endpoints - PPSA02664's,
    // whose +0x18 holds a raw 0xa8 the walk dies on when it is left un-relocated (worklog 748). Not
    // a value guessed: an offset in a pointer slot is a pointer-in-waiting, relocated or wrong.
    for &offset in &[0x18, 0x20, 0x28, 0x30, 0x38] {
        let field = header.wrapping_add(offset);
        // SAFETY: `offset` is one of 0x18..0x38, inside the guest-owned header object whose measured
        // extent is 0x130 bytes.
        let rel = unsafe { peek_u64(field) };
        if rel != 0 && rel < 0x1000 {
            // **Self-relative: the field's own address plus its offset.** Measured - the probe's
            // header held `0x70` at `+0x20` and `0x38` at `+0x28`, and the console wrote back
            // `header + 0x90` and `header + 0x60` (worklog 872). Relative to the header base, as
            // this was, both land 0x20 and 0x28 bytes short.
            // SAFETY: the same field just read, written back as an absolute pointer.
            unsafe { poke_u64(field, field.wrapping_add(rel)) };
        }
    }
    // SAFETY: `+0x20` is inside the guest-owned header object (extent 0x130), read after the loop
    // above relocated it.
    let registers = unsafe { peek_u64(header.wrapping_add(0x20)) };
    // SAFETY: `registers` points into the same guest-owned header object - the relocation above
    // made it a pointer into it - and `patch_program_address` writes only its first four dwords.
    unsafe { patch_program_address(registers, bytecode) };
    OK
}

/// Writes the program's address into the shader's register descriptor, as the console does.
///
/// **Measured (obSCEne REQ-20260925T2056Z-5d19).** The descriptor `+0x20` points at is
/// `{reg, value, reg + 1, value}`, the program-address register pair. The console wrote
/// `payload >> 8` into its second dword - the two bytes a relocation model could not account for -
/// and wrote nothing when the first dword named no register (stages 4 and 5). The high half,
/// `payload >> 40` into the fourth dword, is the same pair's other register: Mesa programs it as
/// `address32_hi >> 8` (`ac_cmdbuf.c:318-324`), and it read zero on the probe only because its
/// payload sat below 2^40 (worklog 874).
///
/// # Safety
///
/// `registers`, when non-zero, must address sixteen writable bytes of guest memory.
unsafe fn patch_program_address(registers: u64, payload: u64) {
    if registers == 0 {
        return;
    }
    // SAFETY: the caller guarantees sixteen readable bytes at `registers`.
    if unsafe { peek_u32(registers) } == 0 {
        return;
    }
    // SAFETY: as above, writable; the second and fourth dwords of the descriptor.
    unsafe {
        poke_u32(registers.wrapping_add(4), (payload >> 8) as u32);
    }
    // SAFETY: as above.
    unsafe {
        poke_u32(registers.wrapping_add(12), (payload >> 40) as u32);
    }
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

/// Field offsets within the command-buffer writer handle every `sceAgcDcb*`/`sceAgcCb*` builder
/// takes in `arg0`.
///
/// **Guest-observed, then confirmed by the library's own behaviour on hardware.** The shape was
/// read off PPSA02664's stack. obSCEne then built a handle to that shape and called the real
/// builders through it: they wrote correct packets and advanced the cursor by exactly what each
/// builder's own `GetSize` had answered, across sixteen builders (worklog 534). Two offsets are
/// confirmed harder still - poisoning `+0x20` with `0xCC` makes the library `call *0x20(%rdi)` and
/// fault at `0xCCCCCCCCCCCCCCCC`, which is direct evidence it reads a function pointer there, and
/// the same poison at `+0x30` underflows its space check first.
///
/// Not every field is used here. `+0x00` is the buffer start, `+0x08` one past its end, `+0x20` the
/// overflow callback and `+0x30` a reserved-dword counter. This writes through the cursor and
/// respects the limit; it does not touch the rest.
mod dcb {
    /// The write cursor - the field a builder advances, and the one that made the layout
    /// checkable: `cur - begin` after a call is exactly the builder's own `GetSize` answer.
    pub(super) const CUR: u64 = 0x10;
    /// The limit a builder checks a packet against before writing it.
    pub(super) const LIMIT: u64 = 0x18;
}

// **A builder returns the address of the packet it just wrote.**
//
// obSCEne reports `0x200060078` from every builder, in every run, and worklog 538 first recorded
// that as an opaque constant on the strength of its stability. It is not one. Its probe allocates
// with `oops_mem_alloc`, and the arithmetic closes exactly: base `0x200060000` aligns to
// `0x200060040`, the eight-byte count prefix puts the struct at `0x200060038`, and its command
// buffer sits `0x40` further on - at `0x200060078`. Other allocations in the same sweep
// (`0x200080000`, `0x200028000`) are in the same region. The value is a pointer into the probe's own
// command buffer, constant only because that allocator is deterministic and every check resets the
// writer before calling.
//
// **Which pointer is derived, not measured.** Every obSCEne check resets first, so `cur == begin` at
// the call and "the packet's address" and "the buffer's start" fit the data equally. The packet's
// address is taken here because it is the reading that makes the guest work: PPSA02664 passes a
// builder's return straight into `sceAgcSetCxRegIndirectPatchAddRegisters`, and a family of
// `sceAgc*Patch*` entry points exists to amend an already-written packet. Returning the buffer start
// would let only the first packet in a buffer ever be patched. Settling it needs two builders called
// without a reset between them, which is asked of obSCEne rather than guessed at (REQ-...c74f).

/// Appends one packet to the writer handle at `dcb`, advancing its cursor by the packet's length.
///
/// This is the effectful half that [`crate::packet::build`] deliberately does not have: the
/// encoders there are pure and fully measured, and this places what they produce. Splitting it that
/// way is why the encoders could land before the handle layout did.
///
/// Refuses rather than overruns. When a packet will not fit, the real library calls the overflow
/// callback at `+0x20` and grows the buffer; **this does not**, because calling a guest callback
/// from a shim is a mechanism nothing has measured. It answers the loud placeholder instead, which
/// is visible in a trace and cannot be mistaken for a firmware code (principle 3, D670).
fn dcb_append(dcb: u64, words: &[u32]) -> u64 {
    if dcb == 0 || words.is_empty() {
        return BAD_ARGUMENT;
    }
    // SAFETY: `dcb` is the guest-owned writer handle the guest passed in arg0; the cursor and the
    // limit are quadwords at its `+0x10` and `+0x18`, the offsets hardware itself reads.
    let cur = unsafe { peek_u64(dcb.wrapping_add(dcb::CUR)) };
    // SAFETY: the same handle, the adjacent field.
    let limit = unsafe { peek_u64(dcb.wrapping_add(dcb::LIMIT)) };
    let length = words.len() as u64 * 4;
    if cur == 0 || limit == 0 || cur.wrapping_add(length) > limit {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    }
    for (i, word) in words.iter().enumerate() {
        // SAFETY: every offset written is below `length`, and `cur + length <= limit` was checked
        // above, so each dword lands inside the guest's own command buffer.
        unsafe { poke_u32(cur.wrapping_add(i as u64 * 4), *word) };
    }
    // SAFETY: the cursor field of the same handle, advanced by what was just written.
    unsafe { poke_u64(dcb.wrapping_add(dcb::CUR), cur.wrapping_add(length)) };
    cur
}

/// `sceAgcDcbEventWrite(dcb, event_type, _)`. Measured: `166-agc/dcb-event-write`.
fn dcb_event_write(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::event_write(args[1] as u32))
}

/// `sceAgcDcbSetIndexCount(dcb, indices)`. Measured: `166-agc/dcb-set-index-count`.
fn dcb_set_index_count(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::set_index_count(args[1] as u32))
}

/// `sceAgcDcbSetNumInstances(dcb, instances)`. Measured: `166-agc/dcb-set-num-instances`.
fn dcb_set_num_instances(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::set_num_instances(args[1] as u32))
}

/// `sceAgcDcbDrawIndexAuto(dcb, index_count, initiator)`. Measured: `166-agc/dcb-draw-auto`.
fn dcb_draw_index_auto(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::draw_index_auto(args[1] as u32, args[2] as u32),
    )
}

/// `sceAgcDcbSetIndexBuffer(dcb, address)`. Measured: `166-agc/dcb-set-index-buffer`.
fn dcb_set_index_buffer(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::set_index_base(args[1]))
}

/// The register offset and value a `*RegisterDirect` builder takes packed into one quadword.
///
/// **Measured** (`166-agc/dcb-set-cx-reg`, `dcb-set-uc-reg`): obSCEne passed
/// `(value << 32) | offset` and the packet came back carrying the offset then the value, both
/// unchanged.
const fn unpack_register_entry(entry: u64) -> (u16, u32) {
    (entry as u16, (entry >> 32) as u32)
}

/// `sceAgcDcbSetCxRegisterDirect(dcb, (value << 32) | offset)`. Measured: `166-agc/dcb-set-cx-reg`.
fn dcb_set_cx_register_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, value) = unpack_register_entry(args[1]);
    dcb_append(args[0], &packet::build::set_context_register(offset, value))
}

/// `sceAgcDcbSetUcRegisterDirect(dcb, (value << 32) | offset)`. Measured: `166-agc/dcb-set-uc-reg`.
fn dcb_set_uc_register_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, value) = unpack_register_entry(args[1]);
    dcb_append(args[0], &packet::build::set_uconfig_register(offset, value))
}

/// `sceAgcDcbSetCxRegistersIndirect(dcb, ...)` - reserve the packet a title patches, and hand back
/// a real address for it.
///
/// # Why a skeleton rather than a full encoder
///
/// This is the producer half of the patch family that walls PPSA02664 and PPSA03416. Unwired, it
/// answered the loud placeholder, the guest carried that placeholder as the packet's address into a
/// `memcpy`, and the run died in `VCRUNTIME140.dll` (worklog 553, 594). The one thing it must do to
/// clear that is what every other builder here does: append a correctly sized packet and return the
/// **real cursor**, so the guest's `memcpy` and the patch that follows it land in command-buffer
/// memory rather than on `0xf7ff0001`.
///
/// The packet body is not encoded from the arguments, and deliberately: `REQ-...4386` measured one
/// producer call, which fixes the header and the 20-byte extent but not which argument becomes which
/// body dword. The guest fills the body itself through `sceAgcSetCxRegIndirectPatchAddRegisters`, so
/// the reservation is the part that has to be right and is the part that is measured.
fn dcb_set_cx_registers_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_cx_registers_indirect_skeleton(),
    )
}

/// The `sceAgc*Patch*` family - amend an already-written packet in place, and **return the measured
/// `0x0`**.
///
/// # One handler for ten entry points, because the measurement is one answer
///
/// obSCEne measured every patch in the family returning `0x0`, each across two argument passes
/// (`166-agc/patch-*`, sweep `20260915-203058`, REQ-...3d1e; and `patch-cx-registers-indirect` in
/// three earlier sweeps, REQ-...4386): the Cx/Sh/Uc register patches (`AddRegisters` and
/// `SetAddress`), the two DmaData address patches, and the wait-reg-mem and end-of-pipe address
/// patches. So they share one handler that answers that `0x0`.
///
/// # Why a return and not an encoder
///
/// It writes nothing to the packet. Each patch amends a specific field in place - a `SetAddress`
/// writes an address, an `AddRegisters` extends the register run - but that is a GPU-submission
/// detail the CPU-side flow does not read, and where the guest cares about the bytes it writes them
/// itself (the Cx case: one eight-byte entry per call, worklog 614). What walled the guest was the
/// *return*: unimplemented, each answered the placeholder, and the guest carried it as a pointer into
/// a `memcpy` and faulted in host code (worklog 614). Answering the measured `0x0` is what clears
/// that, and closing the whole family at once is what stops the wall moving one patch downstream each
/// time (the exact shape worklog 614 hit after the Cx producer skeleton). The `packet` argument is
/// not dereferenced, so a null needs no guard.
fn agc_patch_returns_ok(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceAgcDcbWaitUntilSafeForRendering(dcb, ...)` - a measured library-level no-op.
///
/// obSCEne re-probed it (REQ-...4e91, sweeps `20260917-090300`/`101310`): 0 bytes and `rc 0x0` under
/// every condition (bare writer and one prepared through `sceAgcDcbResetQueue`), its `GetSize` symbol
/// absent from `libSceAgc`, `empty-encoding` true. So it emits nothing and answers the measured `0x0`.
/// It writes nothing and dereferences no argument, so a null handle needs no guard - the same terms as
/// the patch family, though it is a wait/sync no-op rather than a patch.
fn agc_no_op_returns_ok(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Non-export inline query helper (`0x7d86501b8094ef57`).
///
/// Disassembly of the call site at `0x4000000435b5` in PPSA02664 (Alex Kidd in Miracle World,
/// worklog 725) reads as a `GetSize`: `arg0` is a pointer to an out-parameter where a byte size is
/// written, which the caller aligns up to 8 (`add rbx, 7; and rbx, ~7`) and passes to buffer
/// allocators.
///
/// The value written, `0xa8` (168), is **guest-observed, not a measured return.** It is the size the
/// guest's `memcpy` reads at the wall this path dies on - the `+0xa8` read through a null pointer. On
/// retail this NID is not exported: obSCEne `e245` and sweep `20260920-110931` (`166-agc/cb-unnamed-ef57`)
/// both measured its import slot binding null and the call never being made (`call-executed 0x0`,
/// `slot-is-null`), so there is no hardware return to measure. `0xa8` is the best-available stand-in
/// for the size the title's own inlined helper computes, and worklog 725 showed planting it does not
/// by itself clear the wall - it is a plausible value, held as such, not a fix dressed as one.
fn agc_phantom_get_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[0];
    if out != 0 {
        // SAFETY: `out` is the guest-supplied out-parameter stack slot (worklog 725),
        // eight bytes long and aligned to hold the workload size.
        unsafe { poke_u64(out, 0xa8) };
    }
    OK
}

/// `sceAgcInit(state, version)` (and alias NID `0x53bbd82b51d172db`).
///
/// Measured on live PS5 console hardware (FW 12.40, REQ-20260920T1425Z-a3f0, check `166-agc/init`):
/// - Version 13 (0xd) returns `0x0` (OK).
/// - Every other version returns `0x8a6c0004` (`SCE_AGC_ERROR_INVALID_VERSION`).
/// - Writes 0 bytes to `arg0` (extent 0, changed 0, across sentinels and descriptors).
fn agc_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let version = args[1] as u32;
    if version == 13 { OK } else { 0x8a6c_0004 }
}

/// `sceAgcGetIsTrinityMode()` - whether the GPU is the later generation's faster revision.
///
/// The graphics-side twin of `sceKernelIsNeoMode` (which the kernel answers from the presented
/// machine): "Trinity" is obSCEne's own axis name for the faster Prospero revision, beside `orbis`,
/// `neo` and `prospero`, so this asks "is this the Pro variant of this generation?". Answered from
/// the presented machine (D394/D397), so a run presenting a base console gets `0` and one presenting
/// the faster revision gets `1`, without a hardcode either way.
///
/// **Arity 0, answered in the register**, on the `sceKernelIsNeoMode` precedent and the same
/// stale-argument reading that `sceAgcGetRegisterDefaults2` needed: the trace's `(ptr, ptr, ...)`
/// shape is leftover registers, not a call that fills an out-parameter. **`known_by = assumed`**: it
/// is the presented machine reported through a plausible SDK spelling, not a measured return - a
/// non-zero placeholder here tells a base console it is the faster revision and can send it down a
/// path that expects hardware it does not have, which is the danger this removes.
fn get_is_trinity_mode(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(
        orbistoun_core::machine::presented().platform()
            == orbistoun_core::machine::Platform::Trinity,
    )
}

/// `sceAgcCbNop(cb)` - a header-only no-op. Measured whole: `166-agc/cb-nop`.
///
/// The packet takes no arguments, so unlike the reservation skeletons this is the complete,
/// measured encoding, not a stand-in. It is in the cluster the retail titles reach on their
/// secondary command buffer (worklog 600).
fn cb_nop(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::nop())
}

/// `sceAgcDcbAcquireMem(dcb, ...)` - reserve the 32-byte ACQUIRE_MEM packet, cursor real, body zero.
///
/// Header and extent are measured (`166-agc/dcb-acquire-mem`); the argument-to-body mapping is a
/// permutation the sweep summary does not pin, so the body is left zero on the same terms as the Cx
/// producer (D696). PPSA02664 calls this three times on its secondary buffer, so a real cursor here
/// is one fewer placeholder the cluster's patches carry (worklog 600).
fn dcb_acquire_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::acquire_mem_skeleton())
}

/// `sceAgcCbReleaseMem(cb, ...)` - reserve the 32-byte RELEASE_MEM packet, cursor real, body zero.
///
/// Header (`0xc0064900`) and extent measured (`166-agc/cb-release-mem`, sweep `20260915-174357`);
/// the argument-to-body permutation is not, so the body is zero on the same terms as the AcquireMem
/// and Cx-producer skeletons (D696). One fewer placeholder in the command-builder cluster the retail
/// titles reach (worklog 600, REQ-...a70f).
fn cb_release_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::release_mem_skeleton())
}

/// `sceAgcDcbDmaData(dcb, ...)` - reserve the 28-byte DMA_DATA packet, cursor real, body zero.
///
/// Header (`0xc0055000`) and extent measured (`166-agc/dcb-dma-data`, sweep `20260915-174357`); the
/// source/destination/size body is an argument permutation a single zero-argument pass cannot pin
/// (and two captures disagreed on it this session), so it is left zero rather than encoded from one
/// pass. The reservation is what moves the wall (REQ-...a70f).
fn dcb_dma_data(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::dma_data_skeleton())
}

/// `sceAgcDcbSetBaseIndirectArgs(dcb, ...)` - reserve the 16-byte SET_BASE packet, cursor real,
/// body zero.
///
/// Header (`0xc0021100`) and extent measured (`166-agc/dcb-set-base-indirect-args`, sweep
/// `20260915-174357`); the body carries the base-index selector and an address, which one pass
/// cannot separate, so it is zeroed like the other skeletons (REQ-...a70f).
fn dcb_set_base_indirect_args(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::set_base_indirect_args_skeleton())
}

/// `sceAgcDcbResetQueue(dcb, ...)` - reserve the measured 32-byte queue-reset writer-struct and hand
/// back a real cursor.
///
/// Framing and extent measured (`166-agc/dcb-reset-queue`, sweep `20260910-174437`, REQ-...b7e4); the
/// two marker-register values are address-shaped and left zero, on the skeleton discipline - see
/// [`packet::build::reset_queue_skeleton`]. D559: PPSA02664 calls this on its writer before any other
/// AGC use, so the reservation is what clears the wall the placeholder held.
fn dcb_reset_queue(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::reset_queue_skeleton())
}

// The REQ-...a70f cluster, wired as reservation skeletons from the headers and extents obSCEne
// measured in sweep 20260915-203058: each hands the guest a real cursor where the unimplemented
// builder handed it a placeholder to memcpy through (worklog 618). Every header is rebuilt from its
// measured opcode through `command_header`, so it walks back to the packet it stands for; the body is
// zero because a single argument pass does not pin the mapping, the same terms as the earlier
// skeletons.
use packet::build::measured;

/// `sceAgcCbDispatch(cb, ...)` - a compute dispatch. Header `0xc0031500`, 20 bytes.
fn cb_dispatch(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(packet::build::DISPATCH_DIRECT, 4),
    )
}

/// `sceAgcDcbDispatchIndirect(dcb, ...)`. Header `0xc0011600`, 12 bytes.
fn dcb_dispatch_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(measured::DISPATCH_INDIRECT, 2),
    )
}

/// `sceAgcAcbDispatchIndirect(acb, ...)`. Header `0xc0021600`, 16 bytes (the Acb twin, one dword
/// longer than the Dcb form - the measured extent, not an assumed one).
fn acb_dispatch_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(measured::DISPATCH_INDIRECT, 3),
    )
}

/// `sceAgcDcbDrawIndirect(dcb, ...)`. Header `0xc0032400`, 20 bytes.
fn dcb_draw_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(measured::DRAW_INDIRECT, 4),
    )
}

/// `sceAgcDcbDrawIndexIndirect(dcb, ...)`. Header `0xc0032500`, 20 bytes.
fn dcb_draw_index_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(measured::DRAW_INDEX_INDIRECT, 4),
    )
}

/// `sceAgcDcbSetShRegistersIndirect(dcb, ...)`. Header `0xc0036300`, 20 bytes. On PPSA02664's path.
fn dcb_set_sh_registers_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(measured::SET_SH_REG_INDIRECT, 4),
    )
}

/// `sceAgcDcbSetUcRegistersIndirect(dcb, ...)`. Header `0xc0036400`, 20 bytes.
fn dcb_set_uc_registers_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(measured::SET_UCONFIG_REG_INDIRECT, 4),
    )
}

/// `sceAgcDcbStallCommandBufferParser(dcb, ...)`. Header `0xc0004200`, 8 bytes.
fn dcb_stall_command_buffer_parser(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::reservation(measured::STALL_COMMAND_BUFFER_PARSER, 1),
    )
}

/// `sceAgcAcbAcquireMem(acb, ...)` - the Acb twin of `sceAgcDcbAcquireMem`, its header `0xc0065800`
/// now dumped for the Acb form too (203058), so the twin is measured rather than assumed.
fn acb_acquire_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::acquire_mem_skeleton())
}

/// `sceAgcDcbPushMarker(dcb, label, ...)` - a debug marker. Measured 12-byte `SET_UCONFIG_REG`
/// (`166-agc/dcb-push-marker`, sweep 20260915-174357), reserved as a skeleton so it hands a real
/// cursor rather than the placeholder it was answering on PPSA02664's path (worklog 618).
fn dcb_push_marker(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::marker_skeleton())
}

/// `sceAgcDcbPopMarker(dcb, ...)` - the closing debug marker, the same 12-byte packet as
/// [`dcb_push_marker`] (the value, unpinned here, is what distinguishes them).
fn dcb_pop_marker(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::marker_skeleton())
}

/// `sceAgcDcbWaitRegMem(dcb, ...)` - wait on a register or memory word. The measured 56-byte compound
/// of three packets (`166-agc/dcb-wait-reg-mem`), reserved with its headers and zeroed bodies.
fn dcb_wait_reg_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::wait_reg_mem_skeleton())
}

/// The largest register run this will read out of guest memory in one call.
///
/// A packet's count field is fourteen bits, so a run longer than this cannot be encoded at all.
/// Refusing here keeps a wild count from turning into a wild read.
const MAX_REGISTER_RUN: u64 = 0x3ffe;

/// `sceAgcCbSetShRegisterRangeDirect(cb, offset, values, count)`. Measured:
/// `166-agc/dcb-set-sh-reg-direct`, where the run was two registers and came back as
/// `header, offset, value, value` - no marker, `n + 2` dwords.
fn cb_set_sh_register_range_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, values, count) = (args[1], args[2], args[3]);
    if values == 0 || count == 0 || count > MAX_REGISTER_RUN {
        return BAD_ARGUMENT;
    }
    let mut run = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: `values` is the guest's own array of `count` dwords, the argument this call is
        // defined by; `count` is bounded above so the walk cannot run away.
        run.push(unsafe { peek_u32(values.wrapping_add(i * 4)) });
    }
    dcb_append(
        args[0],
        &packet::build::set_sh_register_range(offset as u16, &run),
    )
}

/// Implementations this crate provides for `libSceAgc`.
///
/// `sceAgcCreateShader` (object model, 3c5e) and the five shader-linkage calls e4f1 asked for, whose
/// behaviours obSCEne measured once the 3D-draw sweeps settled (9a41, sweep `20260912-003916`): the
/// interpolant mapping pair, the primitive-state pair, and the shader linker.
///
/// `sceAgcDcbDrawIndex(dcb, index_count, address, initiator)`.
///
/// **Now measured whole** (`166-agc/dcb-draw-index`, sweep `20260914-222710`, answering the request
/// worklog 536 left it out for). Called as `(3, 0x12345678, 0)` it wrote
/// `0xc0042700, 3, 0x12345678, 0, 3, 0` - so the index count appears at **both** body[0] and
/// body[3], and the third argument lands in body[4]. The two dwords worklog 536 could not place are
/// placed.
///
/// One argument set, so what body[0] *means* is not established - only that this builder puts the
/// count there. The published field order calls it `MAX_SIZE`, which is consistent and is not what
/// this reproduces from.
fn dcb_draw_index(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (count, address, initiator) = (args[1] as u32, args[2], args[3] as u32);
    dcb_append(
        args[0],
        &packet::build::draw_index_2(count, address, count, initiator),
    )
}

/// `sceAgcDcbSetIndexSize(dcb, type, flags)`.
///
/// **Measured across eight argument pairs**, which is what makes it implementable where worklog 536
/// refused it on one: see [`packet::build::set_index_size`] for the mapping the sweep establishes.
fn dcb_set_index_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_index_size(args[1] as u32, args[2] as u32),
    )
}

/// The command **builders** are wired through the writer handle in `arg0`: their encodings were
/// measured (worklog 534 first, then extended across later sweeps) and implemented as pure encoders
/// or measured reservation skeletons in [`crate::packet::build`], while the handle layout that places
/// them was guest-observed and then confirmed by the library's own behaviour - see the `dcb` module
/// below, which is private. The array below is the authoritative list; this prose does not restate
/// its length, which would go stale as builders land (the count is pinned instead by
/// `tests/dcb_wiring.rs`).
///
/// **The two that worklog 536 refused on a single input each have since been measured across more
/// inputs and wired**, so each is a builder that is right rather than a guess:
///
/// - `sceAgcDcbDrawIndex` - the two body dwords worklog 536 could not place were read back on a
///   later capture (`(3, 0x12345678, 0)` wrote `0xc0042700, 3, 0x12345678, 0, 3, 0`), placing the
///   count at `body[0]` and `body[3]` and the argument at `body[4]`; `packet::build::draw_index_2` now
///   encodes the whole packet.
/// - `sceAgcDcbSetIndexSize` - measured across eight argument pairs, enough to establish the mapping
///   `packet::build::set_index_size` uses rather than emit one measured packet for every call.
///
/// **`sceAgcDcbResetQueue` is wired as a measured reservation skeleton** (REQ-...b7e4): obSCEne
/// measured its whole 32-byte writer-struct with zero arguments (`166-agc/dcb-reset-queue`), and
/// [`packet::build::reset_queue_skeleton`] reproduces the framing - a NOP filler and two
/// `SET_UCONFIG_REG` packets - while zeroing an address-shaped body a single zero-argument pass
/// cannot separate from the probe's own writer-struct pointer. It is the first AGC call PPSA02664
/// makes (D559), so the reservation clears a wall the placeholder held.
///
/// **`sceAgcDcbWaitUntilSafeForRendering` is wired as a measured no-op** (REQ-...4e91). Worklog 665
/// refused it while its only measurement was the 35-builder sweep's `fail (wrote 0)` - a probe
/// failure, not an empty encoding. The re-probe settled it: its `GetSize` symbol is absent from
/// `libSceAgc`, and the builder wrote 0 bytes and returned `0x0` on a bare writer and on one prepared
/// through `sceAgcDcbResetQueue` (sweeps `20260917-090300`/`101310`), `empty-encoding` true under
/// every condition. So it is a library-level no-op, and `agc_no_op_returns_ok` answers the measured
/// `0x0` and writes nothing - like the patch family, it dereferences nothing, so a null needs no guard.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("0x7d86501b8094ef57", agc_phantom_get_size),
        ("sceAgcCreateShader", create_shader),
        ("sceAgcCreateInterpolantMapping", create_interpolant_mapping),
        ("sceAgcUpdateInterpolantMapping", update_interpolant_mapping),
        ("sceAgcCreatePrimState", create_prim_state),
        ("sceAgcUpdatePrimState", update_prim_state),
        ("sceAgcLinkShaders", link_shaders),
        ("sceAgcDcbEventWrite", dcb_event_write),
        ("sceAgcDcbSetIndexCount", dcb_set_index_count),
        ("sceAgcDcbSetNumInstances", dcb_set_num_instances),
        ("sceAgcDcbDrawIndexAuto", dcb_draw_index_auto),
        ("sceAgcDcbSetIndexBuffer", dcb_set_index_buffer),
        ("sceAgcDcbSetCxRegisterDirect", dcb_set_cx_register_direct),
        ("sceAgcDcbSetUcRegisterDirect", dcb_set_uc_register_direct),
        (
            "sceAgcDcbSetCxRegistersIndirect",
            dcb_set_cx_registers_indirect,
        ),
        ("sceAgcCbNop", cb_nop),
        ("sceAgcDcbAcquireMem", dcb_acquire_mem),
        ("sceAgcCbReleaseMem", cb_release_mem),
        ("sceAgcDcbDmaData", dcb_dma_data),
        ("sceAgcDcbSetBaseIndirectArgs", dcb_set_base_indirect_args),
        ("sceAgcDcbResetQueue", dcb_reset_queue),
        ("sceAgcDcbWaitUntilSafeForRendering", agc_no_op_returns_ok),
        // The REQ-...a70f cluster, reservation skeletons from measured headers (sweep 203058).
        ("sceAgcCbDispatch", cb_dispatch),
        ("sceAgcDcbDispatchIndirect", dcb_dispatch_indirect),
        ("sceAgcAcbDispatchIndirect", acb_dispatch_indirect),
        ("sceAgcDcbDrawIndirect", dcb_draw_indirect),
        ("sceAgcDcbDrawIndexIndirect", dcb_draw_index_indirect),
        (
            "sceAgcDcbSetShRegistersIndirect",
            dcb_set_sh_registers_indirect,
        ),
        (
            "sceAgcDcbSetUcRegistersIndirect",
            dcb_set_uc_registers_indirect,
        ),
        (
            "sceAgcDcbStallCommandBufferParser",
            dcb_stall_command_buffer_parser,
        ),
        ("sceAgcAcbAcquireMem", acb_acquire_mem),
        ("sceAgcDcbPushMarker", dcb_push_marker),
        ("sceAgcDcbPopMarker", dcb_pop_marker),
        ("sceAgcDcbWaitRegMem", dcb_wait_reg_mem),
        // The whole `sceAgc*Patch*` family - each measured to return 0x0 (REQ-...4386, ...3d1e).
        (
            "sceAgcSetCxRegIndirectPatchAddRegisters",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcSetCxRegIndirectPatchSetAddress",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcSetShRegIndirectPatchAddRegisters",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcSetShRegIndirectPatchSetAddress",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcSetUcRegIndirectPatchAddRegisters",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcSetUcRegIndirectPatchSetAddress",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcDmaDataPatchSetDstAddressOrOffset",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate",
            agc_patch_returns_ok,
        ),
        ("sceAgcWaitRegMemPatchAddress", agc_patch_returns_ok),
        (
            "sceAgcQueueEndOfPipeActionPatchAddress",
            agc_patch_returns_ok,
        ),
        (
            "sceAgcCbSetShRegisterRangeDirect",
            cb_set_sh_register_range_direct,
        ),
        ("sceAgcDcbDrawIndex", dcb_draw_index),
        ("sceAgcDcbSetIndexSize", dcb_set_index_size),
        ("0x53bbd82b51d172db", agc_init),
        ("sceAgcInit", agc_init),
        ("sceAgcGetIsTrinityMode", get_is_trinity_mode),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The measured object model is what the handler writes.** A guest hands `out` (a slot) and
    /// `header` (its object buffer); after the call, `*out` is the header address, `+0x10` is the
    /// bytecode pointer, and relative offsets (`+0x08`, `+0x20`, `+0x28`, `+0x30`, and sub-table
    /// entries) are relocated to pointers within the header object (measured in obSCEne 166-agc/create-shader
    /// and verified against retail Unity pipelines).
    #[test]
    fn create_shader_fills_the_object_as_hardware_did() {
        // A slot for the object pointer, and a 0x130-byte object buffer with retail-like offsets.
        let mut slot: u64 = 0;
        let mut object = [0u8; 0x130];
        // Set up relative offsets as found in retail shader headers:
        // +0x08: offset 0xd8 (points to sub-table at +0xe0)
        object[0x8..0x10].copy_from_slice(&0xd8u64.to_le_bytes());
        object[0x20..0x28].copy_from_slice(&0x70u64.to_le_bytes());
        object[0x28..0x30].copy_from_slice(&0x38u64.to_le_bytes());
        object[0x30..0x38].copy_from_slice(&0x60u64.to_le_bytes());
        // Sub-table entries at 0xe0..0x108
        object[0xe0..0xe8].copy_from_slice(&0x38u64.to_le_bytes());
        object[0xe8..0xf0].copy_from_slice(&0x48u64.to_le_bytes());

        let out = std::ptr::addr_of_mut!(slot) as u64;
        let header = object.as_mut_ptr() as u64;
        let bytecode = 0x4000_0000_1234u64;

        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = out;
        args[1] = header;
        args[2] = bytecode;

        assert_eq!(create_shader(&args), OK, "a well-formed call succeeds");
        assert_eq!(
            slot, header,
            "*out is the header address - the object is the header region"
        );

        let read_u64 = |off: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&object[off..off + 8]);
            u64::from_le_bytes(b)
        };
        assert_eq!(
            read_u64(0x08),
            header + 0xe0,
            "+0x08 is the sub-table pointer"
        );
        assert_eq!(read_u64(0x10), bytecode, "+0x10 is the bytecode pointer");
        // **The console's own answer, from its capture** (obSCEne `166-agc/create-shader`,
        // `shader-obj`): the probe's header held `0x70` at `+0x20` and `0x38` at `+0x28`, and
        // hardware wrote back `header + 0x90` and `header + 0x60` - each field's own address plus
        // its offset (worklog 872). This test once asserted `header + 0x70`: the belief, not the
        // capture.
        assert_eq!(
            read_u64(0x20),
            header + 0x20 + 0x70,
            "+0x20 is self-relative"
        );
        assert_eq!(
            read_u64(0x28),
            header + 0x28 + 0x38,
            "+0x28 is self-relative"
        );
        assert_eq!(
            read_u64(0x30),
            header + 0x30 + 0x60,
            "+0x30 is self-relative"
        );
        assert_eq!(
            read_u64(0xe0),
            header + 0xe0 + 0x38,
            "sub-table[0] is self-relative (its address is the table's)"
        );
        assert_eq!(
            read_u64(0xe8),
            header + 0xe8 + 0x48,
            "sub-table[1] is self-relative, not relative to the table base"
        );
    }

    /// **Every group-pointer slot that holds a relative offset is relocated, not just the middle
    /// three.** The measured shader populated `+0x20/+0x28/+0x30`; a Unity header like PPSA02664's
    /// populates the whole `+0x18..+0x38` array, and a slot left with its raw offset (`+0x18`'s `0xa8`)
    /// is the pointer the workload walk dies reading (worklog 748). The guard relocates exactly the
    /// slots holding offsets, so a populated endpoint is fixed up and an empty one (0) is left alone.
    #[test]
    fn create_shader_relocates_every_populated_group_slot() {
        let mut slot: u64 = 0;
        let mut object = [0u8; 0x130];
        object[0x8..0x10].copy_from_slice(&0xd8u64.to_le_bytes());
        // The full group array populated, endpoints included (PPSA02664's shape).
        object[0x18..0x20].copy_from_slice(&0xa8u64.to_le_bytes());
        object[0x20..0x28].copy_from_slice(&0x70u64.to_le_bytes());
        object[0x28..0x30].copy_from_slice(&0x38u64.to_le_bytes());
        object[0x30..0x38].copy_from_slice(&0x60u64.to_le_bytes());
        object[0x38..0x40].copy_from_slice(&0x58u64.to_le_bytes());

        let header = object.as_mut_ptr() as u64;
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(slot) as u64;
        args[1] = header;
        args[2] = 0x4000_0000_1234u64;
        assert_eq!(create_shader(&args), OK);

        let read_u64 = |off: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&object[off..off + 8]);
            u64::from_le_bytes(b)
        };
        // The endpoints the old three-entry list left raw are now absolute pointers.
        assert_eq!(
            read_u64(0x18),
            header + 0x18 + 0xa8,
            "+0x18 endpoint is relocated"
        );
        assert_eq!(
            read_u64(0x38),
            header + 0x38 + 0x58,
            "+0x38 endpoint is relocated"
        );
        // The middle three, self-relative like the endpoints.
        assert_eq!(read_u64(0x20), header + 0x20 + 0x70);
        assert_eq!(read_u64(0x28), header + 0x28 + 0x38);
        assert_eq!(read_u64(0x30), header + 0x30 + 0x60);

        // An empty slot (0) is left alone, not turned into `header + 0` - the guard, not a field list,
        // is what keeps the measured three-of-five behaviour for a header without endpoint offsets.
        let mut empty = [0u8; 0x130];
        empty[0x20..0x28].copy_from_slice(&0x70u64.to_le_bytes());
        let header2 = empty.as_mut_ptr() as u64;
        let mut args2 = [0u64; GUEST_ARG_REGISTERS];
        args2[0] = std::ptr::addr_of_mut!(slot) as u64;
        args2[1] = header2;
        args2[2] = 0x1234u64;
        assert_eq!(create_shader(&args2), OK);
        let read2 = |off: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&empty[off..off + 8]);
            u64::from_le_bytes(b)
        };
        assert_eq!(read2(0x18), 0, "an empty +0x18 is not turned into header+0");
        assert_eq!(read2(0x38), 0, "an empty +0x38 is not turned into header+0");
    }

    /// **The console's full byte diff, replayed** (obSCEne REQ-20260925T2056Z-5d19): the register
    /// descriptor at `+0x90` gets the program address in its second (and fourth) dword, the five
    /// sub-table entries land at `+0x118` then `+0x130` four times, and a descriptor naming no
    /// register (stages 4 and 5) is left alone.
    #[test]
    fn create_shader_matches_the_consoles_byte_diff() {
        let run = |reg: u32| {
            let mut slot: u64 = 0;
            let mut object = [0u8; 0x130];
            object[0x08..0x10].copy_from_slice(&0xd8u64.to_le_bytes());
            object[0x20..0x28].copy_from_slice(&0x70u64.to_le_bytes());
            object[0x28..0x30].copy_from_slice(&0x38u64.to_le_bytes());
            object[0x90..0x94].copy_from_slice(&reg.to_le_bytes());
            object[0x98..0x9c].copy_from_slice(&reg.wrapping_add(1).to_le_bytes());
            for (i, rel) in [0x38u64, 0x48, 0x40, 0x38, 0x30].iter().enumerate() {
                object[0xe0 + i * 8..0xe8 + i * 8].copy_from_slice(&rel.to_le_bytes());
            }
            let header = object.as_mut_ptr() as u64;
            let payload = 0x4000_0072_6600u64;
            let mut args = [0u64; GUEST_ARG_REGISTERS];
            args[0] = std::ptr::addr_of_mut!(slot) as u64;
            args[1] = header;
            args[2] = payload;
            assert_eq!(create_shader(&args), OK);
            (object, header, payload)
        };
        let dword = |o: &[u8; 0x130], at: usize| {
            u32::from_le_bytes([o[at], o[at + 1], o[at + 2], o[at + 3]])
        };
        let qword = |o: &[u8; 0x130], at: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&o[at..at + 8]);
            u64::from_le_bytes(b)
        };

        let (object, header, payload) = run(0x2c8c);
        assert_eq!(qword(&object, 0x20), header + 0x90);
        assert_eq!(
            dword(&object, 0x94),
            (payload >> 8) as u32,
            "sh[1] = payload >> 8"
        );
        assert_eq!(
            dword(&object, 0x9c),
            (payload >> 40) as u32,
            "sh[3] = payload >> 40"
        );
        assert_eq!(qword(&object, 0xe0), header + 0x118);
        for entry in [0xe8, 0xf0, 0xf8, 0x100] {
            assert_eq!(qword(&object, entry), header + 0x130, "entry at {entry:#x}");
        }

        let (unregistered, _, _) = run(0);
        assert_eq!(dword(&unregistered, 0x94), 0, "no register named, no patch");
        assert_eq!(dword(&unregistered, 0x9c), 0);
    }

    /// **A null out-parameter is refused, not dereferenced.** The guest's own wrapper answers
    /// `0x8a6c000a` for a null pointer (D556); reproducing that is safer than writing through null.
    #[test]
    fn create_shader_refuses_a_null_out_parameter() {
        let args = [0u64; GUEST_ARG_REGISTERS];
        assert_eq!(
            create_shader(&args),
            BAD_ARGUMENT,
            "null out is the wrapper's bad-argument code"
        );
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
        assert_eq!(
            read_le_u64(&table, 0),
            0x191,
            "entry 0 is SPI_PS_INPUT_CNTL_0"
        );
        assert_eq!(
            read_le_u64(&table, 8),
            0x1_0000_0192,
            "entry 1 is (1<<32)|0x192"
        );
        assert_eq!(
            read_le_u64(&table, 31 * 8),
            (31u64 << 32) | (0x191 + 31),
            "entry 31"
        );
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
        assert_eq!(
            read_le_u64(&link, 0),
            0x191,
            "interpolant table at the front"
        );
        assert_eq!(read_le_u64(&link, 8), 0x1_0000_0192, "second interpolant");
        assert_eq!(
            read_le_u64(&link, 0x108),
            LINK_STAGE_ROUTING,
            "stage routing at +0x108"
        );
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
        assert_eq!(
            after_create & !0x1f,
            0xabcd_ffe0,
            "the other bits are preserved"
        );

        args[2] = 1; // DI_PT_POINTLIST, arg2 for the update
        assert_eq!(update_prim_state(&args), OK);
        let after_update = u32::from_le_bytes(sec[0x14..0x18].try_into().unwrap());
        assert_eq!(
            after_update & 0x1f,
            1,
            "topology updated to DI_PT_POINTLIST"
        );
        assert_eq!(
            after_update & !0x1f,
            0xabcd_ffe0,
            "the other bits still preserved"
        );
    }

    /// **The phantom `0x7d86501b8094ef57` writes workload size 168 (0xa8) bytes into `*arg0`.**
    #[test]
    fn phantom_get_size_writes_workload_size() {
        let mut slot: u64 = 0;
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(slot) as u64;

        assert_eq!(agc_phantom_get_size(&args), OK);
        assert_eq!(slot, 0xa8);
    }

    /// `sceAgcInit` returns 0 for version 13, and 0x8a6c0004 for other versions.
    #[test]
    fn agc_init_validates_version_and_touches_no_state() {
        let mut buf = [0x55u8; 64];
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = buf.as_mut_ptr() as u64;
        args[1] = 13;

        assert_eq!(agc_init(&args), OK);
        assert_eq!(buf, [0x55u8; 64], "arg0 must remain untouched");

        args[1] = 12;
        assert_eq!(agc_init(&args), 0x8a6c_0004);
        assert_eq!(buf, [0x55u8; 64], "arg0 must remain untouched");
    }

    /// `sceAgcGetIsTrinityMode` answers from the presented machine: a base console is not the
    /// faster revision, so it is `0` - the value that keeps a base guest off the Pro-only path.
    ///
    /// Reads the process default (a retail base console, [`machine::presented`]) rather than
    /// presenting one, so it does not race the process-global other tests may set.
    #[test]
    fn get_is_trinity_mode_is_false_on_a_base_console() {
        let args = [0u64; GUEST_ARG_REGISTERS];
        let base = orbistoun_core::machine::Machine::default();
        assert_ne!(
            base.platform(),
            orbistoun_core::machine::Platform::Trinity,
            "the default machine is a base console"
        );
        // Only assert the base answer when nothing has presented the faster revision, so a run
        // that set the global to Trinity does not make this read as a failure.
        if orbistoun_core::machine::presented().platform()
            != orbistoun_core::machine::Platform::Trinity
        {
            assert_eq!(get_is_trinity_mode(&args), 0);
        }
    }
}
