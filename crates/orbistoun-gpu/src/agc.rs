//! The current generation's graphics interface - `libSceAgc` and `libSceAgcDriver`.
//!
//! Every name is read from a real guest's import table or measured on hardware (the
//! shader-linkage builders); the few entries no name is known for are declared by NID. Every arity is 6, the trampoline's full capture, not a claim: a wrong
//! arity only degrades a trace, while a wrong name matches no import.
//!
//! The command builders write packets through the writer handle in `arg0`, as fully measured
//! encoders or as reservation skeletons that write the measured header and extent with a zeroed
//! body (D696). The `sceAgc*Patch*` family answers its measured `0x0`. Unimplemented names reach
//! the stub policy by name. The implemented/declared split is pinned by `tests/dcb_wiring.rs`.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAgc" {
        "0x7d86501b8094ef57" => 3,
        // The index-type builder: (cb, flags, mode, 0), as PPSA25872's text calls it.
        "0x6dd715e95973a4fc" => 4,
        // Unnamed interpolant-table builders: (record, vs, ps, 0, p4, p5) and
        // (record, vs, ps, ptr, registers, 4), as PPSA02664, PPSA03416 and PPSA28061 call them.
        "0x71040c4df8235e1d" => 6,
        "0x8a6f69da59a5b375" => 6,
        // The shader-pair merge and its sizes: (out, A, B, aux, n, 0), as PPSA04263 and PPSA21564
        // call them (`agc_merge`).
        "0x0a4e469ba741de7d" => 6,
        "0xe486fe61994e8976" => 6,
        // Packet-size queries, answered as measured (`measured_size!`).
        "sceAgcAcbAcquireMemGetSize" => 6,
        "sceAgcDcbAcquireMemGetSize" => 6,
        "sceAgcAcbDmaDataGetSize" => 6,
        "sceAgcDcbDmaDataGetSize" => 6,
        "sceAgcAcbDispatchIndirectGetSize" => 6,
        "sceAgcDcbDispatchIndirectGetSize" => 6,
        "sceAgcAcbEventWriteGetSize" => 6,
        "sceAgcAcbJumpGetSize" => 6,
        "sceAgcDcbJumpGetSize" => 6,
        "sceAgcDcbJump" => 6,
        "sceAgcDcbRewind" => 6,
        "sceAgcRewindPatchSetRewindState" => 6,
        "sceAgcCbBranchGetSize" => 6,
        "sceAgcCbQueueEndOfPipeActionGetSize" => 6,
        "sceAgcDcbCondExecGetSize" => 6,
        "sceAgcDcbDrawIndexIndirectGetSize" => 6,
        "sceAgcDcbDrawIndexIndirectMultiGetSize" => 6,
        "sceAgcDcbDrawIndexOffsetGetSize" => 6,
        "sceAgcDcbDrawIndirectGetSize" => 6,
        "sceAgcDcbGetLodStatsGetSize" => 6,
        "sceAgcDcbRewindGetSize" => 6,
        "sceAgcDcbSetCxRegistersIndirectGetSize" => 6,
        "sceAgcDcbSetShRegistersIndirectGetSize" => 6,
        "sceAgcDcbSetUcRegistersIndirectGetSize" => 6,
        "sceAgcDcbSetUcRegisterDirectGetSize" => 6,
        "sceAgcDcbStallCommandBufferParserGetSize" => 6,
        "sceAgcDcbSetIndexCountGetSize" => 6,
        "sceAgcCbNopGetSize" => 6,
        "sceAgcAcbAcquireMem" => 6,
        "sceAgcAcbDispatchIndirect" => 6,
        "sceAgcAcbDmaData" => 6,
        "sceAgcAcbEventWrite" => 6,
        "sceAgcAcbResetQueue" => 6,
        "sceAgcAcbWaitRegMem" => 6,
        "sceAgcAcbWriteData" => 6,
        "sceAgcCbDispatch" => 6,
        "sceAgcCbNop" => 2,
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
        "sceAgcDcbDrawIndexOffset" => 6,
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
        "sceAgcDcbSetShRegisterDirect" => 6,
        "sceAgcDcbSetUcRegistersIndirect" => 6,
        "sceAgcDcbStallCommandBufferParser" => 6,
        "sceAgcDcbWaitRegMem" => 6,
        "sceAgcDcbWaitUntilSafeForRendering" => 6,
        "sceAgcDcbWriteData" => 6,
        // (packet, address)
        "sceAgcWriteDataPatchSetAddressOrOffset" => 2,
        "sceAgcDmaDataPatchSetDstAddressOrOffset" => 6,
        "sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate" => 6,
        "sceAgcGetIsTrinityMode" => 0,
        "sceAgcGetRegisterDefaults2" => 1,
        "sceAgcGetRegisterDefaults2Internal" => 1,
        "0x53bbd82b51d172db" => 2,
        "sceAgcInit" => 2,
        "sceAgcQueueEndOfPipeActionPatchAddress" => 6,
        "sceAgcQueueEndOfPipeActionPatchData" => 6,
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

/// How many shader pairs an interpolant builder's headers are reported for.
const PAIRS_REPORTED: usize = 4;

/// The bytes of a shader header reported: the measured object extent (`166-agc/create-shader`).
const HEADER_BYTES: usize = 0x130;

/// The bytes reported where they can be read: the sub-table at `+0x08` points past the measured
/// extent, to arrays a header carries after it.
const HEADER_AND_TABLES: usize = 0x200;

/// The shader pairs whose headers were reported, by `(vs, ps)` address.
static PAIRS_SEEN: std::sync::Mutex<Vec<(u64, u64)>> = std::sync::Mutex::new(Vec::new());

/// Whether `pair` is new to `seen` and there is room for it among `most`, keeping it if so.
fn first_sight(seen: &mut Vec<(u64, u64)>, pair: (u64, u64), most: usize) -> bool {
    if seen.contains(&pair) || seen.len() >= most {
        return false;
    }
    seen.push(pair);
    true
}

/// `bytes` set out sixteen a line, each line after its offset.
fn hex_lines(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
        let _ = writeln!(text, "  +{:#05x}  {}", row * 16, hex.join(" "));
    }
    text
}

/// Reports the vertex and pixel shader headers an interpolant builder was handed, the first time a
/// pair is seen and for the first few pairs: the table the builder writes leaves out the remap by
/// the shaders' semantics, and these bytes are where those semantics are (D010). It only reads.
fn report_interpolant_pair(vs: u64, ps: u64) {
    let new = PAIRS_SEEN
        .lock()
        .is_ok_and(|mut seen| first_sight(&mut seen, (vs, ps), PAIRS_REPORTED));
    if !new {
        return;
    }
    let header = |at: u64| {
        [HEADER_AND_TABLES, HEADER_BYTES]
            .into_iter()
            .find_map(|len| {
                let mut bytes = vec![0_u8; len];
                // SAFETY: a shader object the guest made with `sceAgcCreateShader`, at least
                // `HEADER_BYTES` long as measured; read through the checked accessor, which refuses
                // a span it cannot read.
                (at != 0 && unsafe { guest::read_into(at, &mut bytes) }).then(|| hex_lines(&bytes))
            })
            .unwrap_or_else(|| "  unreadable\n".to_owned())
    };
    tracing::info!(
        "an interpolant builder was handed vertex shader {vs:#x} and pixel shader {ps:#x}; their headers, whose semantics the table it writes leaves out:\nvertex\n{}pixel\n{}",
        header(vs),
        header(ps)
    );
}

/// What a flat input's entry adds for `0x71040c4df8235e1d`: `FLAT_SHADE` alone (REQ-im01,
/// `item2-flat-unmatched-7104`).
const FLAT_0X7104: u32 = 0x400;

/// What a flat input's entry adds for `sceAgcCreateInterpolantMapping`: `FLAT_SHADE` and bit 5
/// (REQ-im01, `item2-flat-unmatched-create`).
const FLAT_CREATE: u32 = 0x420;

/// An input no vertex output provides: bit 5 alone (REQ-im01, both builders).
const UNMATCHED: u32 = 0x20;

/// The value of each `SPI_PS_INPUT_CNTL_i`, from the pixel shader's input semantic words and the
/// vertex shader's output ones, as the console wrote it (REQ-im01, `166-agc/interpolant-mapping`):
/// input `i` takes the hardware slot (bits 12:8) of the output whose semantic (bits 7:0) is its own,
/// plus `flat` where the input is flat (bit 22); one no output provides is [`UNMATCHED`]; entries
/// past the inputs keep their own index. An input that is 16-bit (bits 21:20), custom (bit 24) or
/// carries default-value bits (31:28) was not measured and keeps its own index too.
fn route_interpolants(inputs: &[u32], outputs: &[u32], flat: u32) -> [u32; 32] {
    let mut table: [u32; 32] = core::array::from_fn(|i| i as u32);
    for (entry, &input) in table.iter_mut().zip(inputs) {
        let unmeasured = input >> 20 & 3 != 0 || input >> 24 & 1 != 0 || input >> 28 != 0;
        if unmeasured {
            continue;
        }
        *entry = match outputs
            .iter()
            .find(|&&output| output & 0xff == input & 0xff)
        {
            Some(&output) => (output >> 8 & 0x1f) | if input >> 22 & 1 != 0 { flat } else { 0 },
            None => UNMATCHED,
        };
    }
    table
}

/// A shader object's semantic words: the table its `table_at` field points at, as many as its
/// `count` field holds (a `count_bytes`-wide integer at `count_at`). Empty for a null object or an
/// unreadable or implausible table. The fields' places are assumed: they are where GTA V's own
/// shader objects hold a pointer into themselves and a count that match its shaders' interpolants,
/// and the tables obSCEne measured sat at `+0x140` of their objects (REQ-im01, `item3-semantics`).
fn semantic_words(shader: u64, table_at: u64, (count_at, count_bytes): (u64, u32)) -> Vec<u32> {
    if shader == 0 {
        return Vec::new();
    }
    // SAFETY: fields of a shader object the guest made, read through the checked accessor.
    let table = unsafe { guest::read_u64(shader.wrapping_add(table_at)) }.unwrap_or(0);
    // SAFETY: as above.
    let count = unsafe { guest::read_u32(shader.wrapping_add(count_at)) }.unwrap_or(0);
    let count = if count_bytes == 2 {
        count & 0xffff
    } else {
        count
    };
    if table == 0 || u64::from(count) > INTERPOLANT_ENTRIES {
        return Vec::new();
    }
    (0..u64::from(count))
        // SAFETY: entry `i` of the table the object points at, read through the checked accessor.
        .filter_map(|i| unsafe { guest::read_u32(table.wrapping_add(i * 4)) })
        .collect()
}

