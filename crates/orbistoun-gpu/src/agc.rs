//! The current generation's graphics interface - `libSceAgc` and `libSceAgcDriver`.
//!
//! Every name is read from a real guest's import table or measured on hardware (the
//! shader-linkage builders). Every arity is 6, the trampoline's full capture, not a claim: a wrong
//! arity only degrades a trace, while a wrong name matches no import.
//!
//! The command builders write packets through the writer handle in `arg0`, as fully measured
//! encoders or as reservation skeletons that write the measured header and extent with a zeroed
//! body (D696). The `sceAgc*Patch*` family answers its measured `0x0`. Unimplemented names reach
//! the stub policy by name. The implemented/declared split is pinned by `tests/dcb_wiring.rs`.

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
        // Arity four, deferring to the arity the knowledge base records for this name
        // (`declared_arity_and_recorded_arity_never_disagree`).
        "sceAgcCreateShader" => 4,
        // The shader-linkage set, implemented below from measured behaviour. Arity 6 is the
        // trampoline's full capture; the real arities are recorded in the knowledge file's prose.
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
use orbistoun_mem::guest;

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// The `libSceAgc` error family's "bad argument" answer, which the guest's own validating wrapper
/// returns for a null rdi, rsi or rdx. Returned when a required pointer argument is null, rather
/// than writing through it.
const BAD_ARGUMENT: u64 = 0x8a6c_000a;

/// `sceAgcCreateShader(out, header, bytecode, flags)`.
///
/// The object model is measured (obSCEne `166-agc/create-shader`): the shader object is the
/// guest-supplied header region itself. The call writes the header's address into `*out`, sets
/// `+0x10 = bytecode`, and returns `0`. It converts the self-relative offsets in the header
/// (`+0x8` and its sub-table, and the group-pointer array) into absolute pointers within the
/// object, so render-state routines can dereference them.
fn create_shader(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, header, bytecode) = (args[0], args[1], args[2]);
    if out == 0 || header == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `out` is the guest stack slot the call fills with the object pointer, checked
    // non-null.
    unsafe { guest::write_u64(out, header) };
    // SAFETY: `header` is the guest-owned object region (>= 0x130 bytes).
    // The hardware changes exactly 0x2c bytes within the header (obSCEne 166-agc/create-shader):
    // - +0x08: pointer to the sub-table at header + off_8 + 8
    // - +0x10: bytecode pointer (arg2)
    // - +0x18..+0x38: the group-pointer array; each slot holding a relative offset is relocated
    // - sub-table entries 0..5: relative offsets relocated to header pointers
    // +0x50 and other fields are asset metadata and are not touched.
    unsafe { guest::write_u64(header.wrapping_add(0x10), bytecode) };

    // SAFETY: `header` is the guest-owned object region, at least 0x130 bytes (the measured
    // extent), so the quadword at +0x8 is inside it.
    let off_8 = unsafe { guest::read_u64(header.wrapping_add(0x8)) }.unwrap_or(0);
    if off_8 != 0 && off_8 < 0x1000 {
        let sub_table = header.wrapping_add(off_8).wrapping_add(8);
        // SAFETY: the same +0x8 quadword just read, written back as an absolute pointer.
        unsafe { guest::write_u64(header.wrapping_add(0x8), sub_table) };
        for i in 0..5 {
            let entry_addr = sub_table.wrapping_add(i * 8);
            // SAFETY: `sub_table` is inside the guest's own header object - its own relative
            // offset, rejected above unless non-zero and under 0x1000 - and `i * 8 < 40`, so the
            // entry is within the region the hardware dereferences. Unaligned for the field's sake.
            let rel = unsafe { guest::read_u64(entry_addr) }.unwrap_or(0);
            if rel != 0 && rel < 0x1000 {
                // Self-relative, like every offset in this object: the entry's own address plus its
                // offset (obSCEne `166-agc/create-shader`, `shader-obj`). SAFETY: the same entry
                // just read, written back as an absolute pointer.
                unsafe { guest::write_u64(entry_addr, entry_addr.wrapping_add(rel)) };
            }
        }
    }

    // The five group-pointer slots at +0x18..+0x38, which the guest walks with stride 8. A slot
    // holding a relative offset must be relocated to be a valid pointer, and the guard below
    // relocates exactly those and skips zero, so a header populating only the middle three (the
    // measured one) and one populating the endpoints are both handled.
    for &offset in &[0x18, 0x20, 0x28, 0x30, 0x38] {
        let field = header.wrapping_add(offset);
        // SAFETY: `offset` is one of 0x18..0x38, inside the guest-owned header object whose
        // measured extent is 0x130 bytes.
        let rel = unsafe { guest::read_u64(field) }.unwrap_or(0);
        if rel != 0 && rel < 0x1000 {
            // Self-relative: the field's own address plus its offset. Measured:
            // `166-agc/create-shader` wrote `header + 0x90` over `0x70` at `+0x20`, and `header +
            // 0x60` over `0x38` at `+0x28`. SAFETY: the same field just read, written back as an
            // absolute pointer.
            unsafe { guest::write_u64(field, field.wrapping_add(rel)) };
        }
    }
    // SAFETY: `+0x20` is inside the guest-owned header object (extent 0x130), read after the loop
    // above relocated it.
    let registers = unsafe { guest::read_u64(header.wrapping_add(0x20)) }.unwrap_or(0);
    // SAFETY: `registers` points into the same guest-owned header object, as the relocation above
    // made it, and `patch_program_address` writes only its first four dwords.
    unsafe { patch_program_address(registers, bytecode) };
    OK
}

