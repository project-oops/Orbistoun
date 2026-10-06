//! Walking a submitted command buffer into packets.
//!
//! A submission is a buffer of dwords for the GPU's command processor. Each packet header says
//! what kind it is and how many dwords follow, so the stream is walked without understanding any
//! command. Unknown packet types are counted and reported, never skipped; a walk that
//! desynchronises says so, and a packet extending past the buffer is reported, not followed.
//!
//! The packet format is public: the GPU vendor documents it, and the open-source Linux driver and
//! Mesa parse these structures. The length rule agrees with hardware captures of the vendor's own
//! command builders, each with an independently measured length (`tests/measured_packets.rs`).

/// Packet header field positions, as named constants so a correction happens in one place and the
/// report can quote what it used.
mod field {
    /// Packet type occupies the top two bits of every header.
    pub(super) const TYPE_SHIFT: u32 = 30;
    /// Dword count, held one less than the true value.
    pub(super) const COUNT_SHIFT: u32 = 16;
    /// Mask for the count field.
    pub(super) const COUNT_MASK: u32 = 0x3FFF;
    /// Type-3 command opcode.
    pub(super) const OPCODE_SHIFT: u32 = 8;
    /// Mask for the opcode field.
    pub(super) const OPCODE_MASK: u32 = 0xFF;
    /// Type-0 base register index.
    pub(super) const REGISTER_MASK: u32 = 0xFFFF;
}

/// What kind of packet a header describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PacketKind {
    /// A run of consecutive register writes, starting at a base index.
    RegisterWrite {
        /// First register written.
        base_register: u16,
    },
    /// Filler. Carries no body and exists to pad a buffer.
    Filler,
    /// A command, identified by opcode. Draws, dispatches, state changes and shader binds all
    /// arrive as these.
    Command {
        /// Which command.
        opcode: u8,
    },
    /// A header whose type field is reserved.
    ///
    /// Its length is unknown, which makes it a desynchronising event rather than an unrecognised
    /// one.
    Reserved,
}

/// One packet in a submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    /// Byte offset within the submitted buffer.
    pub offset: u32,
    /// Total length in bytes, header included.
    pub length: u32,
    /// The header word, kept so a report can quote what was actually seen.
    pub header: u32,
    /// What the header says this is.
    pub kind: PacketKind,
}

impl Packet {
    /// Byte offset of this packet's body, if it has one.
    pub const fn body_offset(&self) -> u32 {
        self.offset + 4
    }

    /// Length of the body in bytes.
    pub const fn body_length(&self) -> u32 {
        self.length.saturating_sub(4)
    }
}

/// The result of walking one submission.
#[derive(Debug, Clone, Default)]
pub struct PacketWalk {
    /// Every packet, in order.
    pub packets: Vec<Packet>,
    /// A reserved packet type was encountered, so its length was unknown and everything after it is
    /// suspect.
    pub desynchronised: bool,
    /// A packet claimed to extend past the end of the buffer - the best single indicator that a
    /// length rule here is wrong.
    pub overran: bool,
    /// Bytes left over that were not a whole dword.
    pub trailing_bytes: usize,
}

impl PacketWalk {
    /// Whether this walk can be read as a measurement rather than a lower bound.
    pub const fn is_trustworthy(&self) -> bool {
        !self.desynchronised && !self.overran && self.trailing_bytes == 0
    }

    /// Packets of a given kind.
    pub fn count_of(&self, kind: PacketKind) -> usize {
        self.packets.iter().filter(|p| p.kind == kind).count()
    }

    /// Every distinct command opcode seen, with occurrence counts, in a stable order, so two walks
    /// of the same buffer produce identical reports.
    pub fn command_histogram(&self) -> Vec<(u8, usize)> {
        let mut counts = std::collections::BTreeMap::new();
        for packet in &self.packets {
            if let PacketKind::Command { opcode } = packet.kind {
                *counts.entry(opcode).or_insert(0usize) += 1;
            }
        }
        counts.into_iter().collect()
    }
}

/// The smallest a packet can be: a lone header.
pub const MIN_PACKET_BYTES: u32 = 4;

/// Walks a submitted command buffer.
///
/// Never fails. A buffer that cannot be walked is a finding reported through [`PacketWalk`], so a
/// sweep over many submissions says how many were strange rather than stopping at the first.
pub fn walk(bytes: &[u8]) -> PacketWalk {
    let mut result = PacketWalk {
        trailing_bytes: bytes.len() % 4,
        ..PacketWalk::default()
    };

    let mut offset: usize = 0;
    while offset + 4 <= bytes.len() {
        let header = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]);

        // The count field holds one less than the number of body dwords, so a packet with one body
        // dword stores zero; reading it unadjusted truncates every packet by four bytes.
        //
        // A count of all ones is a header-only packet: the hardware's no-op builder writes exactly
        // `0xffff1000` (obSCEne `166-agc/cb-nop`), and the GL cube capture
        // (`tests/captures/agc-gl-cube-fw1240-a`) ends in such words that the hardware runs past.
        // Read as `0x3fff + 1` body dwords it would describe a 64 KiB packet and overrun.
        let count = (header >> field::COUNT_SHIFT) & field::COUNT_MASK;
        let body_dwords = if count == field::COUNT_MASK {
            0
        } else {
            count + 1
        };

        let (kind, length) = match (header >> field::TYPE_SHIFT) & 0x3 {
            0 => (
                PacketKind::RegisterWrite {
                    base_register: (header & field::REGISTER_MASK) as u16,
                },
                4 + body_dwords * 4,
            ),
            2 => (PacketKind::Filler, MIN_PACKET_BYTES),
            3 => (
                PacketKind::Command {
                    opcode: ((header >> field::OPCODE_SHIFT) & field::OPCODE_MASK) as u8,
                },
                4 + body_dwords * 4,
            ),
            _ => {
                // Reserved: its length is undefined, so where the next packet begins is a guess.
                // Advance minimally and mark the walk suspect.
                result.desynchronised = true;
                (PacketKind::Reserved, MIN_PACKET_BYTES)
            }
        };

        let packet = Packet {
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
            length,
            header,
            kind,
        };

        if offset + length as usize > bytes.len() {
            // Following it would read past the buffer into unrelated memory.
            result.overran = true;
            result.packets.push(packet);
            break;
        }

        result.packets.push(packet);
        offset += length as usize;
    }

    result
}