/// Writes the routed table for `vs` and `ps` into `mapping` over the default it holds, the entries
/// a pixel shader's inputs name (REQ-im01). A null pixel shader, or one declaring no inputs, keeps
/// the default, as measured.
fn write_routed_interpolants(mapping: u64, vs: u64, ps: u64, flat: u32) {
    let inputs = semantic_words(ps, 0x30, (0x50, 4));
    if inputs.is_empty() {
        return;
    }
    let outputs = semantic_words(vs, 0x38, (0x56, 2));
    for (i, value) in route_interpolants(&inputs, &outputs, flat)
        .iter()
        .enumerate()
        .take(inputs.len())
    {
        let entry = (u64::from(*value) << 32) | (0x191 + i as u64);
        // SAFETY: entry `i` < 32 of the guest's 256-byte table.
        unsafe { guest::write_u64(mapping.wrapping_add(i as u64 * 8), entry) };
    }
}

/// `sceAgcCreateInterpolantMapping(mapping, vs, ps)`.
///
/// Fills `mapping` (arg0) with the 32-quadword PS-input table and returns `0`: the default, then
/// each pixel input routed to the vertex output of its semantic, a flat one with `0x420`
/// (REQ-im01, [`route_interpolants`]).
fn create_interpolant_mapping(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    write_interpolant_mapping(args, Some(FLAT_CREATE))
}

/// Writes the default table into the record at `args[0]`, then, where `flat` says the builder's
/// remap is measured, routes the pixel shader at `args[2]`'s inputs to the vertex shader at
/// `args[1]`'s outputs over it (REQ-im01).
fn write_interpolant_mapping(args: &[u64; GUEST_ARG_REGISTERS], flat: Option<u32>) -> u64 {
    let mapping = args[0];
    if mapping == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `mapping` is the guest-owned 256-byte out-buffer the call fills.
    unsafe { write_default_interpolants(mapping) };
    if let Some(flat) = flat {
        write_routed_interpolants(mapping, args[1], args[2], flat);
    }
    OK
}

/// `0x71040c4df8235e1d(record, vs, ps, 0, p4, p5)`, unnamed: writes the default PS-input
/// interpolant table into `record` and answers `0`.
///
/// Measured (`200-census/libSceAgc/0x71040c4df8235e1d`, `20261008-142306-eboot.obs.log`): over a
/// record seeded with prim-state words, with header-only shaders and with and without a pixel
/// shader, the call answered `0x0`, the record's first 64 bytes read back as `(0x191 + i, i)`, and
/// neither `p4` nor `p5` changed. The table is taken to be the 32 entries
/// `sceAgcCreateInterpolantMapping` writes, the shape of the 0x100 bytes PPSA02664 copies out of
/// it into an indirect register load. Each pixel input is then routed to the vertex output of its
/// semantic, a flat one with `0x400` (REQ-im01, [`route_interpolants`]).
/// A null record is refused, as the census's zero-argument call faulted on hardware.
fn interpolants_71040c4d(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    report_interpolant_pair(args[1], args[2]);
    write_interpolant_mapping(args, Some(FLAT_0X7104))
}

/// `0x8a6f69da59a5b375(record, vs, ps, ptr, registers, 4)`, unnamed: writes the default PS-input
/// interpolant table into `record` and answers `0`.
///
/// Measured (`200-census/libSceAgc/0x8a6f69da59a5b375`, `20261008-142306-eboot.obs.log`): with a
/// real passthrough vertex shader and a pixel shader, over a record filled with `0xcc`, the call
/// answered `0x0` and the record's first 64 bytes read back as `(0x191 + i, i)`; `ptr` kept its
/// fill and the zeroed register table stayed zero. Otherwise as [`interpolants_71040c4d`].
fn interpolants_8a6f69da(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    report_interpolant_pair(args[1], args[2]);
    write_interpolant_mapping(args, None)
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

/// `VGT_GS_OUT_PRIM_TYPE`'s value `sceAgcCreatePrimState` writes into `prim_state + 0xc` for each
/// input topology, as `166-agc/create-prim-state` measured it for the eight it called: points 0,
/// lines 1, triangle lists, strips, fans and line-adjacency topologies 2, quads 3, rectangle lists 4.
const MEASURED_OUT_PRIMITIVES: [(u32, u32); 8] = [
    (1, 0),
    (2, 1),
    (4, 2),
    (5, 2),
    (6, 2),
    (7, 3),
    (9, 2),
    (17, 4),
];

/// `GE_USER_VGPR_EN`'s user-config offset (`gfx103.json`), which `sceAgcCreatePrimState` names
/// first in `sec_state`, at `+0x8`, its value beside it measured zero (`166-agc/create-prim-state`).
const PRIM_STATE_UC_FIRST: u32 = 0x262;
/// `VGT_PRIMITIVE_TYPE`'s user-config offset (`gfx103.json`), named at `sec_state + 0x10` with the
/// topology beside it.
const VGT_PRIMITIVE_TYPE_OFFSET: u32 = 0x242;
/// `VGT_GS_OUT_PRIM_TYPE`'s context offset (`gfx103.json`), named at `prim_state + 0x8`.
const VGT_GS_OUT_PRIM_TYPE_OFFSET: u32 = 0x29b;

/// The bytes of `prim_state` the create writes: two `(offset, value)` pairs, and nothing past them
/// (D781, measured over a buffer filled with `0xa5`).
const PRIM_STATE_LIST_BYTES: usize = 0x10;
/// The bytes of `sec_state` the create writes: three pairs, and nothing past them (D781, measured
/// as above).
const SEC_STATE_LIST_BYTES: usize = 0x18;

/// `sceAgcCreatePrimState(prim_state, sec_state, gs, vs, topology)`.
///
/// Fills the two register lists as `166-agc/create-prim-state` measured them, and returns `0`.
/// With a geometry shader (`gs` non-null), `prim_state` is written zero and names no register:
/// the output primitive is the geometry shader's (obSCEne `REQ-20261013T1900Z-ps04`,
/// `166-agc/update-prim-state`, `create-geom`).
/// `sec_state` holds user-config `(offset, value)` pairs: `GE_USER_VGPR_EN` named at `+0x8`, and
/// `VGT_PRIMITIVE_TYPE` at `+0x10` with the topology (arg4) in the low five bits of `+0x14`, which a
/// title copies into an indirect register load. `prim_state` names `VGT_GS_OUT_PRIM_TYPE` at `+0x8`
/// with the output primitive at `+0xc`, for a topology the measurement called. Each list is written
/// whole, its unnamed bytes zero, and nothing after it (D781); the output primitive of an
/// unmeasured topology stays zero.
fn create_prim_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (prim_state, sec_state, topology) = (args[0], args[1], args[4] as u32);
    if prim_state == 0 || sec_state == 0 {
        return BAD_ARGUMENT;
    }
    let geometry = args[2] != 0;
    // SAFETY: `sec_state` is the guest-owned buffer the call fills, at least as long as the list a
    // title copies out of it.
    unsafe { guest::write_bytes(sec_state, &[0; SEC_STATE_LIST_BYTES]) };
    // SAFETY: as above, for `prim_state`.
    unsafe { guest::write_bytes(prim_state, &[0; PRIM_STATE_LIST_BYTES]) };
    // SAFETY: `+0x8` and `+0x10` are dwords of `sec_state`'s list.
    unsafe {
        guest::write_u32(sec_state.wrapping_add(0x8), PRIM_STATE_UC_FIRST);
    }
    // SAFETY: as above.
    unsafe {
        guest::write_u32(sec_state.wrapping_add(0x10), VGT_PRIMITIVE_TYPE_OFFSET);
    }
    // SAFETY: `+0x14` is a dword of the same buffer.
    unsafe { set_topology(sec_state, topology) };
    if geometry {
        return OK;
    }
    // SAFETY: `prim_state` is the guest-owned 64-byte routing block; `+0x8` is a dword in it.
    unsafe {
        guest::write_u32(prim_state.wrapping_add(0x8), VGT_GS_OUT_PRIM_TYPE_OFFSET);
    }
    // SAFETY: as above.
    unsafe { set_out_primitive(prim_state, topology) };
    OK
}

/// Writes the output primitive a measured topology maps to at `prim_state + 0xc`; an unmeasured
/// topology leaves it.
///
/// # Safety
///
/// `prim_state` must be a guest-owned block at least 0x10 bytes long.
unsafe fn set_out_primitive(prim_state: u64, topology: u32) {
    if let Some(&(_, out)) = MEASURED_OUT_PRIMITIVES
        .iter()
        .find(|(input, _)| *input == topology)
    {
        // SAFETY: `+0xc` is a dword of the block, by the caller's contract.
        unsafe {
            guest::write_u32(prim_state.wrapping_add(0xc), out);
        }
    }
}

/// `sceAgcUpdatePrimState(prim_state, sec_state, topology)`.
///
/// Updates the topology (arg2) in the low five bits of `sec_state + 0x14`, writes the output
/// primitive it maps to at `prim_state + 0xc`, and returns `0`. Both are measured for topologies 1,
/// 2, 5, 7, 17 and 4, whether or not the state was made with a geometry shader. `prim_state + 0x8`
/// is left: it names `VGT_GS_OUT_PRIM_TYPE` after a create without one and stays zero after one
/// with (obSCEne `REQ-20261013T1900Z-ps04`, `166-agc/update-prim-state`,
/// `reports/hardware/report-20261013-184500.log`).
fn update_prim_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (prim_state, sec_state, topology) = (args[0], args[1], args[2] as u32);
    if prim_state == 0 || sec_state == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `sec_state` is the guest-owned buffer; `+0x14` is a readable, writable dword.
    unsafe { set_topology(sec_state, topology) };
    // SAFETY: `prim_state` is the guest-owned block the create filled.
    unsafe { set_out_primitive(prim_state, topology) };
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
    /// The overflow callback, called as `(writer, dwords, context)` when a packet does not fit.
    pub(super) const OVERFLOW: u64 = 0x20;
    /// The context the overflow callback is handed.
    pub(super) const OVERFLOW_CONTEXT: u64 = 0x28;
}