/// Writes the program's address into the shader's register descriptor, as the hardware does.
///
/// The descriptor `+0x20` points at is `{reg, value, reg + 1, value}`, the program-address register
/// pair. The hardware writes `payload >> 8` into the second dword, and nothing when the first dword
/// names no register (obSCEne `166-agc/create-shader`). The fourth dword takes `payload >> 40`, the
/// pair's high register, as Mesa programs it (`ac_cmdbuf.c:318-324`).
///
/// # Safety
///
/// `registers`, when non-zero, must address sixteen writable bytes of guest memory.
unsafe fn patch_program_address(registers: u64, payload: u64) {
    if registers == 0 {
        return;
    }
    // SAFETY: the caller guarantees sixteen readable bytes at `registers`.
    if unsafe { guest::read_u32(registers) }.unwrap_or(0) == 0 {
        return;
    }
    // SAFETY: as above, writable; the second and fourth dwords of the descriptor.
    unsafe {
        guest::write_u32(registers.wrapping_add(4), (payload >> 8) as u32);
    }
    // SAFETY: as above.
    unsafe {
        guest::write_u32(registers.wrapping_add(12), (payload >> 40) as u32);
    }
}

/// The PS-input interpolant table is thirty-two quadwords.
const INTERPOLANT_ENTRIES: u64 = 32;

/// Writes the default PS-input interpolant table at `at`: entry `i` is `(i << 32) | (0x191 + i)`.
///
/// Measured (obSCEne `166-agc/link-shaders`): it is `sceAgcCreateInterpolantMapping`'s default
/// output and the interpolant half of `sceAgcLinkShaders`' link state. `0x191` is
/// `SPI_PS_INPUT_CNTL_0`, so the default routes input `i` from attribute slot `i`.
///
/// # Safety
///
/// `at` must address `INTERPOLANT_ENTRIES * 8` (256) bytes of guest-owned, writable memory.
unsafe fn write_default_interpolants(at: u64) {
    for i in 0..INTERPOLANT_ENTRIES {
        let entry = (i << 32) | (0x191 + i);
        // SAFETY: the caller guarantees 256 writable bytes at `at`; `i * 8 < 256` by construction.
        unsafe { guest::write_u64(at.wrapping_add(i * 8), entry) };
    }
}

