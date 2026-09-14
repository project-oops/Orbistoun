//! Walking a submitted command buffer into packets.
//!
//! # What a submission actually is
//!
//! When a guest calls the submit function it hands over a buffer of dwords addressed
//! to the GPU's command processor. That buffer is a packet stream: each packet has a
//! header saying what kind it is and how many dwords follow, so the stream can be
//! walked without understanding a single command.
//!
//! Walking it is the entire first step of GPU work, and it is worth doing long before
//! anything is translated. A submission decoded into "set these registers, bind that,
//! draw N times" is the difference between an opaque blob and a work list.
//!
//! # The same discipline as everywhere else
//!
//! Unknown packet types are **counted and reported**, never skipped. A walk that
//! desynchronises says so. A packet claiming to extend past the end of the buffer is
//! a strong signal that a length rule is wrong, and is reported rather than followed
//! into whatever memory happens to sit after the buffer.
//!
//! # Provenance
//!
//! The packet format is public: AMD documents it, and the open-source Linux driver
//! and Mesa parse these exact structures. Hardware documentation from the chip
//! vendor, not console firmware.
//!
//! The values below were **transcribed and unverified** for as long as there was nothing to
//! check them against. As of D565 there is: obSCEne called four `libSceAgc` command builders on
//! hardware and captured what each wrote, with an **independently measured length** for every
//! one - bytes it saw change, owing nothing to any header field.
//!
//! `tests/measured_packets.rs` walks all four. The length rule consumes three of them **exactly**,
//! including one that decomposes into three packets of 16, 28 and 12 bytes landing on a measured
//! 56. Dropping the count adjustment, doubling it, or moving the opcode field all make it fail.
//!
//! So this is no longer transcribed-and-unchecked; it is transcribed **and agreed with hardware
//! on four buffers**. That is not the same as verified line by line - a rule that erred on a
//! packet none of the four contains would still pass - and the caveat narrows rather than lifts.

/// Packet header field positions.
///
/// Kept as named constants rather than inline literals so a correction happens in one
/// place, and so the report can quote what it used.
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
    /// A command, identified by opcode. The interesting one - draws, dispatches,
    /// state changes and shader binds all arrive as these.
    Command {
        /// Which command.
        opcode: u8,
    },
    /// A header whose type field is reserved.
    ///
    /// Its length is therefore unknown, which is what makes it a desynchronising
    /// event rather than merely an unrecognised one.
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
    /// A reserved packet type was encountered, so its length was unknown.
    ///
    /// Everything after that point is suspect.
    pub desynchronised: bool,
    /// A packet claimed to extend past the end of the buffer.
    ///
    /// The best single indicator that a length rule here is wrong.
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

    /// Every distinct command opcode seen, with occurrence counts, in a stable order.
    ///
    /// Ordered so two walks of the same buffer produce byte-identical reports - these
    /// get diffed, and spurious reordering trains a reader to ignore the diff.
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
/// Never fails. A buffer that cannot be walked is a finding reported through
/// [`PacketWalk`], not an error - a sweep over many submissions has to say how many
/// were strange, not stop at the first one.
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

        // The count field holds one less than the number of body dwords, so a packet
        // with a single body dword stores zero. Reading it without the adjustment
        // truncates every packet by four bytes and desynchronises immediately.
        //
        // A count of all ones is the exception: a header-only packet. Measured twice:
        // the console's own no-op builder writes exactly four bytes, `0xffff1000`
        // (obSCEne `166-agc/cb-nop`, sweep 20260914-100833), and the GL cube capture
        // ends in sixteen such words that the hardware ran past to retire its fence
        // (tests/captures/agc-gl-cube-fw1240-a). Read as `0x3fff + 1` body dwords it
        // would describe a 64 KiB packet and every stream padded this way would overrun.
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
                // Reserved. Its length is not defined, so where the next packet begins
                // is a guess - advance minimally and mark the walk suspect.
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
/// packet it stood for. It is public because the GNM command builders (`sceGnmDispatch*`) are
/// exactly this job: they hand a guest a buffer of PM4 that the guest submits later, and the submit
/// path then [`walk`]s. Provenance is the walker's - AMD's public packet format and mesa's opcode
/// values, not console firmware.
pub mod build {
    use super::field;

    /// The header of a **type-3** command packet: an `opcode`, then `body_dwords` dwords.
    ///
    /// `body_dwords` is the true count; the field stores one less, the adjustment [`super::walk`]
    /// undoes. It must be at least one - a type-3 packet with an empty body is not a thing - and a
    /// caller passing zero gets a debug-time panic rather than a header that walks back wrong.
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