/// Where a packet of `length` bytes goes in the writer at `dcb`: its cursor, when the packet fits
/// below the limit. When it does not, the writer's overflow callback is asked for room, as
/// `(writer, dwords, context)`, and the cursor and limit are read again if it answers true in `al`
/// (D788). `None` when there is no callback, it refuses, or it leaves too little room.
fn room(dcb: u64, length: u64) -> Option<u64> {
    let fits = || {
        // SAFETY: `dcb` is the guest-owned writer handle the guest passed in arg0; the cursor and
        // the limit are quadwords at its `+0x10` and `+0x18`, the offsets the hardware reads.
        let cur = unsafe { guest::read_u64(dcb.wrapping_add(dcb::CUR)) }.unwrap_or(0);
        // SAFETY: the same handle, the adjacent field.
        let limit = unsafe { guest::read_u64(dcb.wrapping_add(dcb::LIMIT)) }.unwrap_or(0);
        (cur != 0 && limit != 0 && cur.wrapping_add(length) <= limit).then_some(cur)
    };
    if let Some(cur) = fits() {
        return Some(cur);
    }
    // SAFETY: the same handle's callback field.
    let callback = unsafe { guest::read_u64(dcb.wrapping_add(dcb::OVERFLOW)) }.unwrap_or(0);
    if callback == 0 {
        return None;
    }
    // SAFETY: the same handle's context field.
    let context = unsafe { guest::read_u64(dcb.wrapping_add(dcb::OVERFLOW_CONTEXT)) }.unwrap_or(0);
    // SAFETY: `callback` is the function the guest installed in its own writer for exactly this
    // call, and the writer and context are what the guest put beside it.
    let answer =
        unsafe { orbistoun_kernel::thread::call_guest(callback, [dcb, length / 4, context]) }?;
    // A bool answer: only `al` is defined.
    if answer as u8 == 0 {
        return None;
    }
    fits()
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
/// Refuses rather than overruns: a packet that does not fit asks the writer's overflow callback
/// for room ([`room`], D788), and one it gets none for answers the placeholder, which cannot be
/// mistaken for a firmware code (D670).
fn dcb_append(dcb: u64, words: &[u32]) -> u64 {
    if dcb == 0 || words.is_empty() {
        return BAD_ARGUMENT;
    }
    let length = words.len() as u64 * 4;
    let Some(cur) = room(dcb, length) else {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    };
    for (i, word) in words.iter().enumerate() {
        // SAFETY: every offset written is below `length`, and `cur + length <= limit` was checked
        // above, so each dword lands inside the guest's own command buffer.
        unsafe { guest::write_u32(cur.wrapping_add(i as u64 * 4), *word) };
    }
    // SAFETY: the cursor field of the same handle, advanced by what was just written.
    unsafe { guest::write_u64(dcb.wrapping_add(dcb::CUR), cur.wrapping_add(length)) };
    cur
}

/// Appends `words` and advances the cursor by `dwords`, leaving the rest of the span unwritten - a
/// builder that reserves more than it writes, as `sceAgcDcbSetFlip` skips over its `NOP` body.
/// Refuses as [`dcb_append`] does when the whole span does not fit.
fn dcb_append_spanning(dcb: u64, words: &[u32], dwords: usize) -> u64 {
    if dcb == 0 || words.is_empty() || words.len() > dwords {
        return BAD_ARGUMENT;
    }
    let length = dwords as u64 * 4;
    let Some(cur) = room(dcb, length) else {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    };
    for (i, word) in words.iter().enumerate() {
        // SAFETY: every offset written is below `length`, and `cur + length <= limit` was checked
        // above, so each dword lands inside the guest's own command buffer.
        unsafe { guest::write_u32(cur.wrapping_add(i as u64 * 4), *word) };
    }
    // SAFETY: the cursor field of the same handle, advanced by the whole span.
    unsafe { guest::write_u64(dcb.wrapping_add(dcb::CUR), cur.wrapping_add(length)) };
    cur
}

/// The flip modes whose packets are measured: 1 (`-1d54`), 2 and 3 (`-c4e3`). Mode 0 writes
/// nothing; any higher mode's register word is unmeasured.
const MEASURED_FLIP_MODES: std::ops::RangeInclusive<u64> = 1..=3;

/// `sceAgcDcbSetFlip(dcb, video_handle, buffer_index, flip_mode, flip_arg)`: queues the flip with
/// the display and writes the packet whose release carries it out (D728). Measured
/// (`166-agc/dcb-set-flip`, `-1d54` and `-c4e3`): 76 bytes written, the cursor advanced 256, the
/// packet's address answered; a port that is not open or a buffer that is not registered writes
/// nothing and answers `0`, and so does flip mode 0. A mode above 3 is refused, its register word
/// being unmeasured.
fn dcb_set_flip(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dcb, handle, index, mode, arg) = (args[0], args[1], args[2], args[3], args[4]);
    if mode == 0 {
        return 0;
    }
    if !MEASURED_FLIP_MODES.contains(&mode) {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    }
    let (Ok(index32), Ok(mode32)) = (u32::try_from(index), u32::try_from(mode)) else {
        return 0;
    };
    let Some((context, label)) = crate::display::queue_flip(handle, index, arg) else {
        return 0;
    };
    dcb_append_spanning(
        dcb,
        &crate::display::set_flip_words(index32, mode32, arg, context, label),
        crate::display::SET_FLIP_DWORDS,
    )
}

/// `sceAgcSuspendPoint(...)`: the point a title offers the system to suspend its GPU work at.
/// Outside a system suspend it answers `0` at once and writes nothing: PPSA02664's arguments
/// `(0x109, 3, buffer, 0, 1_000_000_000, 0)` returned `0` in 27 us with none of the buffer's 64
/// bytes changed (obSCEne REQ-20260928T0730Z-7b14). Nothing here suspends a title.
fn suspend_point(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
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

/// `sceAgcDcbSetShRegisterDirect(dcb, (value << 32) | offset)`: a one-value `SET_SH_REG`. Measured
/// (`166-agc/dcb-set-sh-reg-direct`, `20261011-002100-eboot.obs.log` 11592-11598): header
/// `0xc0017600`, the offset, the value, twelve bytes.
fn dcb_set_sh_register_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, value) = unpack_register_entry(args[1]);
    dcb_append(
        args[0],
        &packet::build::set_sh_register_range(offset, &[value]),
    )
}

/// `sceAgcDcbSetCxRegistersIndirect(dcb, ...)` - reserves the packet a title patches and hands
/// back its real address.
///
/// The producer half of the patch family. It appends the measured 20-byte packet, its body filled
/// from `(dcb, table, count)`, and returns the real cursor, so the patches that follow amend it in
/// command-buffer memory (`166-agc/patch-cx-registers-indirect`).
fn dcb_set_cx_registers_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_cx_registers_indirect(args[1], args[2]),
    )
}

/// Offset of an indirect register packet's table address, low half then high half.
const INDIRECT_TABLE_AT: u64 = 4;

/// Offset of an indirect register packet's register count.
const INDIRECT_COUNT_AT: u64 = 16;

/// `sceAgcSet{Cx,Sh,Uc}RegIndirectPatchAddRegisters(packet, count, ...)`: adds `count` to the
/// packet's register count and answers `0x0`. Measured: one Cx call moved dw4 from `0x3880` to
/// `0x3881`, a second to `0x3882`, and nothing else in the packet or workload changed
/// (`166-agc/patch-cx-registers-indirect`); the Sh and Uc ones changed dw4's low byte alone, by
/// one for a count of one (`166-agc/patch-{sh,uc}-reg-add-registers`).
fn indirect_patch_add_registers(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (packet, count) = (args[0], args[1] as u32);
    if packet != 0 {
        let at = packet.wrapping_add(INDIRECT_COUNT_AT);
        // SAFETY: `packet` is the producer's answer the guest passes back, a 20-byte packet in its
        // own command buffer; dw4 lies inside it.
        let was = unsafe { guest::read_u32(at) }.unwrap_or(0);
        let now = (was & !packet::build::INDIRECT_COUNT_MASK)
            | (was.wrapping_add(count) & packet::build::INDIRECT_COUNT_MASK);
        // SAFETY: the same dword, in the guest's own command buffer.
        unsafe { guest::write_u32(at, now) };
    }
    OK
}

/// `sceAgcSet{Cx,Sh,Uc}RegIndirectPatchSetAddress(packet, address)`: writes the table address
/// into dw1 and dw2 and answers `0x0`. Measured: Cx address `0x2_0086_0000` came back as dw1
/// `0x860000`, dw2 `0x2` (`166-agc/patch-cx-registers-indirect`); Sh and Uc `0x2000_0000` and
/// `0x4000_0000` as dw1, nothing past it changed (`166-agc/patch-{sh,uc}-reg-set-address`).
fn indirect_patch_set_address(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (packet, address) = (args[0], args[1]);
    if packet != 0 {
        let at = packet.wrapping_add(INDIRECT_TABLE_AT);
        // SAFETY: dw1 and dw2 of the producer's 20-byte packet, in the guest's command buffer.
        unsafe { guest::write_u32(at, (address as u32) & !3) };
        // SAFETY: as above, the next dword.
        unsafe { guest::write_u32(at.wrapping_add(4), (address >> 32) as u32) };
    }
    OK
}

/// Offset of a `DMA_DATA` packet's source address, dw2 and dw3, as `sceAgcDcbDmaData` places its
/// `a7` ([`packet::build::dma_data`]).
const DMA_DATA_SOURCE_AT: u64 = 8;
/// Offset of a `DMA_DATA` packet's destination address, dw4 and dw5, as it places its `a4`.
const DMA_DATA_DESTINATION_AT: u64 = 16;

/// Writes a 64-bit address into the two dwords at `packet + at`, low first.
fn patch_address(packet: u64, at: u64, address: u64) {
    if packet == 0 {
        return;
    }
    let at = packet.wrapping_add(at);
    // SAFETY: two dwords of the packet the producer reserved and the guest passes back, in its own
    // command buffer.
    unsafe { guest::write_u32(at, address as u32) };
    // SAFETY: as above, the next dword.
    unsafe { guest::write_u32(at.wrapping_add(4), (address >> 32) as u32) };
}