/// Writes the low five bits of the dword at `sec_state + 0x14` to `topology`, preserving the rest.
///
/// Measured (obSCEne `166-agc/update-prim-state`): the probe reads `*(uint32_t *)(sec_state +
/// 0x14) & 0x1f` back after create and update. Read-modify-write, because the guest owns the other
/// twenty-seven bits.
///
/// # Safety
///
/// `sec_state` must address a readable, writable guest buffer of at least `0x18` bytes.
unsafe fn set_topology(sec_state: u64, topology: u32) {
    let field = sec_state.wrapping_add(0x14);
    // SAFETY: the caller guarantees `sec_state + 0x14` is a readable guest dword.
    let prev = unsafe { guest::read_u32(field) }.unwrap_or(0);
    // SAFETY: same field, writable by the same guarantee.
    unsafe { guest::write_u32(field, (prev & !0x1f) | (topology & 0x1f)) };
}

/// `sceAgcCreateInterpolantMapping(mapping, vs, ps)`.
///
/// Fills `mapping` (arg0) with the 32-quadword PS-input table and returns `0`. The only table the
/// hardware was measured writing is the default, so the vs/ps-specific remap is not modelled and
/// the default is written for every input (D010).
fn create_interpolant_mapping(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mapping = args[0];
    if mapping == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `mapping` is the guest-owned 256-byte out-buffer the call fills.
    unsafe { write_default_interpolants(mapping) };
    OK
}

/// `sceAgcUpdateInterpolantMapping(mapping, vs, ps)`.
///
/// Rewrites the mapping table in place and returns `0` (obSCEne `166-agc/update-interpolant`).
/// With no measured remap, the update re-establishes the default table.
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
/// (obSCEne `166-agc/update-prim-state`). The `prim_state` routing block is left as the guest
/// prepared it: its contents are unmeasured.
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
/// Updates the topology (arg2) in the low five bits of `sec_state + 0x14` and returns `0`
/// (`166-agc/update-prim-state`). The routing word at `prim_state + 0xc` is not written, its value
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

/// The stage-routing quadword `sceAgcLinkShaders` writes at `link_state + 0x108`, measured in
/// obSCEne `166-agc/link-shaders`.
const LINK_STAGE_ROUTING: u64 = 0x0000_0002_0000_029b;

/// `sceAgcLinkShaders(link_state, sec_state, null, vs, ps, ...)`.
///
/// Fills `link_state` (arg0) with the 256-byte interpolant table at `+0x0` and the stage-routing
/// quadword at `+0x108`, then returns `0`, as measured in `166-agc/link-shaders`. The rest of the
/// 32-byte routing block is unmeasured and left as the guest prepared it.
fn link_shaders(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let link_state = args[0];
    if link_state == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `link_state` is the guest-owned out-buffer (>= 0x110 bytes) the call fills.
    unsafe { write_default_interpolants(link_state) };
    // SAFETY: same buffer; `+0x108` is the measured routing quadword's slot.
    unsafe { guest::write_u64(link_state.wrapping_add(0x108), LINK_STAGE_ROUTING) };
    OK
}

/// Field offsets within the command-buffer writer handle every `sceAgcDcb*`/`sceAgcCb*` builder
/// takes in `arg0`.
///
/// Guest-observed, then confirmed on hardware: builders called through a handle of this shape
/// write correct packets and advance the cursor by exactly their own `GetSize` answer. Poisoning
/// `+0x20` makes the library call through it, so it holds a function pointer. `+0x00` is the
/// buffer start, `+0x08` one past its end, `+0x20` the overflow callback and `+0x30` a
/// reserved-dword counter. This writes through the cursor, respects the limit, and touches
/// nothing else.
mod dcb {
    /// The write cursor, the field a builder advances: `cur - begin` after a call is exactly the
    /// builder's own `GetSize` answer.
    pub(super) const CUR: u64 = 0x10;
    /// The limit a builder checks a packet against before writing it.
    pub(super) const LIMIT: u64 = 0x18;
}