    /// A **type-2** filler: one dword, no body. Valid PM4 that does nothing, for reserving space
    /// whose contents are not yet modelled honestly rather than left as whatever was there.
    #[must_use]
    pub const fn filler() -> u32 {
        2 << field::TYPE_SHIFT
    }

    /// `IT_DISPATCH_DIRECT`, the opcode that issues a compute dispatch. A public AMD value - mesa's
    /// `PKT3_DISPATCH_DIRECT`.
    pub const DISPATCH_DIRECT: u8 = 0x15;

    /// The dwords of a direct compute dispatch of `x` by `y` by `z` thread groups.
    ///
    /// A `DISPATCH_DIRECT` packet: the three dimensions and the dispatch initiator, whose one set
    /// bit enables the compute shader and so begins the dispatch. Five dwords. What a console's own
    /// builder writes *around* it - the hardware state it assumes is already set - is not modelled
    /// here; this is the dispatch itself, in the documented encoding.
    ///
    /// **The header routes the packet to the compute pipe.** Bit 1 of a type-3 header is the
    /// shader-type: a dispatch is always compute, so it is set - obSCEne's `165-gnm/dispatch-direct`
    /// measured the header as `0xc0031502` on hardware, where an unset bit would give `0xc0031500`.
    /// (The count the same check reports is `6` to this packet's `5`: the sixth dword is the
    /// surrounding state above, which is deliberately not modelled, not part of this packet.)
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

