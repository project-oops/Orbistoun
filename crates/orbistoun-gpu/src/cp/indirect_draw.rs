//! Indirect draws: `DRAW_INDIRECT`, `DRAW_INDEX_INDIRECT`, `DRAW_INDIRECT_MULTI` and
//! `DRAW_INDEX_INDIRECT_MULTI`, whose arguments the command processor reads from memory when it
//! reaches them.
//!
//! Each record is read past the base the last `SET_BASE` before the packet set (base index 1, as
//! radeonsi writes it before every indirect draw, `si_state_draw.cpp:1584-1587`) by the packet's
//! offset, plus the stride for each further draw of a multi form. The record is Mesa's: four words
//! (vertex count, instance count, first vertex, first instance), or five for an indexed draw (index
//! count, instance count, first index, vertex offset, first instance), the layout radeonsi and RADV
//! hand the packets (`pipe_draw_indirect_info`, `VkDrawIndexedIndirectCommand`).
//!
//! The command processor writes the first vertex (or vertex offset), the first instance, and for a
//! multi form the draw's index into the shader registers at the packet's locations, then draws:
//! `count` auto-generated vertices, or `count` indices from the first index on in the bound index
//! buffer, `instances` times (`cp_pm4_table_data_gfx11.json` `DRAW_INDIRECT`..
//! `DRAW_INDEX_INDIRECT_MULTI`). A location of 0 writes nothing: RADV gives it for a register the
//! shader does not read (`radv_cmd_buffer.c:11400-11403`). Each draw is carried out as those direct
//! packets - `SET_SH_REG`, `NUM_INSTANCES`, then `DRAW_INDEX_AUTO` or `DRAW_INDEX_OFFSET_2` - after
//! the stream's own packets before it, through [`CpMemory::run_draws`], so it is drawn exactly as a
//! direct draw with the same state is.
//!
//! What a draw leaves in those registers and in the instance count is not carried into the direct
//! draws after it; radeonsi writes both again before its next direct draw
//! (`si_state_draw.cpp:1577-1581`).

use super::{CpExecution, CpMemory, DrawSegment, Stop, address, indirect_base};
use crate::packet::{self, PacketKind, build::measured};

/// `PKT3_DRAW_INDIRECT_MULTI` (`sid.h:66`).
pub const DRAW_INDIRECT_MULTI: u8 = 0x2c;
/// `PKT3_DRAW_INDEX_INDIRECT_MULTI` (`sid.h:87`).
pub const DRAW_INDEX_INDIRECT_MULTI: u8 = 0x38;

/// `DRAW_INDEX_OFFSET_2` (`sid.h` `0x35`): body `[max_size, index_offset, index_count, initiator]`.
const DRAW_INDEX_OFFSET_2: u8 = 0x35;

/// `start_indx_enable`: bit 28 of `DRAW_INDEX_INDIRECT`'s first-instance word and of the multi
/// forms' draw-index word - the first index is written to `start_indx_loc` too.
const START_INDEX_ENABLE: u32 = 1 << 28;
/// The multi forms' draw-index word: `task_shader_mode` (25), `use_vgprs` (27), `count_indirect_enable`
/// (30) and `draw_index_enable` (31).
const TASK_SHADER_MODE: u32 = 1 << 25;
const USE_VGPRS: u32 = 1 << 27;
const COUNT_INDIRECT_ENABLE: u32 = 1 << 30;
const DRAW_INDEX_ENABLE: u32 = 1 << 31;

/// Whether `opcode` is one of the four indirect draws.
pub(super) const fn is_indirect_draw(opcode: u8) -> bool {
    matches!(
        opcode,
        measured::DRAW_INDIRECT
            | measured::DRAW_INDEX_INDIRECT
            | DRAW_INDIRECT_MULTI
            | DRAW_INDEX_INDIRECT_MULTI
    )
}

/// How many draws a packet asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Count {
    /// The packet's own count.
    Packet(u32),
    /// The u32 at `address`, at most `max`.
    Memory { max: u32, address: u64 },
}