// A builder returns the address of the packet it just wrote.
//
// obSCEne's measurements reset the writer before each call, so "the packet's address" and "the
// buffer's start" fit the data equally. The packet's address is taken because guests pass a
// builder's return straight into `sceAgcSetCxRegIndirectPatchAddRegisters`, and the patch family
// amends an already-written packet; the buffer start would let only the first packet be patched.

/// Appends one packet to the writer handle at `dcb`, advancing its cursor by the packet's length.
///
/// The effectful half that [`crate::packet::build`] does not have: the encoders there are pure,
/// and this places what they produce.
///
/// Refuses rather than overruns. When a packet does not fit, the real library calls the overflow
/// callback at `+0x20`; calling a guest callback from a shim is unmeasured, so this answers the
/// placeholder instead, which cannot be mistaken for a firmware code (D670).
fn dcb_append(dcb: u64, words: &[u32]) -> u64 {
    if dcb == 0 || words.is_empty() {
        return BAD_ARGUMENT;
    }
    // SAFETY: `dcb` is the guest-owned writer handle the guest passed in arg0; the cursor and the
    // limit are quadwords at its `+0x10` and `+0x18`, the offsets the hardware reads.
    let cur = unsafe { guest::read_u64(dcb.wrapping_add(dcb::CUR)) }.unwrap_or(0);
    // SAFETY: the same handle, the adjacent field.
    let limit = unsafe { guest::read_u64(dcb.wrapping_add(dcb::LIMIT)) }.unwrap_or(0);
    let length = words.len() as u64 * 4;
    if cur == 0 || limit == 0 || cur.wrapping_add(length) > limit {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    }
    for (i, word) in words.iter().enumerate() {
        // SAFETY: every offset written is below `length`, and `cur + length <= limit` was checked
        // above, so each dword lands inside the guest's own command buffer.
        unsafe { guest::write_u32(cur.wrapping_add(i as u64 * 4), *word) };
    }
    // SAFETY: the cursor field of the same handle, advanced by what was just written.
    unsafe { guest::write_u64(dcb.wrapping_add(dcb::CUR), cur.wrapping_add(length)) };
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
/// Measured (`166-agc/dcb-set-cx-reg`, `dcb-set-uc-reg`): `(value << 32) | offset` comes back in
/// the packet as the offset then the value, both unchanged.
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

/// `sceAgcDcbSetCxRegistersIndirect(dcb, ...)` - reserves the packet a title patches and hands
/// back its real address.
///
/// The producer half of the patch family. It appends a correctly sized packet and returns the real
/// cursor, so the guest's `memcpy` and the patch that follows land in command-buffer memory. The
/// header and 20-byte extent are measured; which argument becomes which body dword is not, and the
/// guest fills the body itself through `sceAgcSetCxRegIndirectPatchAddRegisters` (D696).
fn dcb_set_cx_registers_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_cx_registers_indirect_skeleton(),
    )
}