/// Building command packets - the write side of [`walk`].
///
/// The same header format, produced rather than parsed, so a packet this writes walks back to the
/// packet it stood for. Public because the `sceGnmDispatch*` command builders hand a guest a buffer
/// of PM4 that it submits later, and the submit path then [`walk`]s it. The format is the vendor's
/// public packet format with Mesa's opcode values.
pub mod build {
    use super::field;

    /// The header of a type-3 command packet: an `opcode`, then `body_dwords` dwords.
    ///
    /// `body_dwords` is the true count; the field stores one less, the adjustment [`super::walk`]
    /// undoes. It must be at least one, since a type-3 packet has a body; zero panics in debug
    /// builds.
    #[must_use]
    pub const fn command_header(opcode: u8, body_dwords: u32) -> u32 {
        debug_assert!(
            body_dwords >= 1,
            "a type-3 packet has at least one body dword"
        );
        (3 << field::TYPE_SHIFT)
            | ((body_dwords - 1) << field::COUNT_SHIFT)
            | ((opcode as u32) << field::OPCODE_SHIFT)
    }

    /// A type-2 filler: one dword, no body. Valid PM4 that does nothing, for reserving space whose
    /// contents are not modelled.
    #[must_use]
    pub const fn filler() -> u32 {
        2 << field::TYPE_SHIFT
    }

    /// `IT_DISPATCH_DIRECT`, the opcode that issues a compute dispatch (Mesa's
    /// `PKT3_DISPATCH_DIRECT`).
    pub const DISPATCH_DIRECT: u8 = 0x15;

    /// The dwords of a direct compute dispatch of `x` by `y` by `z` thread groups.
    ///
    /// A five-dword `DISPATCH_DIRECT` packet: the three dimensions and the dispatch initiator,
    /// whose one set bit enables the compute shader. The hardware state a builder writes around it
    /// is not modelled. Bit 1 of the header is the shader type, set for the compute pipe: obSCEne's
    /// `165-gnm/dispatch-direct` measures the header as `0xc0031502`.
    #[must_use]
    pub fn dispatch_direct(x: u32, y: u32, z: u32) -> [u32; 5] {
        /// `COMPUTE_DISPATCH_INITIATOR` with `COMPUTE_SHADER_EN` (bit 0) set.
        const COMPUTE_SHADER_EN: u32 = 1;
        /// Shader-type bit of the type-3 header: 1 is the compute pipe, which a dispatch requires.
        const SHADER_TYPE_COMPUTE: u32 = 0x2;
        [
            command_header(DISPATCH_DIRECT, 4) | SHADER_TYPE_COMPUTE,
            x,
            y,
            z,
            COMPUTE_SHADER_EN,
        ]
    }

    /// The `DISPATCH_DIRECT` `sceAgcCbDispatch(cb, x, y, z, ..)` writes: header `0xc0031500` (no
    /// shader-type bit, unlike [`dispatch_direct`]'s), the three thread-group counts, and the
    /// initiator `0x41` - `COMPUTE_SHADER_EN` (bit 0) and bit 6 - which no argument measured
    /// changes (obSCEne `reports/report-1791286625.txt` 12105-12185, REQ cd15).
    #[must_use]
    pub fn agc_dispatch_direct(x: u32, y: u32, z: u32) -> [u32; 5] {
        /// The measured initiator.
        const INITIATOR: u32 = 0x41;
        [command_header(DISPATCH_DIRECT, 4), x, y, z, INITIATOR]
    }