/// An indirect draw packet's fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndirectDraw {
    indexed: bool,
    data_offset: u32,
    /// Where the first vertex (non-indexed) or vertex offset (indexed) is written.
    vertex_location: u32,
    /// Where the first index is written, when it is.
    index_location: Option<u32>,
    instance_location: u32,
    /// Where the draw's index is written, when it is.
    draw_index_location: Option<u32>,
    count: Count,
    /// Bytes between records; the single forms read one.
    stride: u32,
    initiator: u32,
}

impl IndirectDraw {
    /// The packet's fields, from its body by the Mesa layout. A multi form that hands its draw
    /// index over in vector registers or drives a task shader is not modelled and needs the GPU, as
    /// does any packet that is not an indirect draw.
    fn decode(opcode: u8, body: &[u32]) -> Result<Self, Stop> {
        match (opcode, body) {
            (measured::DRAW_INDIRECT, &[data_offset, vertex, instance, initiator]) => Ok(Self {
                indexed: false,
                data_offset,
                vertex_location: vertex & 0xffff,
                index_location: None,
                instance_location: instance & 0xffff,
                draw_index_location: None,
                count: Count::Packet(1),
                stride: 0,
                initiator,
            }),
            (measured::DRAW_INDEX_INDIRECT, &[data_offset, locations, instance, initiator]) => {
                Ok(Self {
                    indexed: true,
                    data_offset,
                    vertex_location: locations & 0xffff,
                    index_location: (instance & START_INDEX_ENABLE != 0).then_some(locations >> 16),
                    instance_location: instance & 0xffff,
                    draw_index_location: None,
                    count: Count::Packet(1),
                    stride: 0,
                    initiator,
                })
            }
            (
                DRAW_INDIRECT_MULTI | DRAW_INDEX_INDIRECT_MULTI,
                &[
                    data_offset,
                    locations,
                    instance,
                    draw_index,
                    count,
                    count_low,
                    count_high,
                    stride,
                    initiator,
                ],
            ) => {
                if draw_index & (TASK_SHADER_MODE | USE_VGPRS) != 0 {
                    return Err(Stop::NeedsGpu);
                }
                let indexed = opcode == DRAW_INDEX_INDIRECT_MULTI;
                Ok(Self {
                    indexed,
                    data_offset,
                    vertex_location: locations & 0xffff,
                    index_location: (indexed && draw_index & START_INDEX_ENABLE != 0)
                        .then_some(locations >> 16),
                    instance_location: instance & 0xffff,
                    draw_index_location: (draw_index & DRAW_INDEX_ENABLE != 0)
                        .then_some(draw_index & 0xffff),
                    count: if draw_index & COUNT_INDIRECT_ENABLE != 0 {
                        Count::Memory {
                            max: count,
                            address: address(count_low & !0x3, count_high),
                        }
                    } else {
                        Count::Packet(count)
                    },
                    stride,
                    initiator,
                })
            }
            _ if is_indirect_draw(opcode) => Err(Stop::Malformed),
            _ => Err(Stop::NeedsGpu),
        }
    }

    /// Bytes in one record.
    const fn record_bytes(&self) -> usize {
        if self.indexed { 20 } else { 16 }
    }
}

/// The last value an `INDEX_BUFFER_SIZE` before byte `before` declared, zero when none did: what
/// a synthesised `DRAW_INDEX_OFFSET_2` carries as its `max_size`. Nothing that draws it reads the
/// field.
fn index_buffer_size(stream: &[u8], before: u32) -> u32 {
    packet::walk(stream)
        .packets
        .iter()
        .take_while(|packet| packet.offset < before)
        .filter(|packet| {
            matches!(packet.kind, PacketKind::Command { opcode } if opcode == measured::INDEX_BUFFER_SIZE)
        })
        .filter_map(|packet| {
            let start = packet.body_offset() as usize;
            stream
                .get(start..start + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes)
        })
        .last()
        .unwrap_or(0)
}

fn read_u32(memory: &dyn CpMemory, at: u64) -> Result<u32, Stop> {
    let bytes = memory.read(at, 4).ok_or(Stop::OutOfBounds(at))?;
    Ok(u32::from_le_bytes(
        bytes.try_into().map_err(|_| Stop::Malformed)?,
    ))
}