/// The `sceAgc*Patch*` family - amend an already-written packet in place, and return the measured
/// `0x0`.
///
/// obSCEne measures every patch in the family returning `0x0` across two argument passes
/// (`166-agc/patch-*`): the Cx/Sh/Uc register patches (`AddRegisters` and `SetAddress`), the two
/// DmaData address patches, and the wait-reg-mem and end-of-pipe address patches. The field each
/// amends is a GPU-submission detail the CPU-side flow does not read, and a guest that needs the
/// bytes writes them itself, so the one handler writes nothing. `packet` is not dereferenced, so a
/// null needs no guard.
fn agc_patch_returns_ok(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceAgcDcbWaitUntilSafeForRendering(dcb, ...)` - a measured library-level no-op.
///
/// obSCEne measures 0 bytes written and `0x0` returned under every condition, with no `GetSize`
/// symbol in `libSceAgc`. It writes nothing and dereferences no argument, so a null handle needs
/// no guard.
fn agc_no_op_returns_ok(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Non-export inline query helper (`0x7d86501b8094ef57`).
///
/// At its call site the value written through `arg0` is used as a byte size, aligned up to 8 and
/// passed to buffer allocators. The value, `0xa8` (168), is guest-observed, not a measured return:
/// on the hardware this NID is not exported, its import slot binds null and the call is never made
/// (`166-agc/cb-unnamed-ef57`). `0xa8` stands in for the size the title's own inlined helper
/// computes.
fn agc_phantom_get_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[0];
    if out != 0 {
        // SAFETY: `out` is the guest-supplied out-parameter stack slot, eight bytes, holding the
        // size.
        unsafe { guest::write_u64(out, 0xa8) };
    }
    OK
}

/// `sceAgcInit(state, version)` (and alias NID `0x53bbd82b51d172db`).
///
/// Measured on Prospero-generation hardware (`166-agc/init`): version 13 (`0xd`) returns `0x0`,
/// every other version `0x8a6c0004` (`SCE_AGC_ERROR_INVALID_VERSION`), and nothing is written to
/// `arg0`.
fn agc_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let version = args[1] as u32;
    if version == 13 { OK } else { 0x8a6c_0004 }
}

/// `sceAgcGetIsTrinityMode()` - whether the GPU is the faster revision of this generation.
///
/// The graphics-side twin of `sceKernelIsNeoMode`, answered from the presented machine (D394): `0`
/// for a base machine, `1` for the faster revision. Arity 0, answered in the register; the
/// trace's pointer-shaped arguments are stale registers. The return is assumed, not measured, and
/// a non-zero placeholder would send a base machine down the faster revision's path.
fn get_is_trinity_mode(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(
        orbistoun_core::machine::presented().platform()
            == orbistoun_core::machine::Platform::Trinity,
    )
}

/// `sceAgcCbNop(cb)` - a header-only no-op, measured whole (`166-agc/cb-nop`). It takes no
/// arguments, so this is the complete encoding.
fn cb_nop(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::nop())
}

/// `sceAgcDcbAcquireMem(dcb, ...)` - reserves the 32-byte ACQUIRE_MEM packet, cursor real, body
/// zero.
///
/// Header and extent are measured (`166-agc/dcb-acquire-mem`); the argument-to-body mapping is an
/// unpinned permutation, so the body is zero (D696).
fn dcb_acquire_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::acquire_mem_skeleton())
}

/// `sceAgcCbReleaseMem(cb, ...)` - reserves the 32-byte RELEASE_MEM packet, cursor real, body zero.
///
/// Header (`0xc0064900`) and extent are measured (`166-agc/cb-release-mem`); the argument-to-body
/// permutation is not, so the body is zero (D696).
fn cb_release_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::release_mem_skeleton())
}

/// `sceAgcDcbDmaData(dcb, ...)` - reserves the 28-byte DMA_DATA packet, cursor real, body zero.
///
/// Header (`0xc0055000`) and extent are measured (`166-agc/dcb-dma-data`); the source, destination
/// and size body is an argument permutation one zero-argument pass cannot pin, so it is zero.
fn dcb_dma_data(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::dma_data_skeleton())
}

/// `sceAgcDcbSetBaseIndirectArgs(dcb, ...)` - reserves the 16-byte SET_BASE packet, cursor real,
/// body zero.
///
/// Header (`0xc0021100`) and extent are measured (`166-agc/dcb-set-base-indirect-args`); the body
/// carries a base-index selector and an address one pass cannot separate, so it is zeroed.
fn dcb_set_base_indirect_args(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::set_base_indirect_args_skeleton())
}

/// `sceAgcDcbResetQueue(dcb, ...)` - reserves the measured 32-byte queue-reset stream and hands
/// back a real cursor.
///
/// Framing and extent are measured (`166-agc/dcb-reset-queue`); the two marker-register values are
/// address-shaped and left zero - see [`packet::build::reset_queue_skeleton`]. Guests call it on
/// their writer before any other command builder.
fn dcb_reset_queue(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::reset_queue_skeleton())
}

// Reservation skeletons from measured headers and extents: each hands the guest a real cursor.
// Every header is rebuilt from its measured opcode through `command_header`, so it walks back to
// the packet it stands for; the body is zero because one argument pass does not pin the mapping.
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

/// `sceAgcAcbDispatchIndirect(acb, ...)`. Header `0xc0021600`, 16 bytes: the measured Acb form is
/// one dword longer than the Dcb form.
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

/// `sceAgcDcbSetShRegistersIndirect(dcb, ...)`. Header `0xc0036300`, 20 bytes.
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

/// `sceAgcAcbAcquireMem(acb, ...)` - the Acb twin of `sceAgcDcbAcquireMem`, with the same measured
/// header `0xc0065800`.
fn acb_acquire_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::acquire_mem_skeleton())
}

/// `sceAgcDcbPushMarker(dcb, label, ...)` - a debug marker. The measured 12-byte `SET_UCONFIG_REG`
/// (`166-agc/dcb-push-marker`), reserved as a skeleton so it hands back a real cursor.
fn dcb_push_marker(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::marker_skeleton())
}

/// `sceAgcDcbPopMarker(dcb, ...)` - the closing debug marker, the same 12-byte packet as
/// [`dcb_push_marker`] (the value, unpinned here, is what distinguishes them).
fn dcb_pop_marker(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::marker_skeleton())
}

/// `sceAgcDcbWaitRegMem(dcb, ...)` - wait on a register or memory word. The measured 56-byte
/// compound of three packets (`166-agc/dcb-wait-reg-mem`), reserved with its headers and zeroed
/// bodies.
fn dcb_wait_reg_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::wait_reg_mem_skeleton())
}

/// The largest register run this will read out of guest memory in one call.
///
/// A packet's count field is fourteen bits, so a longer run cannot be encoded, and a wild count
/// is refused instead of becoming a wild read.
const MAX_REGISTER_RUN: u64 = 0x3ffe;

/// `sceAgcCbSetShRegisterRangeDirect(cb, offset, values, count)`. Measured
/// (`166-agc/dcb-set-sh-reg-direct`): a two-register run comes back as `header, offset, value,
/// value`, with no marker, `n + 2` dwords.
fn cb_set_sh_register_range_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, values, count) = (args[1], args[2], args[3]);
    if values == 0 || count == 0 || count > MAX_REGISTER_RUN {
        return BAD_ARGUMENT;
    }
    let mut run = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: `values` is the guest's own array of `count` dwords, the argument this call is
        // defined by; `count` is bounded above.
        run.push(unsafe { guest::read_u32(values.wrapping_add(i * 4)) }.unwrap_or(0));
    }
    dcb_append(
        args[0],
        &packet::build::set_sh_register_range(offset as u16, &run),
    )
}