/// `sceAgcDmaDataPatchSetDstAddressOrOffset(packet, address)`: writes the destination into dw4 and
/// dw5 and answers `0x0`. Measured: `0x3000_0000` and `0x5000_0000` came back as dw4, nothing
/// else changed (`166-agc/patch-dma-data-dst`, sweep 20260915-203058), and a 64-bit
/// `0x8_8001_0c00` came back as dw4 `0x8001_0c00` and dw5 `0x8`, over a dw5 the probe had set to 1
/// (`arm1-64bit-dst`, `arm2-dw5-one-dst`, `20261009-104652-eboot.obs.log`). PPSA03416 fills a
/// `CP_SYNC` fill's destination this way after building it with zero.
fn dma_data_patch_destination(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    patch_address(args[0], DMA_DATA_DESTINATION_AT, args[1]);
    OK
}

/// `sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate(packet, source)`: writes the source into
/// dw2 and dw3 and answers `0x0`. Measured: `0x4000_0000` and `0x6000_0000` came back as dw2
/// (`166-agc/patch-dma-data-src`), and a 64-bit source's high half as dw3, over a dw3 set to 1
/// (`arm1-64bit-src`, `arm2-dw3-one-src`, `20261009-104652-eboot.obs.log`).
fn dma_data_patch_source(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    patch_address(args[0], DMA_DATA_SOURCE_AT, args[1]);
    OK
}

/// Offset of a `RELEASE_MEM` packet's destination address, dw3 and dw4: radeonsi's end-of-pipe
/// write emits it there (`si_cs_emit_write_event_eop`), and `sceAgcCbReleaseMem` places its `a5`
/// in dw3 ([`packet::build::release_mem`]).
const RELEASE_MEM_ADDRESS_AT: u64 = 12;

/// `sceAgcQueueEndOfPipeActionPatchAddress(packet, address)`: writes the end-of-pipe destination
/// into the `RELEASE_MEM`'s dw3 and dw4 and answers `0x0`. Measured: `0x6000_0000` and
/// `0x8000_0000` replaced dw3 whole and nothing else changed (`166-agc/patch-queue-eop-address`,
/// sweep 20260915-203058); the high half into dw4 is assumed, where radeonsi emits it.
fn queue_eop_patch_address(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    patch_address(args[0], RELEASE_MEM_ADDRESS_AT, args[1]);
    OK
}

/// Offset of a `RELEASE_MEM` packet's immediate data, dw5 and dw6: radeonsi's end-of-pipe write
/// emits the value it stores there, after the destination address (Mesa
/// `src/amd/common/ac_cmdbuf_cp.c:494-501`).
const RELEASE_MEM_DATA_AT: u64 = 20;

/// `sceAgcQueueEndOfPipeActionPatchData(packet, data)`: writes the value the end-of-pipe action
/// stores into the `RELEASE_MEM`'s dw5 and dw6 and answers `0x0`, as the measured address patch
/// does (D886, known by assumption: where the data goes is radeonsi's layout, and the answer is the
/// address patch's).
fn queue_eop_patch_data(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    patch_address(args[0], RELEASE_MEM_DATA_AT, args[1]);
    OK
}

/// Offset in `sceAgcDcbWaitRegMem`'s compound ([`packet::build::wait_reg_mem`]) of the
/// leading `SET_UCONFIG_REG`'s second value.
const WAIT_REG_MEM_UCONFIG_VALUE_AT: u64 = 12;
/// Offset in the compound of the `WAIT_REG_MEM`'s poll address, its dw2 and dw3 as radeonsi's
/// `si_cp_wait_mem` emits them.
const WAIT_REG_MEM_POLL_AT: u64 = 24;

/// `sceAgcWaitRegMemPatchAddress(packet, address)`: writes the polled address into the compound
/// `sceAgcDcbWaitRegMem` reserved and answers `0x0`. Measured: `0x5000_0000` and `0x7000_0000`
/// came back at bytes 12 and 24 and nothing else changed (`166-agc/patch-wait-reg-mem-address`);
/// the high half into the poll address's dw3 is assumed, where radeonsi emits it.
fn wait_reg_mem_patch_address(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (packet, address) = (args[0], args[1]);
    if packet != 0 {
        // SAFETY: a dword of the compound the producer reserved and the guest passes back, in its
        // own command buffer.
        unsafe {
            guest::write_u32(
                packet.wrapping_add(WAIT_REG_MEM_UCONFIG_VALUE_AT),
                address as u32,
            )
        };
    }
    patch_address(packet, WAIT_REG_MEM_POLL_AT, address);
    OK
}

/// `sceAgcDcbWaitUntilSafeForRendering(dcb, video_handle, buffer_index)`: writes the wait for the
/// buffer's flip label to read `0` - the display has taken it off the screen - before drawing into
/// it. Measured (`-5a17`, sweep 20260927-153242): with a port open, 64 bytes written and the cursor
/// advanced 128, the packet's address answered, for indices 0, 1 and 4 alike; with no port open,
/// nothing written and `0` answered.
fn dcb_wait_until_safe_for_rendering(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dcb, handle, index) = (args[0], args[1], args[2]);
    let Ok(index32) = u32::try_from(index) else {
        return 0;
    };
    let Some(label) = crate::display::wait_label(handle, index) else {
        return 0;
    };
    dcb_append_spanning(
        dcb,
        &crate::display::wait_until_safe_words(index32, label),
        crate::display::WAIT_UNTIL_SAFE_DWORDS,
    )
}

/// The count field of a header-only `NOP`: `0x3fff`, as in `0xffff1000`, a `NOP` with no body.
const HEADER_ONLY_COUNT: u32 = 0x3fff;

/// What [`agc_packet_payload`] answers for a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Payload {
    /// The payload begins this many bytes into the packet.
    At(u64),
    /// Null: the packet has no body.
    Null,
}

/// What [`agc_packet_payload`] answers for a `kind` on a packet with `header`, or `None` for a
/// pairing nobody measured.
/// Measured (`-r1b0`, sweep 20260927-204316, `166-agc/cb-unnamed-ef57`, all answering `0`): kind 1
/// on `SET_SH_REG` 8; kind 0 on an ordinary `NOP` 4 and on the header-only `NOP` null; kind 2 on an
/// ordinary `NOP` 8.
fn payload_offset(header: u32, kind: u64) -> Option<Payload> {
    if header >> 30 != 3 {
        return None;
    }
    let opcode = ((header >> 8) & 0xff) as u8;
    let count = (header >> 16) & 0x3fff;
    match (kind, opcode) {
        (1, measured::SET_SH_REG) => Some(Payload::At(8)),
        (0, measured::NOP) if count == HEADER_ONLY_COUNT => Some(Payload::Null),
        (0, measured::NOP) => Some(Payload::At(4)),
        (2, measured::NOP) if count != HEADER_ONLY_COUNT => Some(Payload::At(8)),
        _ => None,
    }
}

/// `0x7d86501b8094ef57(out, packet, kind)`: writes where the payload of a packet the guest
/// reserved begins, or null for a `NOP` with no body, and answers `0` - see [`payload_offset`].
/// PPSA02664 fills a register write's values through the kind-1 answer and passes the kind-0
/// answer for its header-only `NOP` to `sceAgcQueueEndOfPipeActionPatchAddress`.
fn agc_packet_payload(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, packet, kind) = (args[0], args[1], args[2]);
    if out == 0 || packet == 0 {
        return BAD_ARGUMENT;
    }
    // SAFETY: `packet` is the guest's packet, whose header this reads.
    let Some(header) = (unsafe { guest::read_u32(packet) }) else {
        return BAD_ARGUMENT;
    };
    let Some(offset) = payload_offset(header, kind) else {
        return BAD_ARGUMENT;
    };
    let answer = match offset {
        Payload::At(offset) => packet.wrapping_add(offset),
        Payload::Null => 0,
    };
    // SAFETY: `out` is the guest-supplied pointer-sized out-parameter.
    unsafe { guest::write_u64(out, answer) };
    OK
}

/// The version the raw init gate accepted first, or `0` before any: the gate is the library's, one
/// per process.
static RAW_INIT_VERSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// `SCE_AGC_ERROR_INVALID_VERSION`, what either gate answers a version the library will not take.
const INVALID_VERSION: u64 = 0x8a6c_0004;

/// The versions `0x53bbd82b51d172db` was measured to accept as a process's first call: 8 (`-7e52`,
/// sweep 20260927-185507, PPSA02664's call) and 13 (sweep 20260915-125124, `(NULL, 13)` first).
const RAW_FIRST_VERSIONS: [u32; 2] = [8, 13];

/// The raw gate's decision: its answer to `version` with `fixed` accepted before (`0` for none), and
/// the version fixed after. Measured across three sweeps: the first accepted version is kept, and
/// from then on that version passes and every other answers `0x8a6c0004` (`-7e52`: 8 then 13, 12,
/// 14 refused and 8 again passing; 20260915-125124: 13 then 0-12 and 16-64 refused, 13 passing). A
/// first call with a version outside the two seen to succeed is unmeasured, and refused by name.
fn raw_init_gate(fixed: u32, version: u32) -> (u64, u32) {
    match fixed {
        0 if RAW_FIRST_VERSIONS.contains(&version) => (OK, version),
        0 => (
            u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()),
            0,
        ),
        _ if version == fixed => (OK, fixed),
        _ => (INVALID_VERSION, fixed),
    }
}

/// The versions `sceAgcInit` was measured to accept (`-7e41`, `166-agc/init-alias-7e41`).
const AGC_INIT_VERSIONS: std::ops::RangeInclusive<u32> = 12..=14;

/// The named gate's decision: `version` against the raw gate's fixed version (`0` for none). With
/// none, 12, 13 and 14 pass (`-7e41`, sweep 20260927-153242); with the raw gate fixed at another
/// version it answers `0x8a6c0004` (`-7e52`: 13 after raw 8). Anything else is unmeasured.
fn named_init_gate(fixed: u32, version: u32) -> u64 {
    if fixed != 0 && version != fixed {
        INVALID_VERSION
    } else if fixed == 0 && AGC_INIT_VERSIONS.contains(&version) {
        OK
    } else {
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    }
}

/// `sceAgcInit(state, version)`: [`named_init_gate`]; writes nothing.
fn agc_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    named_init_gate(
        RAW_INIT_VERSION.load(std::sync::atomic::Ordering::Acquire),
        args[1] as u32,
    )
}