/// The direct packets one record is drawn as, appended to `out`; nothing for a record that draws
/// nothing.
fn draw_words(
    packet: &IndirectDraw,
    record: &[u32],
    draw_index: u32,
    max_size: u32,
    out: &mut Vec<u32>,
) -> bool {
    let (count, instances, first, vertex, first_instance) = match *record {
        [count, instances, first_vertex, first_instance] => {
            (count, instances, 0, first_vertex, first_instance)
        }
        [count, instances, first_index, vertex_offset, first_instance] => {
            (count, instances, first_index, vertex_offset, first_instance)
        }
        _ => return false,
    };
    if count == 0 || instances == 0 {
        return false;
    }
    let mut set = |location: u32, value: u32| {
        if location != 0 {
            out.extend(packet::build::set_sh_register_range(
                location as u16,
                &[value],
            ));
        }
    };
    set(packet.vertex_location, vertex);
    if let Some(location) = packet.index_location {
        set(location, first);
    }
    set(packet.instance_location, first_instance);
    if let Some(location) = packet.draw_index_location {
        set(location, draw_index);
    }
    out.extend(packet::build::set_num_instances(instances));
    if packet.indexed {
        out.extend([
            packet::build::command_header(DRAW_INDEX_OFFSET_2, 4),
            max_size,
            first,
            count,
            packet.initiator,
        ]);
    } else {
        out.extend(packet::build::draw_index_auto(count, packet.initiator));
    }
    true
}