    /// Opcodes measured coming out of the hardware's own command builders.
    ///
    /// Every value was read off a packet `libSceAgc` wrote when called with known arguments; the
    /// public PM4 names are Mesa's `PKT3_*`. Each satisfies the type-3 length rule: the header's
    /// count field plus two dwords is exactly the byte count the builder advanced.
    pub mod measured {
        /// `IT_INDEX_BUFFER_SIZE`, from `sceAgcDcbSetIndexCount` (header `0xc0001300`).
        pub const INDEX_BUFFER_SIZE: u8 = 0x13;
        /// `IT_INDEX_BASE`, from `sceAgcDcbSetIndexBuffer` (header `0xc0012600`).
        pub const INDEX_BASE: u8 = 0x26;
        /// `IT_DRAW_INDEX_2`, from `sceAgcDcbDrawIndex` (header `0xc0042700`).
        pub const DRAW_INDEX_2: u8 = 0x27;
        /// `IT_DRAW_INDEX_AUTO`, from `sceAgcDcbDrawIndexAuto` (header `0xc0012d00`).
        pub const DRAW_INDEX_AUTO: u8 = 0x2d;
        /// `IT_NUM_INSTANCES`, from `sceAgcDcbSetNumInstances` (header `0xc0002f00`).
        pub const NUM_INSTANCES: u8 = 0x2f;
        /// `IT_EVENT_WRITE`, from `sceAgcDcbEventWrite` (header `0xc0004600`).
        pub const EVENT_WRITE: u8 = 0x46;
        /// `IT_SET_CONTEXT_REG`, from `sceAgcDcbSetCxRegisterDirect` (header `0xc0016900`).
        pub const SET_CONTEXT_REG: u8 = 0x69;
        /// `IT_SET_SH_REG`, from `sceAgcCbSetShRegisterRangeDirect` (header `0xc0027600`).
        pub const SET_SH_REG: u8 = 0x76;
        /// `IT_SET_UCONFIG_REG`, from `sceAgcDcbSetUcRegisterDirect` (header `0xc0017900`).
        pub const SET_UCONFIG_REG: u8 = 0x79;
        /// `IT_SET_UCONFIG_REG_INDEX`, from `sceAgcDcbSetIndexSize` (header `0xc0017a00`).
        ///
        /// Distinct from [`SET_UCONFIG_REG`]: measured side by side, `0x79` carried selector
        /// `0x242` with no high bits while `0x7a` carried `0x20000243` - the same register offset
        /// space with an index in the high half.
        pub const SET_UCONFIG_REG_INDEX: u8 = 0x7a;
        /// `IT_SET_CONTEXT_REG_INDIRECT`, from `sceAgcDcbSetCxRegistersIndirect` (header
        /// `0xc0039f00`, four body dwords, 20 bytes total; `166-agc/patch-cx-registers-indirect`).
        ///
        /// The opcode and extent are measured; the body reflects the probe's own arguments, so the
        /// encoder reserves the measured length and writes only the header.
        pub const SET_CONTEXT_REG_INDIRECT: u8 = 0x9f;
        /// `IT_NOP`, from `sceAgcCbNop` (the whole packet is the header `0xffff1000`, 4 bytes).
        pub const NOP: u8 = 0x10;
        /// `IT_ACQUIRE_MEM`, from `sceAgcDcbAcquireMem` (header `0xc0065800`, seven body dwords,
        /// 32 bytes total).
        pub const ACQUIRE_MEM: u8 = 0x58;
        /// `IT_RELEASE_MEM`, from `sceAgcCbReleaseMem` (header `0xc0064900`, seven body dwords, 32
        /// bytes total; `166-agc/cb-release-mem`). The argument-to-body map is not pinned.
        pub const RELEASE_MEM: u8 = 0x49;
        /// `IT_DMA_DATA`, from `sceAgcDcbDmaData` (header `0xc0055000`, six body dwords, 28 bytes
        /// total; `166-agc/dcb-dma-data`).
        pub const DMA_DATA: u8 = 0x50;
        /// `IT_SET_BASE`, from `sceAgcDcbSetBaseIndirectArgs` (header `0xc0021100`, three body
        /// dwords, 16 bytes total; `166-agc/dcb-set-base-indirect-args`).
        pub const SET_BASE: u8 = 0x11;
        /// `IT_DISPATCH_INDIRECT`, from `sceAgcDcbDispatchIndirect`/`AcbDispatchIndirect` (headers
        /// `0xc0011600` / `0xc0021600`; `166-agc/dcb-dispatch-indirect` and
        /// `acb-dispatch-indirect`).
        pub const DISPATCH_INDIRECT: u8 = 0x16;
        /// `IT_DRAW_INDIRECT`, from `sceAgcDcbDrawIndirect` (header `0xc0032400`, four body dwords,
        /// 20 bytes; `166-agc/dcb-draw-indirect`).
        pub const DRAW_INDIRECT: u8 = 0x24;
        /// `IT_DRAW_INDEX_INDIRECT`, from `sceAgcDcbDrawIndexIndirect` (header `0xc0032500`, four
        /// body dwords, 20 bytes; `166-agc/dcb-draw-index-indirect`).
        pub const DRAW_INDEX_INDIRECT: u8 = 0x25;
        /// `IT_SET_SH_REG_INDIRECT`, from `sceAgcDcbSetShRegistersIndirect` (header `0xc0036300`,
        /// four body dwords, 20 bytes; `166-agc/dcb-set-sh-registers-indirect`).
        pub const SET_SH_REG_INDIRECT: u8 = 0x63;
        /// `IT_SET_UCONFIG_REG_INDIRECT`, from `sceAgcDcbSetUcRegistersIndirect` (header
        /// `0xc0036400`, four body dwords, 20 bytes; `166-agc/dcb-set-uc-registers-indirect`).
        pub const SET_UCONFIG_REG_INDIRECT: u8 = 0x64;
        /// The stall from `sceAgcDcbStallCommandBufferParser` (header `0xc0004200`, one body dword,
        /// 8 bytes; `166-agc/dcb-stall-cb-parser`).
        pub const STALL_COMMAND_BUFFER_PARSER: u8 = 0x42;
    }

    /// A reservation skeleton for a builder measured only by its header and extent: the measured
    /// `opcode`, the measured `body_dwords`, and a zeroed body (D696).
    ///
    /// The argument-to-body permutation is unmeasured, so the body is zero and the guest gets a
    /// real cursor advanced by the measured length. The header is rebuilt through
    /// [`command_header`], so it walks back to the packet it stands for; the caller cites which
    /// measured opcode it passed.
    #[must_use]
    pub fn reservation(opcode: u8, body_dwords: u32) -> Vec<u32> {
        let mut out = vec![command_header(opcode, body_dwords)];
        out.resize(1 + body_dwords as usize, 0);
        out
    }

    /// The `SET_UCONFIG_REG` header the marker and wait builders write, with a bit set in its
    /// reserved low byte that `command_header` does not produce.
    ///
    /// `sceAgcDcbPushMarker`/`PopMarker` write a 12-byte `SET_UCONFIG_REG` (opcode `0x79`, two body
    /// dwords) to the command-processor marker register `0x342` with header `0xc0017904`
    /// (`166-agc/dcb-push-marker`/`pop-marker`). The reserved bit is inert to the walk, which reads
    /// only type, count and opcode.
    const MARKER_HEADER: u32 = 0xc001_7904;

    /// The command-processor marker register the marker and wait builders write, `0x342`, as the
    /// first body dword of their `SET_UCONFIG_REG` (`166-agc/dcb-wait-reg-mem`).
    const MARKER_REGISTER: u32 = 0x342;

    /// A push/pop debug-marker skeleton: the measured 12-byte header, body zeroed.
    ///
    /// Both markers write the same `0x79` packet to register `0x342`, the value distinguishing push
    /// from pop. The value is zero because one pass cannot separate a constant from a colour
    /// argument (D696).
    #[must_use]
    pub fn marker_skeleton() -> [u32; 3] {
        [MARKER_HEADER, 0, 0]
    }