/// `0x53bbd82b51d172db(state, version)`: a function of its own, 0x60 bytes past `sceAgcInit` in
/// the library (`-7e41`), answered by [`raw_init_gate`]; writes nothing. The C name obSCEne's
/// `166-agc/init` calls binds to this NID, so its sweeps are this function's.
fn agc_init_raw(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::Ordering;
    let version = args[1] as u32;
    let mut answer = OK;
    // The decision and the fix are one step, so two threads' first calls cannot both fix a version.
    let _ = RAW_INIT_VERSION.fetch_update(Ordering::AcqRel, Ordering::Acquire, |fixed| {
        let (rc, next) = raw_init_gate(fixed, version);
        answer = rc;
        Some(next)
    });
    answer
}

/// A register-defaults descriptor for `version`, or a refusal by name for a version no probe walked.
fn register_defaults(function: &str, version: u64) -> u64 {
    u32::try_from(version)
        .ok()
        .and_then(|version| crate::register_defaults::descriptor(function, version))
        .unwrap_or_else(|| u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()))
}

/// `sceAgcGetRegisterDefaults2(version)`: the library-owned descriptor for `version`, as measured
/// (`-4d8a`, `-4e9b`).
fn defaults2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    register_defaults("sceAgcGetRegisterDefaults2", args[0])
}

/// `sceAgcGetRegisterDefaults2Internal(version)`: the inner variant's descriptor, as measured.
fn defaults2_internal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    register_defaults("sceAgcGetRegisterDefaults2Internal", args[0])
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

/// The longest `NOP` a PM4 header describes: a count field of 14 bits, plus the header and one.
const NOP_MAX_DWORDS: u64 = 0x3fff + 1;

/// `sceAgcCbNop(cb, dwords)` - a `NOP` `dwords` long, its body reserved and left unwritten.
///
/// Measured (`166-agc/cb-nop`): length 1 writes the header-only word `0xffff1000` and answers the
/// packet's address; length 0 writes nothing, and its answer was not reported, so it is refused.
/// A longer one follows the public PM4 header, whose count is the length less two - the rule
/// length 1 is the wrapped case of (`0x3fff`) - with its body skipped, as the measured `NOP` bodies
/// of `sceAgcDcbSetFlip` and `sceAgcDcbWaitUntilSafeForRendering` are. PPSA02664 asks for 3 and
/// writes the two body dwords itself.
fn cb_nop(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let dwords = args[1];
    match dwords {
        1 => dcb_append(args[0], &packet::build::nop()),
        2..=NOP_MAX_DWORDS => dcb_append_spanning(
            args[0],
            // Its body is every dword after the header.
            &[packet::build::command_header(
                measured::NOP,
                (dwords - 1) as u32,
            )],
            dwords as usize,
        ),
        _ => BAD_ARGUMENT,
    }
}

/// `sceAgcDcbAcquireMem(dcb, ...)` - reserves the 32-byte ACQUIRE_MEM packet, cursor real, body
/// zero.
///
/// Header and extent are measured (`166-agc/dcb-acquire-mem`); the argument-to-body mapping is an
/// unpinned permutation, so the body is zero (D696).
fn dcb_acquire_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::acquire_mem_skeleton())
}

/// The eleven arguments after a builder's command buffer: five in registers, the other six from
/// the call's stack area, zero where the dispatch published none (a direct call, as in a test).
fn builder_arguments(args: &[u64; GUEST_ARG_REGISTERS]) -> [u64; 11] {
    let mut all = [0_u64; 11];
    all[..5].copy_from_slice(&args[1..6]);
    let spilled = orbistoun_thunk::stack_arguments();
    if spilled != 0 {
        for (index, slot) in all[5..].iter_mut().enumerate() {
            // SAFETY: the dispatch published this as the words above the return address the
            // guest's call pushed, on the calling thread's live stack; the read is checked.
            *slot = unsafe { guest::read_u64(spilled + index as u64 * 8) }.unwrap_or_default();
        }
    }
    all
}

/// `sceAgcCbReleaseMem(cb, a1, .., a11)`: the `RELEASE_MEM` packet, its fields placed as measured
/// one argument at a time (obSCEne `-1044`, [`packet::build::release_mem`]).
fn cb_release_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::release_mem(&builder_arguments(args)),
    )
}

/// `sceAgcDcbDmaData(dcb, a1, .., a11)`: the `DMA_DATA` packet, its fields placed as measured one
/// argument at a time (obSCEne `-1044`, [`packet::build::dma_data`]).
fn dcb_dma_data(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &packet::build::dma_data(&builder_arguments(args)))
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

/// `sceAgcCbDispatch(cb, x, y, z, ..)` - a compute dispatch of `x` by `y` by `z` thread groups:
/// header `0xc0031500`, the three counts, then the initiator `0x41` whatever the fourth and fifth
/// arguments (obSCEne `reports/report-1791286625.txt` 12105-12185, REQ cd15).
fn cb_dispatch(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::agc_dispatch_direct(args[1] as u32, args[2] as u32, args[3] as u32),
    )
}

/// `sceAgcDcbJump(dcb, chain, policy, target, dwords)`: an `INDIRECT_BUFFER` to `target`,
/// `dwords` long, as the console writes it (obSCEne `report-20261018-gt-census.log` 2843-2851):
/// header `0xc0023f00`, the target's halves, then the size, `chain` in bit 20, `policy` in bits
/// 29:28, and `0x0f20_0000` set besides. A jump made before its target is known is written with a
/// zero target and size, and filled in before the `REWIND` ahead of it is released.
fn dcb_jump(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let target = args[3];
    let control = (args[4] as u32 & 0xF_FFFF)
        | (args[1] as u32 & 1) << 20
        | (args[2] as u32 & 3) << 28
        | 0x0f20_0000;
    dcb_append(
        args[0],
        &[
            packet::build::command_header(0x3F, 3),
            target as u32,
            (target >> 32) as u32,
            control,
        ],
    )
}

/// `sceAgcDcbRewind(dcb, ..)`: a `REWIND` not yet released, `0xc0005900 0x00000000`, as the console
/// writes it with its arguments zero (obSCEne `report-20261018-gt-census.log` 2864-2866); it
/// answers the packet's address, which the title hands to
/// [`rewind_patch_set_rewind_state`]. PPSA04263 makes it with its second argument zero; the
/// census's sentinel arm sets the valid bit, through which argument is not measured.
fn dcb_rewind(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(args[0], &[packet::build::command_header(0x59, 1), 0])
}

/// `sceAgcRewindPatchSetRewindState(rewind, state)`: sets the valid bit of the `REWIND` at `rewind`
/// to `state`, as PPSA04263 calls it with `state` 1 once it has filled in the jump after it. A
/// `REWIND` released carries out what follows it ([`crate::agc_driver::release_rewind`]).
/// Answers 0; the library could not be bound to measure it (census `bound 0`).
fn rewind_patch_set_rewind_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (rewind, released) = (args[0], args[1] & 1 != 0);
    let body = rewind.wrapping_add(4);
    // SAFETY: the address the title's own REWIND builder answered, its body dword four bytes in.
    let Some(word) = (unsafe { guest::read_u32(body) }) else {
        return BAD_ARGUMENT;
    };
    let word = if released {
        word | crate::cp::REWIND_RELEASED
    } else {
        word & !crate::cp::REWIND_RELEASED
    };
    // SAFETY: as above; the dword just read.
    if !unsafe { guest::write_u32(body, word) } {
        return BAD_ARGUMENT;
    }
    if released {
        crate::agc_driver::release_rewind(rewind.wrapping_add(8));
    }
    0
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

/// `sceAgcDcbSetShRegistersIndirect(dcb, table, count)`. Header `0xc0036300`, 20 bytes, the body
/// as the Cx producer fills its own.
fn dcb_set_sh_registers_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_registers_indirect(measured::SET_SH_REG_INDIRECT, args[1], args[2]),
    )
}

/// `sceAgcDcbSetUcRegistersIndirect(dcb, table, count)`. Header `0xc0036400`, 20 bytes, the body
/// as the Cx producer fills its own.
fn dcb_set_uc_registers_indirect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_registers_indirect(
            measured::SET_UCONFIG_REG_INDIRECT,
            args[1],
            args[2],
        ),
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

/// `sceAgcDcbWaitRegMem(dcb, a1, .., a11)` - wait on a memory word: the measured compound of
/// three packets, its fields placed as measured one argument at a time (obSCEne REQ wr3a,
/// [`packet::build::wait_reg_mem`]).
fn dcb_wait_reg_mem(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::wait_reg_mem(&builder_arguments(args)),
    )
}

/// The largest register run this will read out of guest memory in one call.
///
/// A packet's count field is fourteen bits, so a longer run cannot be encoded, and a wild count
/// is refused instead of becoming a wild read.
const MAX_REGISTER_RUN: u64 = 0x3ffe;

/// `sceAgcCbSetShRegistersDirect(cb, pairs, count)`: each `{offset, value}` pair at `pairs` as its
/// own `SET_SH_REG` - the GFX10 form of a scattered SH register write (Mesa `radeon_set_sh_reg`; the
/// pair packet is GFX11's) - answering the first packet's address. A count of 0 writes nothing and
/// answers 0, as measured (`166-agc/cb-set-sh-registers-direct`); the encoding for a count above
/// 0 is assumed. A table that is not readable guest memory is refused.
fn cb_set_sh_registers_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (pairs, count) = (args[1], args[2]);
    if count == 0 {
        return 0;
    }
    if pairs == 0 || count > MAX_REGISTER_RUN {
        return BAD_ARGUMENT;
    }
    let mut words = Vec::with_capacity(count as usize * 3);
    for at in (0..count).map(|i| pairs + i * 8) {
        // SAFETY: the guest's pair table, read through the checked accessor.
        let (Some(offset), Some(value)) = (unsafe { guest::read_u32(at) }, unsafe {
            guest::read_u32(at + 4)
        }) else {
            return BAD_ARGUMENT;
        };
        words.extend(packet::build::set_sh_register_range(
            offset as u16,
            &[value],
        ));
    }
    dcb_append(args[0], &words)
}