/// `sceAgcDcbDrawIndex(dcb, index_count, address, initiator)`.
///
/// Measured whole (`166-agc/dcb-draw-index`): `(3, 0x12345678, 0)` wrote
/// `0xc0042700, 3, 0x12345678, 0, 3, 0`, so the index count appears at `body[0]` and `body[3]` and
/// the third argument lands in `body[4]`. One argument set fixes where this builder puts the count,
/// not what `body[0]` means; the published field order calls it `MAX_SIZE`.
fn dcb_draw_index(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (count, address, initiator) = (args[1] as u32, args[2], args[3] as u32);
    dcb_append(
        args[0],
        &packet::build::draw_index_2(count, address, count, initiator),
    )
}

/// `sceAgcDcbSetIndexSize(dcb, type, flags)`, measured across eight argument pairs; see
/// [`packet::build::set_index_size`] for the mapping.
fn dcb_set_index_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_index_size(args[1] as u32, args[2] as u32),
    )
}

/// Implementations this crate provides for `libSceAgc`.
///
/// `sceAgcCreateShader` and the shader-linkage calls (interpolant mapping, primitive state, shader
/// linker) implement measured behaviour. The command builders are wired through the writer handle
/// in `arg0` (the private `dcb` module) as pure encoders or reservation skeletons from
/// [`crate::packet::build`]. `sceAgcDcbWaitUntilSafeForRendering` and the patch family answer
/// their measured `0x0` and write nothing. This array is the authoritative list; its size is
/// pinned by `tests/dcb_wiring.rs`.
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
        // Reservation skeletons from measured headers.
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
        // The whole `sceAgc*Patch*` family, each measured to return 0x0.
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

    /// The measured object model is what the handler writes: `*out` is the header address, `+0x10`
    /// the bytecode pointer, and the relative offsets (`+0x08`, `+0x20`, `+0x28`, `+0x30` and the
    /// sub-table entries) become pointers within the header object.
    #[test]
    fn create_shader_fills_the_object_as_hardware_did() {
        // A slot for the object pointer, and a 0x130-byte object buffer with offsets shaped like a
        // retail shader header.
        let mut slot: u64 = 0;
        let mut object = [0u8; 0x130];
        // Relative offsets as retail shader headers carry them:
        // +0x08: offset 0xd8 (points to the sub-table at +0xe0)
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
        // Each field becomes its own address plus its offset, as the hardware writes back
        // (obSCEne `166-agc/create-shader`, `shader-obj`).
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

    /// Every group-pointer slot that holds a relative offset is relocated, not just the middle
    /// three, and an empty slot (0) is left alone.
    #[test]
    fn create_shader_relocates_every_populated_group_slot() {
        let mut slot: u64 = 0;
        let mut object = [0u8; 0x130];
        object[0x8..0x10].copy_from_slice(&0xd8u64.to_le_bytes());
        // The full group array populated, endpoints included.
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
        // The endpoints are absolute pointers.
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

        // An empty slot (0) is left alone, not turned into `header + 0`: the guard, not a field
        // list, decides what is relocated.
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

    /// `create_shader` reproduces the hardware's byte diff (obSCEne `166-agc/create-shader`),
    /// including leaving a descriptor that names no register alone.
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

    /// A null out-parameter is refused with `0x8a6c000a`, the guest wrapper's own answer, not
    /// dereferenced.
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

    /// The interpolant table is the measured `(i << 32) | (0x191 + i)`; its first two entries are
    /// the values read back from hardware.
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

    /// LinkShaders writes the interpolant table plus the routing quadword at +0x108, both measured
    /// (`166-agc/link-shaders`).
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

    /// Primitive topology lands in the low five bits of `sec_state + 0x14`, create then update:
    /// `DI_PT_TRILIST` (4) then `DI_PT_POINTLIST` (1), with the surrounding bits preserved.
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

    /// The phantom `0x7d86501b8094ef57` writes the workload size 168 (0xa8) into `*arg0`.
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

    /// `sceAgcGetIsTrinityMode` answers `0` for a base machine, keeping a base guest off the faster
    /// revision's path.
    ///
    /// Reads the process default ([`machine::presented`]) rather than presenting one, so it does
    /// not race the process-global other tests may set.
    #[test]
    fn get_is_trinity_mode_is_false_on_a_base_console() {
        let args = [0u64; GUEST_ARG_REGISTERS];
        let base = orbistoun_core::machine::Machine::default();
        assert_ne!(
            base.platform(),
            orbistoun_core::machine::Platform::Trinity,
            "the default machine is a base console"
        );
        // Only assert the base answer when nothing has presented the faster revision, so a run that
        // set the global does not read as a failure.
        if orbistoun_core::machine::presented().platform()
            != orbistoun_core::machine::Platform::Trinity
        {
            assert_eq!(get_is_trinity_mode(&args), 0);
        }
    }
}