    /// The compound `sceAgcDcbWaitRegMem(dcb, a[0], .., a[10])` writes: a `SET_UCONFIG_REG` of the
    /// marker pair `0x342`/`0x343`, the wait, and a `SET_UCONFIG_REG` restoring `0x342`.
    ///
    /// Measured one argument at a time (obSCEne `reports/report-1791275954.txt`, check
    /// `166-agc/dcb-wait-reg-mem`, REQ wr3a), and on PPSA03416's own call:
    ///
    /// - `a1` selects the form: 0 is `WAIT_REG_MEM` (56 bytes, marker `0xc801_0000`), 1 is
    ///   `WAIT_REG_MEM64` (64 bytes, marker `0xc802_0000`).
    /// - DW1: `MEM_SPACE` (bit 4) always set; `a2 & 7` the compare function; `(a3 & 3) << 8` the
    ///   engine; `(a4 & 3) << 25` the cache policy - the fields Mesa's `sid.h:90-95` names.
    /// - The poll address `a5`: whole into register `0x343`, dword-aligned into DW2.
    /// - DW4 the reference `a6`, DW5 the mask `a7`, DW6 the poll interval `a8 >> 4`.
    /// - `a9..a11` change nothing.
    ///
    /// The address's high half in DW3 is assumed, where radeonsi's `si_cp_wait_mem` puts it and
    /// [`crate::agc`]'s patch writes it. The 64-bit form was measured with `a5..a8` zero, so its
    /// non-zero fields follow `amd_cp_packets_gfx11.h`'s order (address, reference, mask, each low
    /// then high, then the interval) by assumption.
    #[must_use]
    pub fn wait_reg_mem(a: &[u64; 11]) -> Vec<u32> {
        let [
            wide,
            function,
            engine,
            cache,
            poll,
            reference,
            mask,
            interval,
            ..,
        ] = *a;
        let control = (0x10 | (function & 7) | ((engine & 3) << 8) | ((cache & 3) << 25)) as u32;
        let interval = ((interval >> 4) & 0xffff) as u32;
        let mut words = vec![
            0xc002_7904,
            MARKER_REGISTER,
            if wide == 1 { 0xc802_0000 } else { 0xc801_0000 },
            poll as u32,
        ];
        if wide == 1 {
            let address = poll & !7;
            words.extend([
                command_header(crate::cp::WAIT_REG_MEM64, 8),
                control,
                address as u32,
                (address >> 32) as u32,
                reference as u32,
                (reference >> 32) as u32,
                mask as u32,
                (mask >> 32) as u32,
                interval,
            ]);
        } else {
            let address = poll & !3;
            words.extend([
                command_header(crate::cp::WAIT_REG_MEM, 6),
                control,
                address as u32,
                (address >> 32) as u32,
                reference as u32,
                mask as u32,
                interval,
            ]);
        }
        words.extend([MARKER_HEADER, MARKER_REGISTER, 0xc800_0000]);
        words
    }

    /// A `sceAgcDcbResetQueue` skeleton - the measured 32-byte stream, every header kept, body
    /// zero. Eight dwords: a NOP filler, then two `SET_UCONFIG_REG` packets.
    ///
    /// Measured in `166-agc/dcb-reset-queue`: `0xffff1000` (the [`nop`] filler), a four-dword
    /// `SET_UCONFIG_REG` (`0xc0027904`) and a three-dword one (`MARKER_HEADER`), both to the marker
    /// register `0x342`. The marker values the zero-argument pass wrote are address-shaped and may
    /// point into the probe's own writer struct, so the bodies are zeroed (D696) and the headers
    /// kept so the reservation walks back to two packets and advances the cursor by 32.
    #[must_use]
    pub fn reset_queue_skeleton() -> [u32; 8] {
        [0xffff_1000, 0xc002_7904, 0, 0, 0, MARKER_HEADER, 0, 0]
    }

    /// A `SET_CONTEXT_REG_INDIRECT` skeleton - the packet a guest patches with register data. Five
    /// dwords, 20 bytes.
    ///
    /// The producer writes a 20-byte packet with header `0xc0039f00`, and
    /// `sceAgcSetCxRegIndirectPatchAddRegisters` then amends it in place without advancing the
    /// cursor. The format word `0x80000000` in dw3 (offset-and-data, register offset 0) is measured
    /// (`166-agc/patch-cx-registers-indirect`). dw1-dw2 hold the base GPU address, set by
    /// `sceAgcSetCxRegIndirectPatchSetAddress`; dw4 holds the register count, incremented by
    /// `sceAgcSetCxRegIndirectPatchAddRegisters`.
    ///
    /// The producer fills it from its arguments `(dcb, table, count)`: dw1-dw2 the table address
    /// with its low two bits clear, dw4 the count's low fourteen bits. Measured with the probe
    /// passing `(dcb, 2, table)`, which came back as dw1 `0` and dw4 `0x3880`, the table pointer's
    /// low bits.
    #[must_use]
    pub fn set_cx_registers_indirect(table: u64, count: u64) -> [u32; 5] {
        [
            command_header(measured::SET_CONTEXT_REG_INDIRECT, 4),
            (table as u32) & !3,
            (table >> 32) as u32,
            0x8000_0000,
            (count as u32) & INDIRECT_COUNT_MASK,
        ]
    }

    /// Bits of an indirect register packet's dw4 that hold the register count.
    pub const INDIRECT_COUNT_MASK: u32 = 0x3fff;

    /// A `NOP` - a header-only no-op packet, one dword, four bytes.
    ///
    /// Measured whole (`166-agc/cb-nop`): `sceAgcCbNop` writes exactly `0xffff1000`, a type-3
    /// header with an all-ones count. No arguments enter it, so it is complete, not a skeleton.
    #[must_use]
    pub fn nop() -> [u32; 1] {
        [0xffff_1000]
    }

    /// An `ACQUIRE_MEM` skeleton - the packet reserved, its cursor real, its argument body zeroed.
    ///
    /// Header and extent are measured (`166-agc/dcb-acquire-mem`): the builder emits `0xc0065800`
    /// and advances 32 bytes. The seven body dwords are a permutation of the arguments with
    /// unpinned shifts, so the body is zeroed (D696).
    #[must_use]
    pub fn acquire_mem_skeleton() -> [u32; 8] {
        [
            command_header(measured::ACQUIRE_MEM, 7),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]
    }