/// `sceAgcCbSetShRegisterRangeDirect(cb, offset, values, count)`. Measured
/// (`166-agc/dcb-set-sh-reg-direct`): a two-register run comes back as `header, offset, value,
/// value`, with no marker, `n + 2` dwords.
///
/// With no `values` the packet is reserved with its payload zeroed and its address returned: a
/// title passes null and then fills the run itself through the payload pointer
/// [`agc_packet_payload`] gives it (guest-observed in PPSA02664).
fn cb_set_sh_register_range_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, values, count) = (args[1], args[2], args[3]);
    if count == 0 || count > MAX_REGISTER_RUN {
        return BAD_ARGUMENT;
    }
    let mut run = Vec::with_capacity(count as usize);
    if values == 0 {
        run.resize(count as usize, 0);
        return dcb_append(
            args[0],
            &packet::build::set_sh_register_range(offset as u16, &run),
        );
    }
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

/// `sceAgcDcbDrawIndexOffset(dcb, index_offset, index_count, modifier)`: a `DRAW_INDEX_OFFSET_2`
/// (Mesa `sid.h` `0x35`) from the bound index buffer, body `[max_size, index_offset, index_count,
/// initiator]`, in the 20 bytes the builder was measured to take (`166-agc/*-getsize`). The index
/// count goes in `max_size` and the modifier in the initiator word, as `sceAgcDcbDrawIndex` was
/// measured to place them; the body is assumed.
fn dcb_draw_index_offset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, count, modifier) = (args[1] as u32, args[2] as u32, args[3] as u32);
    dcb_append(
        args[0],
        &[
            packet::build::command_header(DRAW_INDEX_OFFSET_2, 4),
            count,
            offset,
            count,
            modifier,
        ],
    )
}

/// `DRAW_INDEX_OFFSET_2`'s opcode (Mesa `sid.h`).
const DRAW_INDEX_OFFSET_2: u8 = 0x35;

/// `sceAgcDcbSetIndexSize(dcb, type, flags)`, measured across eight argument pairs; see
/// [`packet::build::set_index_size`] for the mapping.
fn dcb_set_index_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    dcb_append(
        args[0],
        &packet::build::set_index_size(args[1] as u32, args[2] as u32),
    )
}

/// `0x6dd715e95973a4fc(cb, flags, mode, 0)`: the index-type `SET_UCONFIG_REG_INDEX`, the packet
/// [`dcb_set_index_size`] writes, answering its address. Measured on the full 0x40-byte writer
/// (`166-agc/builder-6dd715e95973a4fc`, `20261010-115300-eboot.log` 14205-14235): `(0, 0)` writes
/// `0x400`, `(1, 1)` `0x441` and `(0, 2)` `0x480` - flags at bit 6 as `sceAgcDcbSetIndexSize`'s,
/// mode 1 at bit 0 and mode 2 at bit 7. Any other pair, and a non-zero fourth argument, are
/// unmeasured and refused. Out of space it calls the writer's callback; that path is the
/// placeholder here, as for every builder.
fn index_type_6dd715e9(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// The mode bits each measured mode wrote.
    const MODE_BITS: [u32; 3] = [0, 0x1, 0x80];
    let (flags, mode) = (args[1], args[2]);
    let mode_bits = usize::try_from(mode)
        .ok()
        .and_then(|m| MODE_BITS.get(m).copied());
    let (Some(mode_bits), 0..=1, 0) = (mode_bits, flags, args[3] as u32) else {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    };
    let mut words = packet::build::set_index_size(0, flags as u32);
    words[2] |= mode_bits;
    dcb_append(args[0], &words)
}

/// `WRITE_DATA`'s destination field for each measured destination argument: 4 wrote `0x200` and 1
/// wrote `0x40000000` (`166-agc/dcb-write-data`). Any other is unmeasured.
fn write_data_destination(destination: u64) -> Option<u32> {
    match destination {
        4 => Some(0x0000_0200),
        1 => Some(0x4000_0000),
        _ => None,
    }
}

/// The `WRITE_DATA` `sceAgcDcbWriteData(dcb, destination, cache_policy, address, data, count,
/// stack0, confirm)` writes: header (body = 3 + count), control, the address's halves, then the
/// data. The control word is the destination field ([`write_data_destination`]),
/// `cache_policy << 25` (Mesa `sid.h` `WRITE_DATA_CACHE_POLICY`) and `confirm` at bit 20
/// (`WRITE_DATA_WR_CONFIRM`), as the four measured calls wrote. `None` for an unmeasured
/// destination, empty data, a first stack argument other than the 0 every call passed, or a
/// `confirm` other than 0 or 1.
fn write_data_packet(
    destination: u64,
    cache_policy: u64,
    address: u64,
    data: &[u32],
    stack0: u64,
    confirm: u64,
) -> Option<Vec<u32>> {
    /// `WRITE_DATA`'s opcode (Mesa `sid.h` `PKT3_WRITE_DATA`).
    const WRITE_DATA: u8 = 0x37;
    if data.is_empty() || stack0 != 0 || confirm > 1 || cache_policy > 3 {
        return None;
    }
    let control =
        write_data_destination(destination)? | (cache_policy as u32) << 25 | (confirm as u32) << 20;
    let body = u32::try_from(data.len() + 3).ok()?;
    let mut words = Vec::with_capacity(data.len() + 4);
    words.push(packet::build::command_header(WRITE_DATA, body));
    words.push(control);
    words.push(address as u32);
    words.push((address >> 32) as u32);
    words.extend_from_slice(data);
    Some(words)
}

/// The longest `WRITE_DATA` written: the counts called range to 0x20 dwords in PPSA25872's text.
const MOST_WRITE_DATA_DWORDS: u64 = 0x100;

/// `sceAgcDcbWriteData(dcb, destination, cache_policy, address, data, count, stack0, confirm)`:
/// appends [`write_data_packet`] and answers its address. Its last two arguments are on the stack
/// (every call site in PPSA25872 pushes them); a call reaching here without them is refused.
fn dcb_write_data(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let refused = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    let (data, count) = (args[4], args[5] as u32);
    let spilled = orbistoun_thunk::stack_arguments();
    if spilled == 0 || data == 0 || count == 0 || u64::from(count) > MOST_WRITE_DATA_DWORDS {
        return refused;
    }
    // SAFETY: the dispatch published `spilled` as the first stack argument's address on the
    // calling thread's live stack; the second is the word above it.
    let (Some(stack0), Some(confirm)) = (unsafe { guest::read_u64(spilled) }, unsafe {
        guest::read_u64(spilled.wrapping_add(8))
    }) else {
        return refused;
    };
    let mut words = Vec::with_capacity(count as usize);
    for i in 0..u64::from(count) {
        // SAFETY: the guest's data, `count` dwords by the call's contract.
        let Some(word) = (unsafe { guest::read_u32(data.wrapping_add(i * 4)) }) else {
            return refused;
        };
        words.push(word);
    }
    match write_data_packet(args[1], args[2], args[3], &words, stack0, confirm) {
        Some(packet) => dcb_append(args[0], &packet),
        None => refused,
    }
}

/// What `sceAgcWriteDataPatchSetAddressOrOffset` answered for a pointer that is not a packet
/// (`166-agc/dcb-write-data`, the swapped call).
const NOT_A_PACKET: u64 = 0x8a6c_000c;

/// `sceAgcWriteDataPatchSetAddressOrOffset(packet, address)`: writes `address` into a
/// `WRITE_DATA`'s destination dwords and answers 0, as measured. The measured refusal,
/// `0x8a6c000c`, came from a call handed a heap word for the packet; refusing whatever does not
/// begin with a `WRITE_DATA` header is the assumed rule that reproduces it.
fn write_data_patch_set_address(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// `WRITE_DATA`'s opcode, at bits 8-15 of a type-3 header.
    const WRITE_DATA: u32 = 0x37;
    let (packet, address) = (args[0], args[1]);
    if packet == 0 {
        return NOT_A_PACKET;
    }
    // SAFETY: the guest's packet, whose header this reads.
    let Some(header) = (unsafe { guest::read_u32(packet) }) else {
        return NOT_A_PACKET;
    };
    if header >> 30 != 3 || (header >> 8) & 0xff != WRITE_DATA {
        return NOT_A_PACKET;
    }
    // SAFETY: a `WRITE_DATA`'s destination dwords, the second and third after its header.
    unsafe {
        guest::write_u32(packet.wrapping_add(8), address as u32);
    }
    // SAFETY: as above, the high half.
    unsafe {
        guest::write_u32(packet.wrapping_add(12), (address >> 32) as u32);
    }
    OK
}

/// Implementations this crate provides for `libSceAgc`.
///
/// `sceAgcCreateShader` and the shader-linkage calls (interpolant mapping, primitive state, shader
/// linker) implement measured behaviour. The command builders are wired through the writer handle
/// in `arg0` (the private `dcb` module) as pure encoders or reservation skeletons from
/// [`crate::packet::build`]; the flip builders' words come from [`crate::display`]. The patch
/// family answers its measured `0x0` and writes nothing. This array is the authoritative list; its size is
/// pinned by `tests/dcb_wiring.rs`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    IMPLEMENTATIONS
}

/// A packet-size query answering a constant: the bytes its builder's `GetSize` answered with its
/// arguments zero, in the `166-agc` check named beside it
/// (`reports/hardware/20261009-151440-eboot.obs.log`). Each builder emits a packet of fixed length,
/// and the builder advanced the writer by exactly that answer where it was measured.
macro_rules! measured_size {
    ($($name:ident = $bytes:literal, $check:literal;)*) => {
        $(
            #[doc = concat!("Answers `", stringify!($bytes), "` bytes (`166-agc/", $check, "`).")]
            fn $name(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
                $bytes
            }
        )*
    };
}