/// Carries out the indirect draw at byte `offset` of `stream`, its body `body`; any other packet
/// needs the GPU.
pub(super) fn run(
    stream: &[u8],
    offset: u32,
    opcode: u8,
    body: &[u32],
    memory: &mut dyn CpMemory,
    result: &mut CpExecution,
) -> Result<(), Stop> {
    let draw = IndirectDraw::decode(opcode, body)?;
    let base = indirect_base(stream, offset).ok_or(Stop::Malformed)? + u64::from(draw.data_offset);
    let count = match draw.count {
        Count::Packet(count) => count,
        Count::Memory { max, address } => read_u32(memory, address)?.min(max),
    };
    let max_size = if draw.indexed {
        index_buffer_size(stream, offset)
    } else {
        0
    };
    let mut words = Vec::new();
    let mut drawn = 0;
    for index in 0..count {
        let at = base + u64::from(index) * u64::from(draw.stride);
        let bytes = memory
            .read(at, draw.record_bytes())
            .ok_or(Stop::OutOfBounds(at))?;
        let record: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        if draw_words(&draw, &record, index, max_size, &mut words) {
            drawn += 1;
        }
    }
    if drawn == 0 {
        return Ok(());
    }
    let mut drawn_stream = stream
        .get(..offset as usize)
        .ok_or(Stop::Malformed)?
        .to_vec();
    drawn_stream.extend(words.iter().flat_map(|word| word.to_le_bytes()));
    let segment = DrawSegment {
        stream: &drawn_stream,
        first: offset,
        end: u32::try_from(drawn_stream.len()).map_err(|_| Stop::Malformed)?,
        whole: false,
    };
    if !memory.run_draws(&segment) {
        return Err(Stop::NeedsGpu);
    }
    result.draws += drawn;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DRAW_INDEX_INDIRECT_MULTI, DRAW_INDIRECT_MULTI};
    use crate::cp::{CpMemory, DrawSegment, Stopped, execute};
    use crate::packet::{
        self, PacketKind,
        build::{
            command_header, measured, set_base_indirect_args, set_index_base, set_index_count,
            set_index_size,
        },
    };
    use crate::registers::{DrawCall, DrawKind, draw_calls};
    use std::collections::BTreeMap;

    /// Guest memory over `[0x1000, 0x20000)` whose draws are recorded, not drawn: each segment it
    /// is handed, as the direct draws and shader-register writes from its first packet on.
    #[derive(Default)]
    struct Recording {
        bytes: BTreeMap<u64, u8>,
        refuse: bool,
        segments: Vec<Drawn>,
    }

    /// One segment as the draw path would read it.
    #[derive(Debug, PartialEq, Eq)]
    struct Drawn {
        first: u32,
        draws: Vec<DrawCall>,
        /// `(location, value)` of each `SET_SH_REG` from the first packet on.
        registers: Vec<(u32, u32)>,
        /// Whether everything before `first` is the stream's own.
        prefix_kept: bool,
    }

    impl Recording {
        fn put(&mut self, at: u64, words: &[u32]) {
            for (i, byte) in words.iter().flat_map(|w| w.to_le_bytes()).enumerate() {
                self.bytes.insert(at + i as u64, byte);
            }
        }
    }

    thread_local! {
        static SUBMITTED: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// The single forms as a builder writes them with a modifier that names no register.
    fn draw_indirect(offset: u32) -> [u32; 5] {
        packet::build::draw_indirect(offset, unnamed())
    }

    fn draw_index_indirect(offset: u32) -> [u32; 5] {
        packet::build::draw_index_indirect(offset, unnamed())
    }

    fn unnamed() -> packet::build::IndirectModifier {
        packet::build::IndirectModifier::new(0x2000_0000).expect("measured")
    }

    impl CpMemory for Recording {
        fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
            (address >= 0x1000 && address + length as u64 <= 0x20000).then(|| {
                (0..length as u64)
                    .map(|i| self.bytes.get(&(address + i)).copied().unwrap_or(0))
                    .collect()
            })
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
            if !(address >= 0x1000 && address + bytes.len() as u64 <= 0x20000) {
                return false;
            }
            for (i, byte) in bytes.iter().enumerate() {
                self.bytes.insert(address + i as u64, *byte);
            }
            true
        }
        fn timestamp(&mut self) -> u64 {
            0
        }
        fn run_draws(&mut self, segment: &DrawSegment<'_>) -> bool {
            let stream = &segment.stream[..segment.end as usize];
            let walked = packet::walk(stream);
            let draws = draw_calls(&walked, stream)
                .into_iter()
                .filter(|draw| draw.packet_offset >= segment.first)
                .collect();
            let registers = walked
                .packets
                .iter()
                .filter(|p| p.offset >= segment.first)
                .filter(|p| {
                    matches!(p.kind, PacketKind::Command { opcode } if opcode == measured::SET_SH_REG)
                })
                .map(|p| {
                    let at = p.body_offset() as usize;
                    let word =
                        |i: usize| u32::from_le_bytes(stream[at + i * 4..at + i * 4 + 4].try_into().expect("word"));
                    (word(0), word(1))
                })
                .collect();
            let prefix_kept = SUBMITTED.with(|submitted| {
                let submitted = submitted.borrow();
                let first = segment.first as usize;
                submitted.get(..first) == stream.get(..first)
            });
            self.segments.push(Drawn {
                first: segment.first,
                draws,
                registers,
                prefix_kept,
            });
            !self.refuse
        }
    }

    fn bytes(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn run(stream: &[u32], memory: &mut Recording) -> crate::cp::CpExecution {
        let stream = bytes(stream);
        SUBMITTED.with(|submitted| *submitted.borrow_mut() = stream.clone());
        execute(&stream, memory)
    }

    fn at(stream: &[u32]) -> u32 {
        u32::try_from(stream.len() * 4).expect("small")
    }

    fn release(fence: u64) -> Vec<u32> {
        vec![
            command_header(measured::RELEASE_MEM, 7),
            0x0660_3514,
            1 << 29,
            fence as u32,
            (fence >> 32) as u32,
            0xbeef_cafe,
            0,
            0,
        ]
    }

    /// A `DRAW_INDIRECT` draws the vertex and instance counts of the record past the last
    /// `SET_BASE` by its offset, after writing the first vertex and first instance to the shader
    /// registers it names; the fence after it retires once it was drawn.
    #[test]
    fn a_draw_indirect_draws_the_record_past_the_base() {
        let mut memory = Recording::default();
        memory.put(0x4010, &[3, 2, 5, 7]);
        let mut stream = set_base_indirect_args(0, 0x4000).to_vec();
        let draw = at(&stream);
        stream.extend([
            command_header(measured::DRAW_INDIRECT, 4),
            0x10,
            0x4c,
            0x4e,
            2,
        ]);
        stream.extend(release(0x1000));

        let done = run(&stream, &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(done.draws, 1);
        assert_eq!(
            memory.segments,
            vec![Drawn {
                first: draw,
                draws: vec![DrawCall {
                    // Past the two register writes and the instance count.
                    packet_offset: draw + 32,
                    instances: 2,
                    kind: DrawKind::Auto { vertices: 3 },
                }],
                registers: vec![(0x4c, 5), (0x4e, 7)],
                prefix_kept: true,
            }]
        );
        assert_eq!(
            memory.read(0x1000, 4),
            Some(0xbeef_cafe_u32.to_le_bytes().to_vec())
        );
    }

    /// A `DRAW_INDEX_INDIRECT` draws the record's indices from its first index on in the bound
    /// index buffer, its vertex offset and first instance written to the registers it names, and
    /// with `start_indx_enable` the first index to `start_indx_loc` too.
    #[test]
    fn a_draw_index_indirect_draws_from_its_first_index() {
        let mut memory = Recording::default();
        // A vertex offset of -2.
        memory.put(0x4000, &[6, 1, 3, 0xffff_fffe, 4]);
        let mut stream = set_index_base(0x9000).to_vec();
        stream.extend(set_index_size(1, 0));
        stream.extend(set_index_count(36));
        stream.extend(set_base_indirect_args(0, 0x4000));
        let draw = at(&stream);
        stream.extend([
            command_header(measured::DRAW_INDEX_INDIRECT, 4),
            0,
            (0x50 << 16) | 0x4c,
            (1 << 28) | 0x4e,
            0,
        ]);

        let done = run(&stream, &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        let [drawn] = memory.segments.as_slice() else {
            panic!("one segment: {:?}", memory.segments)
        };
        assert_eq!(
            drawn.draws,
            vec![DrawCall {
                // Past the three register writes and the instance count.
                packet_offset: draw + 44,
                instances: 1,
                kind: DrawKind::Indexed {
                    indices: 6,
                    address: 0x9000 + 3 * 4
                },
            }]
        );
        assert_eq!(
            drawn.registers,
            vec![(0x4c, 0xffff_fffe), (0x50, 3), (0x4e, 4)]
        );
    }

    /// A location of 0 writes nothing, as RADV gives it for a register the shader does not read.
    #[test]
    fn a_location_of_zero_writes_no_register() {
        let mut memory = Recording::default();
        memory.put(0x4000, &[3, 1, 5, 7]);
        let mut stream = set_base_indirect_args(0, 0x4000).to_vec();
        stream.extend([command_header(measured::DRAW_INDIRECT, 4), 0, 0x4c, 0, 2]);

        let done = run(&stream, &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(memory.segments[0].registers, vec![(0x4c, 5)]);
    }

    /// A record with no vertices or no instances draws nothing and the stream goes on.
    #[test]
    fn a_record_that_draws_nothing_is_not_drawn() {
        for record in [[0, 2, 0, 0], [3, 0, 0, 0]] {
            let mut memory = Recording::default();
            memory.put(0x4000, &record);
            let mut stream = set_base_indirect_args(0, 0x4000).to_vec();
            stream.extend(draw_indirect(0));
            stream.extend(release(0x1000));

            let done = run(&stream, &mut memory);

            assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
            assert_eq!(done.draws, 0);
            assert!(memory.segments.is_empty());
            assert_eq!(
                memory.read(0x1000, 4),
                Some(0xbeef_cafe_u32.to_le_bytes().to_vec())
            );
        }
    }

    /// The multi forms draw as many records as the u32 at their count address says, at most their
    /// own count, a stride apart, each with its draw index written where the packet names; without
    /// `count_indirect_enable` the packet's own count is the count.
    #[test]
    fn a_multi_draw_reads_its_count_and_strides_through_its_records() {
        let mut memory = Recording::default();
        memory.put(0x5000, &[2]);
        memory.put(0x5004, &[9]);
        for (i, first) in [0u32, 10, 20, 30].into_iter().enumerate() {
            memory.put(0x4000 + i as u64 * 32, &[3, 1, first, 0, 0]);
        }
        let multi = |count: u32, address: u64, flags: u32| {
            vec![
                command_header(DRAW_INDEX_INDIRECT_MULTI, 9),
                0,
                0x4c,
                0x4e,
                flags | 0x4d,
                count,
                address as u32,
                (address >> 32) as u32,
                32,
                0,
            ]
        };
        for (packet, drawn) in [
            (multi(4, 0x5000, (1 << 30) | (1 << 31)), 2),
            (multi(4, 0x5004, (1 << 30) | (1 << 31)), 4),
            (multi(3, 0, 1 << 31), 3),
        ] {
            let mut memory = Recording {
                bytes: memory.bytes.clone(),
                ..Recording::default()
            };
            let mut stream = set_index_base(0x9000).to_vec();
            stream.extend(set_index_size(1, 0));
            stream.extend(set_base_indirect_args(0, 0x4000));
            stream.extend(packet);

            let done = run(&stream, &mut memory);

            assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
            assert_eq!(done.draws, drawn);
            let [segment] = memory.segments.as_slice() else {
                panic!("one segment: {:?}", memory.segments)
            };
            let firsts: Vec<u64> = segment
                .draws
                .iter()
                .map(|draw| match draw.kind {
                    DrawKind::Indexed { address, .. } => (address - 0x9000) / 4,
                    DrawKind::Auto { .. } => panic!("indexed"),
                })
                .collect();
            assert_eq!(firsts, [0, 10, 20, 30][..drawn]);
            let indices: Vec<u32> = segment
                .registers
                .iter()
                .filter(|(location, _)| *location == 0x4d)
                .map(|&(_, value)| value)
                .collect();
            assert_eq!(indices, (0..drawn as u32).collect::<Vec<_>>());
        }
    }

    /// A multi draw that hands its draw index over in vector registers, or drives a task shader,
    /// is not modelled and needs the GPU; so does one the executor refuses, whose fence stays
    /// unwritten.
    #[test]
    fn an_unmodelled_or_refused_indirect_draw_stops_the_stream() {
        for flags in [1 << 27, 1 << 25] {
            let mut memory = Recording::default();
            memory.put(0x4000, &[3, 1, 0, 0]);
            let mut stream = set_base_indirect_args(0, 0x4000).to_vec();
            let draw = at(&stream);
            stream.extend([
                command_header(DRAW_INDIRECT_MULTI, 9),
                0,
                0,
                0,
                flags,
                1,
                0,
                0,
                16,
                2,
            ]);
            let done = run(&stream, &mut memory);
            assert_eq!(
                done.stopped,
                Stopped::NeedsGpu {
                    offset: draw,
                    opcode: DRAW_INDIRECT_MULTI
                }
            );
        }

        let mut memory = Recording {
            refuse: true,
            ..Recording::default()
        };
        memory.put(0x4000, &[3, 1, 0, 0, 0]);
        let mut stream = set_base_indirect_args(0, 0x4000).to_vec();
        let draw = at(&stream);
        stream.extend(draw_index_indirect(0));
        stream.extend(release(0x1000));
        let done = run(&stream, &mut memory);
        assert_eq!(
            done.stopped,
            Stopped::NeedsGpu {
                offset: draw,
                opcode: measured::DRAW_INDEX_INDIRECT
            }
        );
        assert_eq!(memory.read(0x1000, 4), Some(vec![0; 4]), "no fence");
    }

    /// Arguments that cannot be read stop the stream at the draw, naming the address; a draw with
    /// no `SET_BASE` before it has no arguments to read.
    #[test]
    fn unreadable_arguments_stop_the_stream() {
        let mut memory = Recording::default();
        let mut stream = set_base_indirect_args(0, 0xdead_0000).to_vec();
        let draw = at(&stream);
        stream.extend(draw_indirect(0x10));
        let done = run(&stream, &mut memory);
        assert_eq!(
            done.stopped,
            Stopped::OutOfBounds {
                offset: draw,
                opcode: measured::DRAW_INDIRECT,
                address: 0xdead_0010
            }
        );

        let done = run(&draw_indirect(0), &mut memory);
        assert_eq!(done.stopped, Stopped::Malformed { offset: 0 });
    }

    /// An indirect draw is GPU work the stream's single direct segment is not all of.
    #[test]
    fn a_stream_with_an_indirect_draw_has_no_whole_segment() {
        let mut stream = vec![command_header(measured::DRAW_INDEX_AUTO, 2), 3, 2];
        stream.extend(set_base_indirect_args(0, 0x4000));
        stream.extend(draw_indirect(0));
        let stream = bytes(&stream);
        let walked = packet::walk(&stream);
        let segments = crate::cp::draw_segments(&stream, &walked.packets);
        assert_eq!(segments.len(), 1);
        assert!(!segments[0].whole);
    }
}