    /// The `RELEASE_MEM` packet `sceAgcCbReleaseMem(cb, a[0], .., a[10])` writes: eight dwords, 32
    /// bytes, each field placed by the argument that sets it.
    ///
    /// Measured one argument at a time, all ones and every other argument zero (obSCEne
    /// `-1044`, check `166-agc/cb-release-mem-args`), and checked against distinct markers and
    /// sentinels (`-ff23`):
    ///
    /// - DW1: event type `a1 & 0x3f`; event index 5, or 6 when bit 6 of `a1` is set;
    ///   `(a2 & 0xfff) << 12`; `(a4 & 3) << 25`.
    /// - DW2: `(a3 & 3) << 16`; interrupt select `(a10 & 7) << 24`; data select `(a6 & 7) << 29`.
    /// - DW3-DW4: the destination `a5`, its low two bits clear.
    /// - DW5-DW6: the data `a7`.
    /// - DW7: `a11 & 0x07ff_ffff`.
    ///
    /// `a8` and `a9` land nowhere. Every argument from `a6` on is a stack argument.
    #[must_use]
    pub fn release_mem(a: &[u64; 11]) -> [u32; 8] {
        let [a1, a2, a3, a4, a5, a6, a7, _, _, a10, a11] = *a;
        let index = 5 + ((a1 >> 6) & 1);
        let destination = a5 & !3;
        [
            command_header(measured::RELEASE_MEM, 7),
            ((a1 & 0x3f) | (index << 8) | ((a2 & 0xfff) << 12) | ((a4 & 3) << 25)) as u32,
            (((a3 & 3) << 16) | ((a10 & 7) << 24) | ((a6 & 7) << 29)) as u32,
            destination as u32,
            (destination >> 32) as u32,
            a7 as u32,
            (a7 >> 32) as u32,
            (a11 & 0x07ff_ffff) as u32,
        ]
    }

    /// The `DMA_DATA` packet `sceAgcDcbDmaData(dcb, a[0], .., a[10])` writes: seven dwords, 28
    /// bytes, each field placed by the argument that sets it.
    ///
    /// Measured one argument at a time, all ones and every other argument zero (obSCEne
    /// `-1044`, check `166-agc/dcb-dma-data-args`), and checked against distinct markers and
    /// sentinels (`-ff23`):
    ///
    /// - DW1: `a1 & 1`; `(a6 & 3) << 13`; `(a2 & 3) << 20`; `(a3 & 3) << 25`; `(a5 & 3) << 29`;
    ///   `(a11 & 1) << 31`.
    /// - DW2-DW3: the source `a7`. DW4-DW5: the destination `a4`.
    /// - DW6: the byte count `a8 & 0x03ff_ffff`; bit 26 is `a5 & 1` and bit 28 is set when
    ///   `a5 & 3 == 3`; bit 27 is `a2 & 1` and bit 29 is set when `a2 & 3 == 3`; `(a9 & 1) << 30`;
    ///   `(a10 & 1) << 31`. The select values decode into those bits rather than copy into them:
    ///   `a5 = 2` sets neither, `0x55` only bit 26, all ones both. `a2 & 3 == 1` is unmeasured and
    ///   follows `a5`'s pattern.
    ///
    /// Every argument from `a6` on is a stack argument.
    #[must_use]
    pub fn dma_data(a: &[u64; 11]) -> [u32; 7] {
        let [a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11] = *a;
        let control = (a1 & 1)
            | ((a6 & 3) << 13)
            | ((a2 & 3) << 20)
            | ((a3 & 3) << 25)
            | ((a5 & 3) << 29)
            | ((a11 & 1) << 31);
        let both = |select: u64| u64::from(select & 3 == 3);
        let count = (a8 & 0x03ff_ffff)
            | ((a5 & 1) << 26)
            | ((a2 & 1) << 27)
            | (both(a5) << 28)
            | (both(a2) << 29)
            | ((a9 & 1) << 30)
            | ((a10 & 1) << 31);
        [
            command_header(measured::DMA_DATA, 6),
            control as u32,
            a7 as u32,
            (a7 >> 32) as u32,
            a4 as u32,
            (a4 >> 32) as u32,
            count as u32,
        ]
    }

    /// A `SET_BASE` skeleton for the indirect-args base - the packet reserved, cursor real, body
    /// zeroed. Four dwords, 16 bytes.
    ///
    /// Header and extent are measured (`166-agc/dcb-set-base-indirect-args`): `0xc0021100`, 16
    /// bytes. The one pass carried `1` in the first body dword (the base-index selector for the
    /// draw-indirect base) and then an address; one pass cannot separate a constant selector from
    /// an argument, so the whole body is zeroed (D696).
    #[must_use]
    pub fn set_base_indirect_args_skeleton() -> [u32; 4] {
        [command_header(measured::SET_BASE, 3), 0, 0, 0]
    }

    /// An `EVENT_WRITE` for `event_type`. Two dwords.
    ///
    /// Measured whole (`166-agc/dcb-event-write`): `sceAgcDcbEventWrite(dcb, 62, 0)` wrote
    /// `0xc0004600` then `0x3e`, advancing 8 bytes. This is the short, address-less form; a longer
    /// form carrying a destination address is not this function.
    #[must_use]
    pub fn event_write(event_type: u32) -> [u32; 2] {
        [command_header(measured::EVENT_WRITE, 1), event_type]
    }

    /// An `INDEX_BUFFER_SIZE` declaring how many indices a subsequent draw reads. Two dwords.
    ///
    /// Measured whole (`166-agc/dcb-set-index-count`): `sceAgcDcbSetIndexCount(dcb, 36)` wrote
    /// `0xc0001300` then `0x24`, advancing 8 bytes, and its own `GetSize` answered 8.
    #[must_use]
    pub fn set_index_count(indices: u32) -> [u32; 2] {
        [command_header(measured::INDEX_BUFFER_SIZE, 1), indices]
    }

    /// A `NUM_INSTANCES` setting the instance count for subsequent draws. Two dwords.
    ///
    /// Measured whole (`166-agc/dcb-set-num-instances`): `sceAgcDcbSetNumInstances(dcb, 1)` wrote
    /// `0xc0002f00` then `1`, advancing 8 bytes.
    #[must_use]
    pub fn set_num_instances(instances: u32) -> [u32; 2] {
        [command_header(measured::NUM_INSTANCES, 1), instances]
    }