measured_size! {
    acb_acquire_mem_get_size = 0x20, "acb-acquire-mem-getsize";
    dcb_acquire_mem_get_size = 0x20, "dcb-acquire-mem-getsize";
    acb_dma_data_get_size = 0x1c, "acb-dma-data-getsize";
    dcb_dma_data_get_size = 0x1c, "dcb-dma-data-getsize";
    acb_dispatch_indirect_get_size = 0x10, "acb-dispatch-indirect";
    dcb_dispatch_indirect_get_size = 0xc, "dcb-dispatch-indirect";
    acb_event_write_get_size = 0x8, "acb-event-write";
    acb_jump_get_size = 0x10, "acb-jump-getsize";
    dcb_jump_get_size = 0x10, "dcb-jump-getsize";
    cb_branch_get_size = 0x38, "cb-branch-getsize";
    cb_queue_end_of_pipe_action_get_size = 0x20, "cb-queue-eop-action-getsize";
    dcb_cond_exec_get_size = 0x14, "dcb-cond-exec";
    dcb_draw_index_indirect_get_size = 0x14, "dcb-draw-index-indirect-getsize";
    dcb_draw_index_indirect_multi_get_size = 0x40, "dcb-draw-index-indirect-multi-getsize";
    dcb_draw_index_offset_get_size = 0x14, "dcb-draw-index-offset";
    dcb_draw_indirect_get_size = 0x14, "dcb-draw-indirect";
    dcb_get_lod_stats_get_size = 0x14, "dcb-get-lod-stats-getsize";
    dcb_rewind_get_size = 0x8, "dcb-rewind-getsize";
    dcb_set_cx_registers_indirect_get_size = 0x14, "dcb-set-cx-registers-indirect";
    dcb_set_sh_registers_indirect_get_size = 0x14, "dcb-set-sh-registers-indirect";
    dcb_set_uc_registers_indirect_get_size = 0x14, "dcb-set-uc-registers-indirect";
    dcb_set_uc_register_direct_get_size = 0xc, "dcb-set-uc-register-direct-getsize";
    dcb_stall_command_buffer_parser_get_size = 0x8, "dcb-stall-cb-parser-getsize";
    dcb_set_index_count_get_size = 0x8, "dcb-set-index-count-getsize";
}

/// `sceAgcCbNopGetSize(n)`: four bytes for each of the `n` dwords the NOP holds - 0, 4, 8, 16 and
/// 32 for `n` 0, 1, 2, 4 and 8 (`166-agc/cb-nop-getsize`).
fn cb_nop_get_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    args[0].wrapping_mul(4)
}