    /// Opcodes measured coming out of the console's own command builders.
    ///
    /// **Every value below was read off a packet `libSceAgc` wrote**, not transcribed from a
    /// document and hoped to match - obSCEne sweep `20260914-100833` called each builder with known
    /// arguments and recorded the header it produced (orbistoun worklog 534). The public PM4 names
    /// are mesa's `PKT3_*`; the numbers are ours.
    ///
    /// Every one satisfies the type-3 length rule: the header's count field plus two dwords is
    /// exactly the byte count the builder advanced, on all ten. That is the field split D565 derived
    /// from four builders and could not then confirm.
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
        /// **Distinct from [`SET_UCONFIG_REG`], and the pair is what shows it.** Measured in one
        /// run, `0x79` carried selector `0x242` with no high bits while `0x7a` carried
        /// `0x20000243` - same low-half register offset space, an index in the high half. Derived
        /// from the two packets side by side, not cited.
        pub const SET_UCONFIG_REG_INDEX: u8 = 0x7a;
    }

    /// An `EVENT_WRITE` for `event_type`. Two dwords.
    ///
    /// **Measured whole** (`166-agc/dcb-event-write`): `sceAgcDcbEventWrite(dcb, 62, 0)` wrote
    /// `0xc0004600` then `0x3e`, advancing 8 bytes. The event type passes through unchanged.
    ///
    /// This is the **short, address-less form**. A longer form that carries a destination address
    /// exists - another implementation publishes four dwords for it - and is not this function.
    #[must_use]
    pub fn event_write(event_type: u32) -> [u32; 2] {
        [command_header(measured::EVENT_WRITE, 1), event_type]
    }

    /// An `INDEX_BUFFER_SIZE` declaring how many indices a subsequent draw reads. Two dwords.
    ///
    /// **Measured whole** (`166-agc/dcb-set-index-count`): `sceAgcDcbSetIndexCount(dcb, 36)` wrote
    /// `0xc0001300` then `0x24`, advancing 8 bytes - and its own `GetSize` answered 8 in the same
    /// run, so the reservation and the packet agree.
    #[must_use]
    pub fn set_index_count(indices: u32) -> [u32; 2] {
        [command_header(measured::INDEX_BUFFER_SIZE, 1), indices]
    }

    /// A `NUM_INSTANCES` setting the instance count for subsequent draws. Two dwords.
    ///
    /// **Measured whole** (`166-agc/dcb-set-num-instances`): `sceAgcDcbSetNumInstances(dcb, 1)`
    /// wrote `0xc0002f00` then `1`, advancing 8 bytes.
    #[must_use]
    pub fn set_num_instances(instances: u32) -> [u32; 2] {
        [command_header(measured::NUM_INSTANCES, 1), instances]
    }

    /// A `DRAW_INDEX_AUTO` - a draw whose indices are generated rather than fetched. Three dwords.
    ///
    /// **Measured whole** (`166-agc/dcb-draw-auto`): `sceAgcDcbDrawIndexAuto(dcb, 3, 2)` wrote
    /// `0xc0012d00`, `3`, `2`, advancing 12 bytes. Both arguments appear in order.
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
    /// **Measured whole** (`166-agc/dcb-set-index-buffer`): `sceAgcDcbSetIndexBuffer(dcb,
    /// 0x12345678)` wrote `0xc0012600`, `0x12345678`, `0`, advancing 12 bytes - the address split
    /// low half first.
    #[must_use]
    pub fn set_index_base(address: u64) -> [u32; 3] {
        let lo = u32::try_from(address & 0xffff_ffff).unwrap_or_default();
        let hi = u32::try_from(address >> 32).unwrap_or_default();
        [command_header(measured::INDEX_BASE, 2), lo, hi]
    }

    /// A `SET_CONTEXT_REG` writing one value into one context register. Three dwords.
    ///
    /// **Measured whole** (`166-agc/dcb-set-cx-reg`): the builder takes the register offset and the
    /// value packed into one quadword as `(value << 32) | offset`, and wrote `0xc0016900`, `0x200`,
    /// `0x12345678` - the pair unpacked into the packet in that order, advancing 12 bytes.
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
    /// **Measured whole** (`166-agc/dcb-set-uc-reg`): same packed-quadword argument as
    /// [`set_context_register`]; `(4 << 32) | 0x242` wrote `0xc0017900`, `0x242`, `4`, advancing 12
    /// bytes. Its own `GetSize` answered 12 in the same run.
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
    /// **Measured whole** (`166-agc/dcb-set-sh-reg-direct`):
    /// `sceAgcCbSetShRegisterRangeDirect(cb, 0x08, values, 2)` wrote `0xc0027600`, `8`,
    /// `0x12345678`, `0x9abcdef0` - one header, one selector, one dword per register - advancing 16
    /// bytes. So `n` registers cost `n + 2` dwords.
    ///
    /// **This refutes a specific published claim.** Another implementation gives `n + 4` for this
    /// builder and states that the real library prepends a two-dword marker. The measured packet
    /// begins with the register write itself and carries no marker (worklog 534, D683).
    ///
    /// Returns an empty vector for an empty run: a zero-register write is not a packet, and
    /// `command_header` would have to claim a body dword that does not exist.
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

    /// A `DRAW_INDEX_2` - an indexed draw from a bound index buffer. Six dwords.
    ///
    /// **Three of the five body dwords are measured; two are transcribed.**
    /// `sceAgcDcbDrawIndex(dcb, 3, 0x12345678, 0)` wrote header `0xc0042700` and advanced 24 bytes,
    /// and the check read back the address low half, its high half, and the index count at body
    /// positions 1, 2 and 3 (`166-agc/dcb-draw-index`). It did **not** read positions 0 and 4, so
    /// `max_size` and `initiator` are placed by the published PM4 field order rather than by
    /// measurement, and are named here so a caller supplies them rather than inheriting a zero
    /// somebody invented.
    ///
    /// The size is not in doubt - 24 bytes, measured, against another implementation's 7 dwords
    /// (worklog 534).
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

    /// Builds a type-3 command header. **Generated, never extracted** (D051).
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
        // The single easiest thing to get wrong here. Without the adjustment every
        // packet is short by one dword and the walk desynchronises on packet two.
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
        // Count all ones: the packet is its header. The console's no-op builder writes
        // exactly this word and nothing after it (obSCEne 166-agc/cb-nop).
        let bytes = stream(&[0xFFFF_1000, command(0x05, 1), 0xDEAD_BEEF]);
        let result = walk(&bytes);
        assert_eq!(result.packets[0].kind, PacketKind::Command { opcode: 0x10 });
        assert_eq!(result.packets[0].length, 4);
        assert_eq!(result.packets[1].kind, PacketKind::Command { opcode: 0x05 });
        assert!(result.is_trustworthy());
    }

    #[test]
    fn filler_carries_no_body() {
        // Filler has no count field to honour; treating it as though it did would
        // swallow whatever follows.
        let bytes = stream(&[0x8000_0000, command(0x05, 1), 0xDEAD_BEEF]);
        let result = walk(&bytes);
        assert_eq!(result.packets[0].kind, PacketKind::Filler);
        assert_eq!(result.packets[0].length, 4);
        assert_eq!(result.packets[1].kind, PacketKind::Command { opcode: 0x05 });
        assert!(result.is_trustworthy());
    }

    #[test]
    fn a_reserved_packet_type_desynchronises_the_walk() {
        // Its length is undefined, so the next packet's position is a guess. Saying so
        // is what stops a reader trusting the rest of the walk.
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
        // Reports get diffed between runs; hash ordering would make every one differ.
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
        // A guest legitimately submits an empty buffer. Treating it as malformed would
        // fill a report with noise.
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

    /// **A header the builder writes is a header the walker reads back as the same packet.**
    ///
    /// The write side and the read side share one format, and this is the guard that they agree:
    /// the builder's `command_header` must equal the independently-computed one above, and a
    /// dispatch it builds must walk back to a single `DISPATCH_DIRECT` command of the right length.
    /// If they ever drift, a submission orbistoun handed the guest would desynchronise its own walk.
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
}