    /// A `DRAW_INDEX_AUTO` - a draw whose indices are generated rather than fetched. Three dwords.
    ///
    /// Measured whole (`166-agc/dcb-draw-auto`): `sceAgcDcbDrawIndexAuto(dcb, 3, 2)` wrote
    /// `0xc0012d00`, `3`, `2`, advancing 12 bytes.
    #[must_use]
    pub fn draw_index_auto(index_count: u32, initiator: u32) -> [u32; 3] {
        [
            command_header(measured::DRAW_INDEX_AUTO, 2),
            index_count,
            initiator,
        ]
    }

    /// An `INDEX_BASE` carrying the 64-bit address of the index buffer. Three dwords.
    ///
    /// Measured whole (`166-agc/dcb-set-index-buffer`): `sceAgcDcbSetIndexBuffer(dcb, 0x12345678)`
    /// wrote `0xc0012600`, `0x12345678`, `0`, advancing 12 bytes: the address, low half first.
    #[must_use]
    pub fn set_index_base(address: u64) -> [u32; 3] {
        let lo = u32::try_from(address & 0xffff_ffff).unwrap_or_default();
        let hi = u32::try_from(address >> 32).unwrap_or_default();
        [command_header(measured::INDEX_BASE, 2), lo, hi]
    }

    /// A `SET_CONTEXT_REG` writing one value into one context register. Three dwords.
    ///
    /// Measured whole (`166-agc/dcb-set-cx-reg`): the builder takes `(value << 32) | offset` as one
    /// quadword and wrote `0xc0016900`, `0x200`, `0x12345678`, advancing 12 bytes.
    #[must_use]
    pub fn set_context_register(offset: u16, value: u32) -> [u32; 3] {
        [
            command_header(measured::SET_CONTEXT_REG, 2),
            u32::from(offset),
            value,
        ]
    }

    /// A `SET_UCONFIG_REG` writing one value into one user-config register. Three dwords.
    ///
    /// Measured whole (`166-agc/dcb-set-uc-reg`): the same packed-quadword argument as
    /// [`set_context_register`]; `(4 << 32) | 0x242` wrote `0xc0017900`, `0x242`, `4`, advancing 12
    /// bytes.
    #[must_use]
    pub fn set_uconfig_register(offset: u16, value: u32) -> [u32; 3] {
        [
            command_header(measured::SET_UCONFIG_REG, 2),
            u32::from(offset),
            value,
        ]
    }

    /// A `SET_SH_REG` writing a run of consecutive shader registers. `values.len() + 2` dwords.
    ///
    /// Measured whole (`166-agc/dcb-set-sh-reg-direct`): `sceAgcCbSetShRegisterRangeDirect(cb,
    /// 0x08, values, 2)` wrote `0xc0027600`, `8`, `0x12345678`, `0x9abcdef0`, advancing 16 bytes,
    /// with no leading marker. Another project's table gives `n + 4`; the measurement governs
    /// (D683).
    ///
    /// Returns an empty vector for an empty run: a zero-register write is not a packet.
    #[must_use]
    pub fn set_sh_register_range(offset: u16, values: &[u32]) -> Vec<u32> {
        if values.is_empty() {
            return Vec::new();
        }
        let Ok(body) = u32::try_from(values.len() + 1) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(values.len() + 2);
        out.push(command_header(measured::SET_SH_REG, body));
        out.push(u32::from(offset));
        out.extend_from_slice(values);
        out
    }

    /// A `SET_UCONFIG_REG_INDEX` selecting the index buffer's entry width. Three dwords.
    ///
    /// Measured across eight argument pairs (`166-agc/dcb-set-index-size`): `type` 0-3 and `flags`
    /// 0-1 give `0x400 | (flags << 6) | type`, with the selector `0x20000243` constant. Larger
    /// arguments are masked so a wild value cannot corrupt neighbouring fields.
    #[must_use]
    pub fn set_index_size(index_type: u32, flags: u32) -> [u32; 3] {
        /// The `0x20000243` selector, constant across all measured calls. Its low half is an
        /// ordinary register offset; the high bits distinguish opcode `0x7a` from `0x79`.
        const SELECTOR: u32 = 0x2000_0243;
        /// Bit 10, set in every measured value.
        const BASE: u32 = 0x400;
        [
            command_header(measured::SET_UCONFIG_REG_INDEX, 2),
            SELECTOR,
            BASE | ((flags & 0x1) << 6) | (index_type & 0x3),
        ]
    }