/// The list [`implementations`] answers.
const IMPLEMENTATIONS: &[(&str, GuestFn)] = &[
    ("0x7d86501b8094ef57", agc_packet_payload),
    ("0x6dd715e95973a4fc", index_type_6dd715e9),
    ("0x0a4e469ba741de7d", crate::agc_merge::merge_shaders),
    ("0xe486fe61994e8976", crate::agc_merge::merged_sizes),
    ("sceAgcDcbWriteData", dcb_write_data),
    (
        "sceAgcWriteDataPatchSetAddressOrOffset",
        write_data_patch_set_address,
    ),
    ("0x71040c4df8235e1d", interpolants_71040c4d),
    ("0x8a6f69da59a5b375", interpolants_8a6f69da),
    ("sceAgcAcbAcquireMemGetSize", acb_acquire_mem_get_size),
    ("sceAgcDcbAcquireMemGetSize", dcb_acquire_mem_get_size),
    ("sceAgcAcbDmaDataGetSize", acb_dma_data_get_size),
    ("sceAgcDcbDmaDataGetSize", dcb_dma_data_get_size),
    (
        "sceAgcAcbDispatchIndirectGetSize",
        acb_dispatch_indirect_get_size,
    ),
    (
        "sceAgcDcbDispatchIndirectGetSize",
        dcb_dispatch_indirect_get_size,
    ),
    ("sceAgcAcbEventWriteGetSize", acb_event_write_get_size),
    ("sceAgcAcbJumpGetSize", acb_jump_get_size),
    ("sceAgcDcbJumpGetSize", dcb_jump_get_size),
    ("sceAgcDcbJump", dcb_jump),
    ("sceAgcDcbRewind", dcb_rewind),
    (
        "sceAgcRewindPatchSetRewindState",
        rewind_patch_set_rewind_state,
    ),
    ("sceAgcCbBranchGetSize", cb_branch_get_size),
    (
        "sceAgcCbQueueEndOfPipeActionGetSize",
        cb_queue_end_of_pipe_action_get_size,
    ),
    ("sceAgcDcbCondExecGetSize", dcb_cond_exec_get_size),
    (
        "sceAgcDcbDrawIndexIndirectGetSize",
        dcb_draw_index_indirect_get_size,
    ),
    (
        "sceAgcDcbDrawIndexIndirectMultiGetSize",
        dcb_draw_index_indirect_multi_get_size,
    ),
    (
        "sceAgcDcbDrawIndexOffsetGetSize",
        dcb_draw_index_offset_get_size,
    ),
    ("sceAgcDcbDrawIndirectGetSize", dcb_draw_indirect_get_size),
    ("sceAgcDcbGetLodStatsGetSize", dcb_get_lod_stats_get_size),
    ("sceAgcDcbRewindGetSize", dcb_rewind_get_size),
    (
        "sceAgcDcbSetCxRegistersIndirectGetSize",
        dcb_set_cx_registers_indirect_get_size,
    ),
    (
        "sceAgcDcbSetShRegistersIndirectGetSize",
        dcb_set_sh_registers_indirect_get_size,
    ),
    (
        "sceAgcDcbSetUcRegistersIndirectGetSize",
        dcb_set_uc_registers_indirect_get_size,
    ),
    (
        "sceAgcDcbSetUcRegisterDirectGetSize",
        dcb_set_uc_register_direct_get_size,
    ),
    (
        "sceAgcDcbStallCommandBufferParserGetSize",
        dcb_stall_command_buffer_parser_get_size,
    ),
    (
        "sceAgcDcbSetIndexCountGetSize",
        dcb_set_index_count_get_size,
    ),
    ("sceAgcCbNopGetSize", cb_nop_get_size),
    ("sceAgcCreateShader", create_shader),
    ("sceAgcSuspendPoint", suspend_point),
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
    ("sceAgcDcbSetShRegisterDirect", dcb_set_sh_register_direct),
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
    (
        "sceAgcDcbWaitUntilSafeForRendering",
        dcb_wait_until_safe_for_rendering,
    ),
    ("sceAgcDcbSetFlip", dcb_set_flip),
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
        indirect_patch_add_registers,
    ),
    (
        "sceAgcSetCxRegIndirectPatchSetAddress",
        indirect_patch_set_address,
    ),
    (
        "sceAgcSetShRegIndirectPatchAddRegisters",
        indirect_patch_add_registers,
    ),
    (
        "sceAgcSetShRegIndirectPatchSetAddress",
        indirect_patch_set_address,
    ),
    (
        "sceAgcSetUcRegIndirectPatchAddRegisters",
        indirect_patch_add_registers,
    ),
    (
        "sceAgcSetUcRegIndirectPatchSetAddress",
        indirect_patch_set_address,
    ),
    (
        "sceAgcDmaDataPatchSetDstAddressOrOffset",
        dma_data_patch_destination,
    ),
    (
        "sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate",
        dma_data_patch_source,
    ),
    ("sceAgcWaitRegMemPatchAddress", wait_reg_mem_patch_address),
    (
        "sceAgcQueueEndOfPipeActionPatchAddress",
        queue_eop_patch_address,
    ),
    ("sceAgcQueueEndOfPipeActionPatchData", queue_eop_patch_data),
    ("sceAgcCbSetShRegistersDirect", cb_set_sh_registers_direct),
    (
        "sceAgcCbSetShRegisterRangeDirect",
        cb_set_sh_register_range_direct,
    ),
    ("sceAgcDcbDrawIndex", dcb_draw_index),
    ("sceAgcDcbDrawIndexOffset", dcb_draw_index_offset),
    ("sceAgcDcbSetIndexSize", dcb_set_index_size),
    ("0x53bbd82b51d172db", agc_init_raw),
    ("sceAgcInit", agc_init),
    ("sceAgcGetIsTrinityMode", get_is_trinity_mode),
    ("sceAgcGetRegisterDefaults2", defaults2),
    ("sceAgcGetRegisterDefaults2Internal", defaults2_internal),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The interpolant table routes each pixel input to the vertex output of its semantic, as the
    /// console wrote it for obSCEne's shader pairs (REQ-im01, `166-agc/interpolant-mapping`): a
    /// permuted pair routes input 0 (semantic 2) to slot 2 and input 1 (semantic 0) to slot 0; a flat
    /// input adds `0x400` - `0x420` from `sceAgcCreateInterpolantMapping` - and an input no output
    /// provides is `0x20`; entries past the inputs keep their own index.
    #[test]
    fn inputs_are_routed_by_semantic_as_measured() {
        let outputs = [0x0004_0000, 0x0004_0101, 0x0004_0202];
        let permuted = route_interpolants(&[0x0004_0002, 0x0004_0100], &outputs, FLAT_0X7104);
        assert_eq!(permuted[..4], [2, 0, 2, 3]);
        assert_eq!(permuted[31], 31);
        let flat = [0x0004_0002, 0x0044_0100, 0x0004_0263];
        assert_eq!(
            route_interpolants(&flat, &outputs, FLAT_0X7104)[..3],
            [2, 0x400, 0x20]
        );
        assert_eq!(
            route_interpolants(&flat, &outputs, FLAT_CREATE)[..3],
            [2, 0x420, 0x20]
        );
        assert_eq!(
            route_interpolants(&[], &outputs, FLAT_CREATE)[..3],
            [0, 1, 2]
        );
    }

    /// A shader pair handed to an interpolant builder is kept for the report the first time only,
    /// a few pairs at most: GTA V hands its builder one per draw, and what is wanted is the headers'
    /// bytes, from which the remap by semantics the table leaves out can be read.
    #[test]
    fn a_shader_pair_is_kept_once_while_there_is_room() {
        let mut seen = Vec::new();
        assert!(first_sight(&mut seen, (0x1000, 0x2000), 2));
        assert!(!first_sight(&mut seen, (0x1000, 0x2000), 2), "seen before");
        assert!(first_sight(&mut seen, (0x1000, 0x3000), 2));
        assert!(!first_sight(&mut seen, (0x4000, 0x5000), 2), "no room");
    }

    /// A header is set out sixteen bytes a line, each with its offset.
    #[test]
    fn a_header_is_set_out_with_offsets() {
        let bytes: Vec<u8> = (0..20).collect();
        assert_eq!(
            hex_lines(&bytes),
            "  +0x000  00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f\n  +0x010  10 11 12 13\n"
        );
    }

    /// `sceAgcDcbWriteData`'s packet for the four measured calls (`166-agc/dcb-write-data`,
    /// `20261010-115300-eboot.log` 13916-13946): the header counts the body, arg 2 picks the
    /// destination word, arg 3 is Mesa's `CACHE_POLICY` at bit 25 and the second stack argument
    /// `WR_CONFIRM` at bit 20.
    #[test]
    fn write_data_packets_are_built_as_measured() {
        let at = 0x0000_0007_eeff_b874;
        assert_eq!(
            write_data_packet(4, 0, 0, &[0x1122_3344], 0, 1),
            Some(vec![0xc003_3700, 0x0010_0200, 0, 0, 0x1122_3344])
        );
        assert_eq!(
            write_data_packet(4, 0, 0, &[0x1122_3344], 0, 0),
            Some(vec![0xc003_3700, 0x0000_0200, 0, 0, 0x1122_3344])
        );
        assert_eq!(
            write_data_packet(4, 2, at, &[0x1122_3344], 0, 1),
            Some(vec![0xc003_3700, 0x0410_0200, 0xeeff_b874, 7, 0x1122_3344])
        );
        assert_eq!(
            write_data_packet(1, 3, at, &[0x5566_7788, 0x99aa_bbcc], 0, 0),
            Some(vec![
                0xc004_3700,
                0x4600_0000,
                0xeeff_b874,
                7,
                0x5566_7788,
                0x99aa_bbcc
            ])
        );
        assert_eq!(write_data_packet(2, 0, 0, &[1], 0, 0), None, "unmeasured");
        assert_eq!(write_data_packet(4, 0, 0, &[], 0, 0), None, "no data");
        assert_eq!(write_data_packet(4, 0, 0, &[1], 1, 0), None, "unmeasured");
    }

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

    /// The two unnamed builders a title hands `(record, vs, ps, ...)` write the same default table
    /// over whatever the record held: the census rows show `(0x191 + i, i)` for the first eight
    /// entries over a record seeded with prim-state words (`0x71040c4df8235e1d`) and over one filled
    /// with `0xcc` (`0x8a6f69da59a5b375`), and the table is the 32 entries
    /// `sceAgcCreateInterpolantMapping` writes.
    #[test]
    fn the_unnamed_builders_write_the_default_interpolant_table() {
        for builder in [interpolants_71040c4d, interpolants_8a6f69da] {
            let mut record = [0xccu8; 0x110];
            let mut args = [0u64; GUEST_ARG_REGISTERS];
            args[0] = record.as_mut_ptr() as u64;
            assert_eq!(builder(&args), OK);
            for i in 0..32u64 {
                assert_eq!(
                    read_le_u64(&record, (i * 8) as usize),
                    (i << 32) | (0x191 + i)
                );
            }
            assert!(
                record[0x100..].iter().all(|&byte| byte == 0xcc),
                "past the table"
            );
            assert_eq!(builder(&[0; GUEST_ARG_REGISTERS]), BAD_ARGUMENT);
        }
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

    /// The create writes both register lists whole and nothing past them (D781), as
    /// `166-agc/create-prim-state` measured over buffers filled with `0xa5`: `(0, 0)`, `(0x262, 0)`
    /// and `(0x242 VGT_PRIMITIVE_TYPE, topology)` in `sec_state`, and `(0, 0)` then
    /// `(0x29b VGT_GS_OUT_PRIM_TYPE, 2)` - triangles - in `prim_state`.
    #[test]
    fn prim_state_lists_hold_only_measured_pairs() {
        let mut prim = [0xa5u8; 0x20];
        let mut sec = [0xa5u8; 0x110];
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = prim.as_mut_ptr() as u64;
        args[1] = sec.as_mut_ptr() as u64;
        args[4] = 4; // DI_PT_TRILIST

        assert_eq!(create_prim_state(&args), OK);
        let pairs = |bytes: &[u8], count: usize| -> Vec<(u32, u32)> {
            bytes[..count * 8]
                .chunks(8)
                .map(|pair| {
                    (
                        u32::from_le_bytes(pair[..4].try_into().unwrap()),
                        u32::from_le_bytes(pair[4..].try_into().unwrap()),
                    )
                })
                .collect()
        };
        assert_eq!(pairs(&sec, 3), [(0, 0), (0x262, 0), (0x242, 4)]);
        assert_eq!(pairs(&prim, 2), [(0, 0), (0x29b, 2)]);
        assert!(
            sec[0x18..].iter().all(|&byte| byte == 0xa5),
            "past sec_state's list"
        );
        assert!(
            prim[0x10..].iter().all(|&byte| byte == 0xa5),
            "past prim_state's list"
        );
    }

    /// An update writes the output primitive at `prim_state + 0xc` and the topology in
    /// `sec_state`, from a state made without a geometry shader and from one made with one, whose
    /// `prim_state` the create leaves zero (`REQ-20261013T1900Z-ps04`, `166-agc/update-prim-state`).
    #[test]
    fn an_update_moves_the_output_primitive_as_measured() {
        let vs = [0u8; 0x40];
        for geometry in [0, vs.as_ptr() as u64] {
            let mut prim = [0u8; 0x10];
            let mut sec = [0u8; 0x18];
            let mut args = [0u64; GUEST_ARG_REGISTERS];
            args[0] = prim.as_mut_ptr() as u64;
            args[1] = sec.as_mut_ptr() as u64;
            args[2] = geometry;
            args[3] = vs.as_ptr() as u64;
            args[4] = 4;
            assert_eq!(create_prim_state(&args), OK);
            let named = if geometry == 0 { 0x29b_u32 } else { 0 };
            let expect_create = if geometry == 0 { 2_u32 } else { 0 };
            assert_eq!(prim[8..12], named.to_le_bytes(), "create, gs {geometry:#x}");
            assert_eq!(prim[12..16], expect_create.to_le_bytes());
            for (topology, out) in [(1, 0), (2, 1), (5, 2), (7, 3), (17, 4), (4, 2)] {
                let update = [
                    prim.as_mut_ptr() as u64,
                    sec.as_mut_ptr() as u64,
                    topology,
                    0,
                    0,
                    0,
                ];
                assert_eq!(update_prim_state(&update), OK);
                assert_eq!(prim[..8], [0; 8]);
                assert_eq!(prim[8..12], named.to_le_bytes(), "{topology}");
                assert_eq!(
                    prim[12..16],
                    u32::try_from(out).unwrap().to_le_bytes(),
                    "{topology}"
                );
                assert_eq!(sec[0x10..0x14], 0x242_u32.to_le_bytes());
                assert_eq!(
                    sec[0x14..0x18],
                    u32::try_from(topology).unwrap().to_le_bytes()
                );
            }
        }
    }

    /// The DMA patches write a 64-bit address's halves into their two dwords, the high half over
    /// whatever was there, as `arm2-dw5-one-dst` and `arm2-dw3-one-src` measured with it set to 1:
    /// `0x8_8001_0c00` becomes dw4 `0x8001_0c00` and dw5 `0x8`, or dw2 and dw3.
    #[test]
    fn a_dma_patch_writes_both_halves_over_what_was_there() {
        for (patch, low, high) in [
            (
                dma_data_patch_destination as fn(&[u64; GUEST_ARG_REGISTERS]) -> u64,
                4,
                5,
            ),
            (dma_data_patch_source, 2, 3),
        ] {
            let mut packet = [0xc005_5000u32, 0x10_0000, 0, 0, 0, 0, 0];
            packet[high] = 1;
            let mut args = [0u64; GUEST_ARG_REGISTERS];
            args[0] = packet.as_mut_ptr() as u64;
            args[1] = 0x8_8001_0c00;
            assert_eq!(patch(&args), OK);
            assert_eq!((packet[low], packet[high]), (0x8001_0c00, 0x8));
        }
    }

    /// Primitive topology lands in the low five bits of `sec_state + 0x14`: the create writes
    /// `DI_PT_TRILIST` (4), and the update `DI_PT_POINTLIST` (1) with the surrounding bits preserved.
    #[test]
    fn prim_state_records_topology_in_sec_state() {
        let mut prim = [0u8; 64];
        let mut sec = [0u8; 0x100];
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        args[0] = prim.as_mut_ptr() as u64;
        args[1] = sec.as_mut_ptr() as u64;
        args[4] = 4; // DI_PT_TRILIST

        assert_eq!(create_prim_state(&args), OK);
        let after_create = u32::from_le_bytes(sec[0x14..0x18].try_into().unwrap());
        assert_eq!(after_create, 4, "topology is DI_PT_TRILIST");
        // Poison the high bits of the topology dword to prove read-modify-write preserves them.
        sec[0x14..0x18].copy_from_slice(&(0xabcd_ffe0u32 | 4).to_le_bytes());

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

    /// The payload decision on each measured header (`-r1b0`): a register write's values two dwords
    /// in, a `NOP`'s body one dword in or null when it has none, kind 2 two dwords into a `NOP`, and
    /// nothing for a pairing no arm covered. `tests/dcb_wiring.rs` runs the handler on real packets.
    #[test]
    fn the_payload_offsets_are_the_measured_ones() {
        assert_eq!(payload_offset(0xc003_7600, 1), Some(Payload::At(8)));
        assert_eq!(payload_offset(0xc002_1000, 0), Some(Payload::At(4)));
        assert_eq!(payload_offset(0xffff_1000, 0), Some(Payload::Null));
        assert_eq!(payload_offset(0xc002_1000, 2), Some(Payload::At(8)));
        assert_eq!(payload_offset(0xffff_1000, 2), None);
        assert_eq!(payload_offset(0xc003_7600, 0), None);
        assert_eq!(payload_offset(0x0000_1000, 0), None, "not a type-3 header");
    }

    /// The raw gate keeps its first accepted version (`-7e52`, 20260915-125124), and refuses an
    /// unmeasured first version by name without fixing it.
    #[test]
    fn the_raw_gate_keeps_its_first_version() {
        let unimplemented = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
        assert_eq!(raw_init_gate(0, 12), (unimplemented, 0));
        assert_eq!(raw_init_gate(0, 8), (OK, 8));
        assert_eq!(raw_init_gate(8, 13), (INVALID_VERSION, 8));
        assert_eq!(raw_init_gate(8, 8), (OK, 8));
        assert_eq!(raw_init_gate(0, 13), (OK, 13));
        assert_eq!(raw_init_gate(13, 0), (INVALID_VERSION, 13));
    }

    /// The named gate passes 12-14 until the raw gate fixes another version.
    #[test]
    fn the_named_gate_follows_the_raw_one() {
        assert_eq!(named_init_gate(0, 13), OK);
        assert_eq!(named_init_gate(8, 13), INVALID_VERSION);
        assert_ne!(named_init_gate(0, 1), OK);
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