    /// A `DRAW_INDEX_2` - an indexed draw from a bound index buffer. Six dwords.
    ///
    /// `sceAgcDcbDrawIndex(dcb, 3, 0x12345678, 0)` wrote header `0xc0042700` and advanced 24 bytes,
    /// with the address low half, high half and index count at body positions 1, 2 and 3
    /// (`166-agc/dcb-draw-index`). Positions 0 and 4, `max_size` and `initiator`, follow the public
    /// PM4 field order and are caller-supplied.
    #[must_use]
    pub fn draw_index_2(max_size: u32, address: u64, index_count: u32, initiator: u32) -> [u32; 6] {
        let lo = u32::try_from(address & 0xffff_ffff).unwrap_or_default();
        let hi = u32::try_from(address >> 32).unwrap_or_default();
        [
            command_header(measured::DRAW_INDEX_2, 5),
            max_size,
            lo,
            hi,
            index_count,
            initiator,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{PacketKind, walk};

    fn stream(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// Builds a type-3 command header, generated rather than extracted (D051).
    fn command(opcode: u8, body_dwords: u32) -> u32 {
        (3 << 30) | ((body_dwords - 1) << 16) | (u32::from(opcode) << 8)
    }

    fn register_write(base: u16, body_dwords: u32) -> u32 {
        ((body_dwords - 1) << 16) | u32::from(base)
    }

    #[test]
    fn packets_are_walked_at_their_declared_lengths() {
        let bytes = stream(&[
            command(0x22, 2),
            0xAAAA_AAAA,
            0xBBBB_BBBB,
            command(0x37, 1),
            0xCCCC_CCCC,
        ]);
        let result = walk(&bytes);
        assert_eq!(result.packets.len(), 2);
        assert_eq!(result.packets[0].kind, PacketKind::Command { opcode: 0x22 });
        assert_eq!(result.packets[0].length, 12);
        assert_eq!(result.packets[1].offset, 12);
        assert!(result.is_trustworthy());
    }

    #[test]
    fn the_count_field_is_read_as_one_less_than_the_body() {
        // Without the count adjustment every packet is short by one dword and the walk
        // desynchronises on packet two.
        let bytes = stream(&[command(0x10, 1), 0x1111_1111, command(0x11, 1), 0x2222_2222]);
        let result = walk(&bytes);
        assert_eq!(result.packets.len(), 2, "both packets found");
        assert_eq!(result.packets[0].length, 8);
        assert_eq!(result.packets[1].kind, PacketKind::Command { opcode: 0x11 });
        assert!(result.is_trustworthy());
    }

    #[test]
    fn a_register_write_reports_its_base_register() {
        let bytes = stream(&[register_write(0x2C0A, 1), 0x0000_0001]);
        let result = walk(&bytes);
        assert_eq!(
            result.packets[0].kind,
            PacketKind::RegisterWrite {
                base_register: 0x2C0A
            }
        );
    }

    #[test]
    fn a_header_only_no_op_is_one_dword() {
        // Count all ones: the packet is its header, as the hardware's no-op builder writes it.
        let bytes = stream(&[0xFFFF_1000, command(0x05, 1), 0xDEAD_BEEF]);
        let result = walk(&bytes);
        assert_eq!(result.packets[0].kind, PacketKind::Command { opcode: 0x10 });
        assert_eq!(result.packets[0].length, 4);
        assert_eq!(result.packets[1].kind, PacketKind::Command { opcode: 0x05 });
        assert!(result.is_trustworthy());
    }

    #[test]
    fn filler_carries_no_body() {
        // Filler has no count field; honouring one would swallow whatever follows.
        let bytes = stream(&[0x8000_0000, command(0x05, 1), 0xDEAD_BEEF]);
        let result = walk(&bytes);
        assert_eq!(result.packets[0].kind, PacketKind::Filler);
        assert_eq!(result.packets[0].length, 4);
        assert_eq!(result.packets[1].kind, PacketKind::Command { opcode: 0x05 });
        assert!(result.is_trustworthy());
    }

    #[test]
    fn a_reserved_packet_type_desynchronises_the_walk() {
        // Its length is undefined, so the next packet's position is a guess, and the walk says so.
        let bytes = stream(&[0x4000_0000, command(0x05, 1), 0xDEAD_BEEF]);
        let result = walk(&bytes);
        assert!(result.desynchronised);
        assert!(!result.is_trustworthy());
        assert_eq!(result.packets[0].kind, PacketKind::Reserved);
    }

    #[test]
    fn a_packet_extending_past_the_buffer_is_reported_not_followed() {
        let bytes = stream(&[command(0x22, 8), 0x1111_1111]);
        let result = walk(&bytes);
        assert!(result.overran);
        assert!(!result.is_trustworthy());
    }

    #[test]
    fn a_histogram_counts_commands_in_a_stable_order() {
        // Reports are diffed between runs, so the order is stable.
        let bytes = stream(&[
            command(0x30, 1),
            0x0,
            command(0x10, 1),
            0x0,
            command(0x30, 1),
            0x0,
        ]);
        let result = walk(&bytes);
        assert_eq!(result.command_histogram(), vec![(0x10, 1), (0x30, 2)]);
    }

    #[test]
    fn an_empty_submission_walks_to_nothing_without_complaint() {
        // An empty buffer is a legitimate submission, not a malformed one.
        let result = walk(&[]);
        assert!(result.packets.is_empty());
        assert!(result.is_trustworthy());
    }

    #[test]
    fn a_buffer_that_is_not_whole_dwords_is_reported() {
        let mut bytes = stream(&[command(0x05, 1), 0x0]);
        bytes.push(0x99);
        let result = walk(&bytes);
        assert_eq!(result.trailing_bytes, 1);
        assert!(!result.is_trustworthy());
    }

    /// A header the builder writes is a header the walker reads back as the same packet: a built
    /// dispatch walks back to one `DISPATCH_DIRECT` command of the right length.
    #[test]
    fn a_built_dispatch_walks_back_to_the_packet_it_stood_for() {
        use super::build;

        assert_eq!(
            build::command_header(build::DISPATCH_DIRECT, 4),
            command(build::DISPATCH_DIRECT, 4),
            "the builder's header matches the one computed from the format directly"
        );

        let dwords = build::dispatch_direct(64, 1, 1);
        let result = walk(&stream(&dwords));
        assert_eq!(result.packets.len(), 1, "one packet");
        assert_eq!(
            result.packets[0].kind,
            PacketKind::Command {
                opcode: build::DISPATCH_DIRECT
            }
        );
        assert_eq!(result.packets[0].length, 20, "header plus four body dwords");
        assert!(result.is_trustworthy(), "and it walks cleanly to the end");
    }

    /// Both builders' packets are the bytes obSCEne dumped (`-1044`, sweep 20260928-212840, lines
    /// 10741-10930): each argument alone at all ones, then the markers, PPSA03416's arguments and
    /// the two sentinel sets, which move every field at once.
    #[test]
    fn release_mem_is_the_measured_packet() {
        use super::build::release_mem;
        const ONES: u64 = u64::MAX;
        let alone = |slot: usize| {
            let mut a = [0_u64; 11];
            a[slot] = ONES;
            a
        };
        let markers: [u64; 11] = [
            0x11,
            0x22,
            0x33,
            0x44,
            0x55,
            0x1111_6666,
            0x7777_7777_7777_0007,
            0x8888_0008,
            0x9999_0009,
            0xaaaa_000a,
            0xbbbb_000b,
        ];
        let mut ppsa_release = markers;
        ppsa_release[..5].copy_from_slice(&[0x2b, 0, 1, 3, 0]);
        let mut ppsa_dma = markers;
        ppsa_dma[..5].copy_from_slice(&[0, 0, 0, 0, 2]);
        let sentinel_a: [u64; 11] = std::array::from_fn(|i| 0x1111_1111_1111_1111 * (i as u64 + 1));
        let mut sentinel_b: [u64; 11] =
            std::array::from_fn(|i| 0xc1c1_c1c1_c1c1_c1c1 + 0x0101_0101_0101_0101 * i as u64);
        sentinel_b[9] = 0xca0c_a0ca_0ca0_ca0c;

        let release: [(&[u64; 11], [u32; 7]); 16] = [
            (&[0; 11], [0x500, 0, 0, 0, 0, 0, 0]),
            (&alone(0), [0x63f, 0, 0, 0, 0, 0, 0]),
            (&alone(1), [0x00ff_f500, 0, 0, 0, 0, 0, 0]),
            (&alone(2), [0x500, 0x0003_0000, 0, 0, 0, 0, 0]),
            (&alone(3), [0x0600_0500, 0, 0, 0, 0, 0, 0]),
            (&alone(4), [0x500, 0, 0xffff_fffc, 0xffff_ffff, 0, 0, 0]),
            (&alone(5), [0x500, 0xe000_0000, 0, 0, 0, 0, 0]),
            (&alone(6), [0x500, 0, 0, 0, 0xffff_ffff, 0xffff_ffff, 0]),
            (&alone(7), [0x500, 0, 0, 0, 0, 0, 0]),
            (&alone(8), [0x500, 0, 0, 0, 0, 0, 0]),
            (&alone(9), [0x500, 0x0700_0000, 0, 0, 0, 0, 0]),
            (&alone(10), [0x500, 0, 0, 0, 0, 0, 0x07ff_ffff]),
            (
                &markers,
                [
                    0x0002_2511,
                    0xc203_0000,
                    0x54,
                    0,
                    0x7777_0007,
                    0x7777_7777,
                    0x03bb_000b,
                ],
            ),
            (
                &ppsa_release,
                [
                    0x0600_052b,
                    0xc201_0000,
                    0,
                    0,
                    0x7777_0007,
                    0x7777_7777,
                    0x03bb_000b,
                ],
            ),
            (
                &sentinel_a,
                [
                    0x0022_2511,
                    0xc203_0000,
                    0x5555_5554,
                    0x5555_5555,
                    0x7777_7777,
                    0x7777_7777,
                    0x03bb_bbbb,
                ],
            ),
            (
                &sentinel_b,
                [
                    0x002c_2601,
                    0xc403_0000,
                    0xc5c5_c5c4,
                    0xc5c5_c5c5,
                    0xc7c7_c7c7,
                    0xc7c7_c7c7,
                    0x03cb_cbcb,
                ],
            ),
        ];
        for (arguments, body) in release {
            let packet = release_mem(arguments);
            assert_eq!(packet[0], 0xc006_4900);
            assert_eq!(packet[1..], body, "{arguments:x?}");
        }
    }

    /// `DMA_DATA`'s side of the same measurements.
    #[test]
    fn dma_data_is_the_measured_packet() {
        use super::build::dma_data;
        const ONES: u64 = u64::MAX;
        let alone = |slot: usize| {
            let mut a = [0_u64; 11];
            a[slot] = ONES;
            a
        };
        let markers: [u64; 11] = [
            0x11,
            0x22,
            0x33,
            0x44,
            0x55,
            0x1111_6666,
            0x7777_7777_7777_0007,
            0x8888_0008,
            0x9999_0009,
            0xaaaa_000a,
            0xbbbb_000b,
        ];
        let mut ppsa_release = markers;
        ppsa_release[..5].copy_from_slice(&[0x2b, 0, 1, 3, 0]);
        let mut ppsa_dma = markers;
        ppsa_dma[..5].copy_from_slice(&[0, 0, 0, 0, 2]);
        let sentinel_a: [u64; 11] = std::array::from_fn(|i| 0x1111_1111_1111_1111 * (i as u64 + 1));
        let mut sentinel_b: [u64; 11] =
            std::array::from_fn(|i| 0xc1c1_c1c1_c1c1_c1c1 + 0x0101_0101_0101_0101 * i as u64);
        sentinel_b[9] = 0xca0c_a0ca_0ca0_ca0c;

        let dma: [(&[u64; 11], [u32; 6]); 15] = [
            (&[0; 11], [0, 0, 0, 0, 0, 0]),
            (&alone(0), [1, 0, 0, 0, 0, 0]),
            (&alone(1), [0x0030_0000, 0, 0, 0, 0, 0x2800_0000]),
            (&alone(2), [0x0600_0000, 0, 0, 0, 0, 0]),
            (&alone(3), [0, 0, 0, 0xffff_ffff, 0xffff_ffff, 0]),
            (&alone(4), [0x6000_0000, 0, 0, 0, 0, 0x1400_0000]),
            (&alone(5), [0x6000, 0, 0, 0, 0, 0]),
            (&alone(6), [0, 0xffff_ffff, 0xffff_ffff, 0, 0, 0]),
            (&alone(7), [0, 0, 0, 0, 0, 0x03ff_ffff]),
            (&alone(8), [0, 0, 0, 0, 0, 0x4000_0000]),
            (&alone(9), [0, 0, 0, 0, 0, 0x8000_0000]),
            (&alone(10), [0x8000_0000, 0, 0, 0, 0, 0]),
            (
                &markers,
                [0xa620_4001, 0x7777_0007, 0x7777_7777, 0x44, 0, 0x4488_0008],
            ),
            (
                &ppsa_dma,
                [0xc000_4000, 0x7777_0007, 0x7777_7777, 0, 0, 0x4088_0008],
            ),
            (
                &sentinel_b,
                [
                    0xa620_4001,
                    0xc7c7_c7c7,
                    0xc7c7_c7c7,
                    0xc4c4_c4c4,
                    0xc4c4_c4c4,
                    0x44c8_c8c8,
                ],
            ),
        ];
        for (arguments, body) in dma {
            let packet = dma_data(arguments);
            assert_eq!(packet[0], 0xc005_5000);
            assert_eq!(packet[1..], body, "{arguments:x?}");
        }
        let packet = dma_data(&sentinel_a);
        assert_eq!(
            packet[1..],
            [
                0xa620_4001,
                0x7777_7777,
                0x7777_7777,
                0x4444_4444,
                0x4444_4444,
                0x4488_8888
            ]
        );
    }
}
