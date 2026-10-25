//! Legacy transform feedback - "streamout" - on the vertex stage (D861).
//!
//! A legacy vertex shader whose `SPI_SHADER_PGM_RSRC2_VS.SO_EN` is set stores its outputs itself:
//! after its user data the wave is handed a word holding how many of its vertices to write
//! (`streamout_config`), where in the buffers the wave's first vertex goes (`streamout_write_index`)
//! and each enabled buffer's offset (`streamout_offset[n]`), and it stores with `buffer_store` through
//! a descriptor it loads itself (Mesa `ac_shader_args.h`, `si_state_streamout.c`). With the
//! rasterizer killed (`PA_CL_CLIP_CNTL.DX_RASTERIZATION_KILL`) its position and parameter exports
//! reach nothing, so the draw is exactly that program run as a compute wave over the draw's
//! vertices, with those words supplied - which is how it is carried out here.
//!
//! The command processor keeps each buffer's offset between draws; `STRMOUT_BUFFER_UPDATE` sets it
//! and stores it, and each streamout draw advances it by what it wrote.

use crate::dispatch::DispatchState;
use crate::registers::RegisterWrite;
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{ComputeInputs, PartialGroups};
use std::sync::Mutex;

/// `PKT3_STRMOUT_BUFFER_UPDATE` (Mesa `sid.h`).
pub const STRMOUT_BUFFER_UPDATE: u8 = 0x34;

/// How many streamout buffers there are.
pub const BUFFERS: usize = 4;

/// `VGT_STRMOUT_CONFIG` (`gfx10.json`, context dword `0xA2E5`): `STREAMOUT_n_EN` in bits 0-3.
pub(crate) const VGT_STRMOUT_CONFIG: u32 = 0xA2E5;
/// `VGT_STRMOUT_BUFFER_CONFIG` (`0xA2E6`): `STREAM_k_BUFFER_EN` in bits `4k..4k+3`.
const VGT_STRMOUT_BUFFER_CONFIG: u32 = 0xA2E6;
/// `VGT_STRMOUT_BUFFER_SIZE_0` (`0xA2B4`), in dwords; buffer `n`'s is `4n` further on.
const BUFFER_SIZE_0: u32 = 0xA2B4;
/// `VGT_STRMOUT_VTX_STRIDE_0` (`0xA2B5`), in dwords; buffer `n`'s is `4n` further on.
const VTX_STRIDE_0: u32 = 0xA2B5;
/// `PA_CL_CLIP_CNTL.DX_RASTERIZATION_KILL`, bit 22.
const RASTERIZATION_KILL: u32 = 1 << 22;
/// `VGT_SHADER_STAGES_EN` (`0xA2D5`).
const VGT_SHADER_STAGES_EN: u32 = 0xA2D5;
/// Its `PRIMGEN_EN`, bit 13: the vertex stage runs as a primitive shader, which stores its
/// streamout through another contract.
const PRIMGEN_EN: u32 = 1 << 13;
/// Its `VS_W32_EN`, bit 23: the vertex stage's waves are thirty-two lanes wide.
const VS_W32_EN: u32 = 1 << 23;
/// Its stage enables besides the vertex stage's: `LS_EN` (1:0), `HS_EN` (2), `ES_EN` (4:3),
/// `GS_EN` (5) - a tessellation or geometry stage before the vertex stage's outputs.
const OTHER_STAGES: u32 = 0x3F;
/// `VGT_PRIMITIVE_TYPE` (uconfig `0xC242`).
const VGT_PRIMITIVE_TYPE: u32 = 0xC242;
/// `VGT_INDX_OFFSET` (`0xA102`): added to each auto-generated vertex index.
const VGT_INDX_OFFSET: u32 = 0xA102;

/// `SPI_SHADER_PGM_LO_VS` and `_HI_VS` (`0x2C48`, `0x2C49`).
const PGM_LO_VS: u32 = 0x2C48;
const PGM_HI_VS: u32 = 0x2C49;
/// `SPI_SHADER_PGM_RSRC1_VS` (`0x2C4A`).
const PGM_RSRC1_VS: u32 = 0x2C4A;
/// `SPI_SHADER_PGM_RSRC2_VS` (`0x2C4B`).
const PGM_RSRC2_VS: u32 = 0x2C4B;
/// `SPI_SHADER_USER_DATA_VS_0` (`0x2C4C`).
const USER_DATA_VS_0: u32 = 0x2C4C;
/// The vertex stage's user-data registers.
const USER_DATA_VS_WORDS: u32 = 32;

/// `RSRC1_VS.VGPR_COMP_CNT`, bits 25:24: the vertex inputs past the vertex id.
const VGPR_COMP_CNT: u32 = 0x3 << 24;
/// `RSRC1_VS.DX10_CLAMP`, bit 21.
const DX10_CLAMP: u32 = 1 << 21;
/// `RSRC2_VS.USER_SGPR`, bits 5:1.
const USER_SGPR_SHIFT: u32 = 1;
const USER_SGPR_MASK: u32 = 0x1F;
/// `RSRC2_VS.SO_BASE0_EN`, bit 8; buffer `n`'s is bit `8 + n`.
const SO_BASE0_EN: u32 = 8;
/// `RSRC2_VS.SO_EN`, bit 12.
const SO_EN: u32 = 1 << 12;
/// The `RSRC2_VS` fields carried out: `USER_SGPR`, the `SO_BASEn_EN` and `SO_EN`. Anything else -
/// scratch, a trap handler, exceptions - is refused.
const RSRC2_MODELLED: u32 = (USER_SGPR_MASK << USER_SGPR_SHIFT) | (0xF << SO_BASE0_EN) | SO_EN;

/// Where an update takes a buffer's offset from (`STRMOUT_OFFSET_SOURCE`, control bits 2:1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetSource {
    /// From the packet: the body's fourth word.
    Packet(u32),
    /// From memory: the 32-bit filled size, in bytes, at this address.
    Memory(u64),
    /// Kept as it is.
    Keep,
}

/// One `STRMOUT_BUFFER_UPDATE`: which buffer, where its offset comes from, and where its filled
/// size is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Update {
    /// `SELECT_BUFFER`, control bits 9:8.
    pub buffer: usize,
    /// `OFFSET_SOURCE`.
    pub source: OffsetSource,
    /// `STORE_BUFFER_FILLED_SIZE`, control bit 0: the filled size in bytes is written here.
    pub store_to: Option<u64>,
}

/// Reads a `STRMOUT_BUFFER_UPDATE` body: `[control, dst_lo, dst_hi, offset or src_lo, src_hi]`
/// (Mesa `si_state_streamout.c` `si_emit_streamout_begin` and `_end`).
///
/// # Errors
///
/// A short body; an offset from the streamout unit's own filled size (`FROM_VGT_FILLED_SIZE`),
/// which nothing here keeps apart from the offset; a store together with a new offset, whose
/// order is not known; and a packet offset other than zero, whose unit (`DATA_TYPE`, bit 7) is
/// not established.
pub fn decode_update(body: &[u32]) -> Result<Update, &'static str> {
    let [control, dst_low, dst_high, low, high, ..] = *body else {
        return Err("a STRMOUT_BUFFER_UPDATE body shorter than five words");
    };
    let store = control & 1 != 0;
    let source = match (control >> 1) & 0x3 {
        0 if low == 0 => OffsetSource::Packet(0),
        0 => {
            return Err(concat!(
                "a streamout offset from the packet other than zero, whose unit (DATA_TYPE) is ",
                "not established"
            ));
        }
        1 => {
            return Err("a streamout offset from the streamout unit's filled size is not modelled");
        }
        2 => OffsetSource::Memory(u64::from(low & !0x3) | (u64::from(high) << 32)),
        _ => OffsetSource::Keep,
    };
    if store && source != OffsetSource::Keep {
        return Err("a streamout update that both stores the filled size and sets the offset");
    }
    Ok(Update {
        buffer: ((control >> 8) & 0x3) as usize,
        source,
        store_to: store.then(|| u64::from(dst_low & !0x3) | (u64::from(dst_high) << 32)),
    })
}

/// Each streamout buffer's offset, in bytes, as the command processor holds it between draws;
/// `None` for a buffer no update has set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Offsets(pub [Option<u32>; BUFFERS]);

/// Each stream's primitive counters, as `SAMPLE_STREAMOUTSTATS` samples them: the primitives
/// written to its buffers, and the primitives its draws needed room for - counting up from the
/// first streamout draw, a query reading the difference between two samples.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// `NumPrimitivesWritten`, a stream each.
    pub written: [u64; BUFFERS],
    /// `PrimitiveStorageNeeded`, a stream each.
    pub needed: [u64; BUFFERS],
}

/// The counters the queue holds, kept across submissions as the hardware keeps them.
pub fn stats() -> &'static Mutex<Stats> {
    static STATS: Mutex<Stats> = Mutex::new(Stats {
        written: [0; BUFFERS],
        needed: [0; BUFFERS],
    });
    &STATS
}

/// The stream an `EVENT_WRITE` samples the counters of, where it is a `SAMPLE_STREAMOUTSTATS`:
/// event type 32 for stream 0 and 1-3 for the others, at event index 3 (`gfx10.json`
/// `VGT_EVENT_TYPE`; radeonsi `emit_sample_streamout`).
#[must_use]
pub const fn stats_event_stream(initiator: u32) -> Option<usize> {
    if (initiator >> 8) & 0xF != 3 {
        return None;
    }
    match initiator & 0x3F {
        32 => Some(0),
        stream @ 1..=3 => Some(stream as usize),
        _ => None,
    }
}

/// The offsets the queue holds, kept across submissions as the hardware keeps them.
pub fn offsets() -> &'static Mutex<Offsets> {
    static OFFSETS: Mutex<Offsets> = Mutex::new(Offsets([None; BUFFERS]));
    &OFFSETS
}

/// One streamout draw ready to run: the vertex program as a compute wave, and the offsets once it
/// has written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The dispatches that carry the draw out, a wave each, in order.
    pub dispatches: Vec<DispatchState>,
    /// How many vertices it writes: the draw's, or fewer where a buffer fills.
    pub vertices: u32,
    /// The primitives written, and the primitives the draw needed room for: what the stream's
    /// counters move by.
    pub primitives: (u64, u64),
    /// Whether the draw is also rasterized: its streamout is made alongside drawing it (D881).
    pub rasterized: bool,
    /// The offsets after it.
    pub after: Offsets,
}

/// Vertices a primitive of `VGT_PRIMITIVE_TYPE` `kind` takes, for the list types; a strip, fan or
/// any other is refused, since its primitives share vertices.
const fn vertices_per_primitive(kind: u32) -> Option<u32> {
    match kind {
        1 => Some(1),
        2 => Some(2),
        4 => Some(3),
        _ => None,
    }
}

/// Whether the registers in force at `packet_offset` ask for streamout: any stream enabled.
#[must_use]
pub fn enabled_at(writes: &[RegisterWrite], packet_offset: u32) -> bool {
    latest_at(writes, packet_offset)(VGT_STRMOUT_CONFIG).is_some_and(|word| word & 0xF != 0)
}

fn latest_at(writes: &[RegisterWrite], packet_offset: u32) -> impl Fn(u32) -> Option<u32> + '_ {
    move |register| {
        writes
            .iter()
            .rev()
            .find(|w| crate::registers::in_force_at(w, packet_offset) && w.register == register)
            .map(|w| w.value)
    }
}

/// What a streamout draw's registers say about how it runs.
struct Shape {
    rasterized: bool,
    width: Width,
    lanes: u32,
    per_primitive: u32,
    /// `VGT_STRMOUT_BUFFER_CONFIG`: the buffers the first stream writes.
    buffers: u32,
}

/// The wave width, the vertices a primitive takes and the buffers written, or what is not
/// carried out ([`plan`]).
fn shape(latest: &impl Fn(u32) -> Option<u32>) -> Result<Shape, &'static str> {
    let stages = latest(VGT_SHADER_STAGES_EN).unwrap_or(0);
    if stages & PRIMGEN_EN != 0 {
        return Err("streamout from a primitive shader is not modelled");
    }
    if stages & OTHER_STAGES != 0 {
        return Err("streamout after a tessellation or geometry stage is not modelled");
    }
    let (width, lanes) = if stages & VS_W32_EN == 0 {
        (Width::Wave64, 64)
    } else {
        (Width::Wave32, 32)
    };
    if latest(VGT_INDX_OFFSET).unwrap_or(0) != 0 {
        return Err("a streamout draw with a base vertex index is not modelled");
    }
    let per_primitive = vertices_per_primitive(latest(VGT_PRIMITIVE_TYPE).unwrap_or(0))
        .ok_or("streamout of a strip, fan or other primitive type is not modelled")?;
    let buffers = latest(VGT_STRMOUT_BUFFER_CONFIG).unwrap_or(0);
    if buffers & !0xF != 0 {
        return Err("streamout to a stream other than the first is not modelled");
    }
    Ok(Shape {
        rasterized: latest(crate::depth::PA_CL_CLIP_CNTL).unwrap_or(0) & RASTERIZATION_KILL == 0,
        width,
        lanes,
        per_primitive,
        buffers,
    })
}

/// The legacy vertex program's address and its two resource words, or what is not carried out.
fn vertex_program(latest: &impl Fn(u32) -> Option<u32>) -> Result<(u64, u32, u32), &'static str> {
    let (Some(low), Some(high)) = (latest(PGM_LO_VS), latest(PGM_HI_VS)) else {
        return Err("SPI_SHADER_PGM_LO/HI_VS were never written, so there is no vertex program");
    };
    let program = (u64::from(low) << 8) | (u64::from(high & 0xFF) << 40);
    let rsrc1 = latest(PGM_RSRC1_VS).ok_or("SPI_SHADER_PGM_RSRC1_VS was never written")?;
    let rsrc2 = latest(PGM_RSRC2_VS).ok_or("SPI_SHADER_PGM_RSRC2_VS was never written")?;
    if rsrc1 & VGPR_COMP_CNT != 0 {
        return Err("a streamout vertex program reading inputs past the vertex id is not modelled");
    }
    if rsrc2 & !RSRC2_MODELLED != 0 {
        return Err("the vertex program asks for scratch, a trap handler or exceptions");
    }
    if rsrc2 & SO_EN == 0 {
        return Err("streamout is on but the vertex program takes no streamout words (SO_EN)");
    }
    Ok((program, rsrc1, rsrc2))
}

/// Plans the `DRAW_INDEX_AUTO` of `vertices` at `packet_offset` as a streamout pass (D861): the
/// legacy vertex program as compute waves of whole primitives, a thread per vertex, each with the
/// streamout words after its user data - its vertex count, its first vertex as the write index,
/// and the buffers' offsets.
///
/// # Errors
///
/// Each thing not carried out, by name: rasterization left on, a primitive shader, another stage,
/// a strip or fan, an offset or base index, a buffer no update set,
/// or a program field outside the modelled ones.
pub fn plan(
    writes: &[RegisterWrite],
    packet_offset: u32,
    vertices: u32,
    offsets: Offsets,
) -> Result<Plan, &'static str> {
    let latest = latest_at(writes, packet_offset);
    let Shape {
        rasterized,
        width,
        lanes,
        per_primitive,
        buffers,
    } = shape(&latest)?;
    let (program, rsrc1, rsrc2) = vertex_program(&latest)?;

    // Each enabled buffer's room, from its offset to its end, in whole primitives.
    let mut written = vertices / per_primitive * per_primitive;
    let mut strides = [0_u32; BUFFERS];
    for (n, stride) in strides.iter_mut().enumerate() {
        if buffers >> n & 1 == 0 {
            continue;
        }
        let register = |base: u32| base + 4 * n as u32;
        let size = latest(register(BUFFER_SIZE_0)).ok_or("a streamout buffer's size unwritten")?;
        *stride = latest(register(VTX_STRIDE_0)).ok_or("a streamout buffer's stride unwritten")?;
        let offset = offsets.0[n].ok_or("a streamout buffer whose offset no update set")?;
        if *stride == 0 {
            continue;
        }
        let room = size.saturating_sub(offset / 4) / (*stride * per_primitive) * per_primitive;
        written = written.min(room);
    }

    let user_sgprs = (rsrc2 >> USER_SGPR_SHIFT) & USER_SGPR_MASK;
    if user_sgprs > USER_DATA_VS_WORDS {
        return Err("more user SGPRs than there are vertex user-data registers");
    }
    let user_data: Vec<Option<u32>> = (0..user_sgprs)
        .map(|i| latest(USER_DATA_VS_0 + i))
        .collect();
    // Each base-enabled buffer's offset in dwords, as the draw starts: every wave's.
    let mut bases = Vec::new();
    for (n, offset) in offsets.0.iter().enumerate() {
        if rsrc2 >> (SO_BASE0_EN + n as u32) & 1 != 0 {
            bases.push(offset.ok_or("a streamout base enabled for an unset buffer")? / 4);
        }
    }
    // A wave of whole primitives at a time, as the geometry engine hands them out.
    let per_wave = lanes / per_primitive * per_primitive;
    let dispatches = (0..written)
        .step_by(per_wave.max(1) as usize)
        .map(|first| {
            let count = per_wave.min(written - first);
            // `streamout_config`: the wave's vertex count in bits 22:16, stream 0 in bits 25:24;
            // `streamout_write_index`: the wave's first vertex, counted from the draw's, whose
            // offsets carry what earlier draws wrote; then the buffers' offsets.
            let mut words = user_data.clone();
            words.push(Some(count << 16));
            words.push(Some(first));
            words.extend(bases.iter().map(|&base| Some(base)));
            let unwritten_user_data = words
                .iter()
                .enumerate()
                .filter(|(_, word)| word.is_none())
                .fold(0, |mask, (i, _)| mask | 1 << i);
            DispatchState {
                program,
                user_data: words,
                inputs: ComputeInputs {
                    workgroup_ids: [false; 3],
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 1,
                    threads: [lanes, 1, 1],
                    unwritten_user_data,
                    partial: Some(PartialGroups {
                        last: [0, 0, 0],
                        threads: [count, 1, 1],
                    }),
                    integer_images: [false; 2],
                    integer_fetched: 0,
                    // Its lanes' vertex ids, which its `v0` holds.
                    first_thread: first,
                    sampled_buffers: 0,
                    scratch_wave_bytes: 0,
                    device_addressed: false,
                    // The vertex stage's subnormals, from bits 19:12 of its `RSRC1`.
                    float_mode: u8::try_from((rsrc1 >> 12) & 0xff).ok(),
                },
                width,
                dx10_clamp: Some(rsrc1 & DX10_CLAMP != 0),
                local_words: None,
                groups: [1, 1, 1],
                vertex_streamout: true,
            }
        })
        .collect();

    let mut after = offsets;
    for (n, offset) in after.0.iter_mut().enumerate() {
        if let Some(offset) = offset
            && buffers >> n & 1 != 0
        {
            *offset += written * strides[n] * 4;
        }
    }
    Ok(Plan {
        dispatches,
        vertices: written,
        primitives: (
            u64::from(written / per_primitive),
            u64::from(vertices / per_primitive),
        ),
        rasterized,
        after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writes(pairs: &[(u32, u32)]) -> Vec<RegisterWrite> {
        pairs
            .iter()
            .map(|&(register, value)| RegisterWrite {
                packet_offset: 0,
                register,
                value,
            })
            .collect()
    }

    /// GCTS00001's streamout pass: one point, one buffer of one dword, a stride of one dword.
    fn one_point() -> Vec<(u32, u32)> {
        vec![
            (VGT_STRMOUT_CONFIG, 0xF),
            (VGT_STRMOUT_BUFFER_CONFIG, 0x1),
            (BUFFER_SIZE_0, 1),
            (VTX_STRIDE_0, 1),
            (crate::depth::PA_CL_CLIP_CNTL, 0x0140_0000),
            (VGT_SHADER_STAGES_EN, 0x0081_0000),
            (VGT_PRIMITIVE_TYPE, 1),
            (PGM_LO_VS, 0x0402_8004),
            (PGM_HI_VS, 0),
            (PGM_RSRC1_VS, 0x002C_0000),
            (PGM_RSRC2_VS, 0x1112),
        ]
    }

    /// The submitted pass becomes one wave32 compute wave at the vertex program, its nine user
    /// words then the vertex count, the write index and buffer 0's offset in dwords.
    #[test]
    fn gcts_streamout_pass_is_one_wave() {
        let mut pairs = one_point();
        pairs.extend((0..9).map(|i| (USER_DATA_VS_0 + i, 0x100 + i)));
        let at = Offsets([Some(0), None, None, None]);
        let plan = plan(&writes(&pairs), 0x40, 1, at).unwrap();
        assert_eq!(plan.dispatches[0].program, 0x4_0280_0400);
        assert_eq!(plan.dispatches[0].width, Width::Wave32);
        assert_eq!(plan.dispatches[0].user_data.len(), 12);
        assert_eq!(
            plan.dispatches[0].user_data[9..],
            [Some(1 << 16), Some(0), Some(0)]
        );
        assert_eq!(plan.dispatches[0].inputs.unwritten_user_data, 0);
        assert_eq!(plan.vertices, 1);
        assert_eq!(plan.after, Offsets([Some(4), None, None, None]));
    }

    /// A buffer with room for fewer primitives than the draw has writes that many, whole ones, and
    /// a full buffer writes none.
    #[test]
    fn a_full_buffer_clamps_the_vertex_count() {
        let mut pairs = one_point();
        pairs.push((VGT_PRIMITIVE_TYPE, 4));
        pairs.push((BUFFER_SIZE_0, 8));
        pairs.extend((0..9).map(|i| (USER_DATA_VS_0 + i, 0)));
        let ran = plan(
            &writes(&pairs),
            0x40,
            9,
            Offsets([Some(4), None, None, None]),
        )
        .unwrap();
        // Seven dwords of room at a dword a vertex: two triangles.
        assert_eq!(ran.vertices, 6);
        assert_eq!(ran.dispatches[0].user_data[9], Some(6 << 16));
        assert_eq!(ran.after.0[0], Some(28));
        let full = plan(&writes(&pairs), 0x40, 3, ran.after).unwrap();
        assert_eq!(full.vertices, 0);
    }

    /// A draw larger than a wave is carried out a wave of whole primitives at a time: each wave's
    /// count and write index its own, its lanes' vertex ids from its first, the buffers' offsets
    /// the draw's.
    #[test]
    fn a_draw_larger_than_a_wave_runs_a_wave_at_a_time() {
        let mut pairs = one_point();
        pairs.push((VGT_PRIMITIVE_TYPE, 4));
        pairs.push((BUFFER_SIZE_0, 1000));
        pairs.extend((0..9).map(|i| (USER_DATA_VS_0 + i, 0)));
        let ran = plan(
            &writes(&pairs),
            0x40,
            70,
            Offsets([Some(8), None, None, None]),
        )
        .unwrap();
        // Wave32 holds ten whole triangles: thirty vertices, thirty, then the last nine.
        let waves: Vec<(Option<u32>, Option<u32>, u32)> = ran
            .dispatches
            .iter()
            .map(|d| (d.user_data[9], d.user_data[10], d.inputs.first_thread))
            .collect();
        assert_eq!(
            waves,
            [
                (Some(30 << 16), Some(0), 0),
                (Some(30 << 16), Some(30), 30),
                (Some(9 << 16), Some(60), 60)
            ]
        );
        assert!(ran.dispatches.iter().all(|d| d.user_data[11] == Some(2)));
        assert_eq!(ran.vertices, 69);
        assert_eq!(ran.after.0[0], Some(8 + 69 * 4));
    }

    /// Every wave of every streamout draw of one program is one module: a wave's first vertex is
    /// read from the push block, not compiled in (D861), so draws with other first vertices
    /// translate nothing new and compile nothing new.
    #[test]
    fn every_wave_of_every_draw_shares_one_module() {
        use crate::pipeline::{GuestMemory, MAX_SHADER_BYTES, Pipeline};
        use orbistoun_translate::wavefront::FIRST_THREAD_PUSH_WORD;
        use orbistoun_translate::{Fidelity, Strategy};
        /// The vertex program, alone in guest memory.
        struct Program(Vec<u8>);
        impl GuestMemory for Program {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let at = usize::try_from(address.checked_sub(0x4_0280_0400)?).ok()?;
                self.0.get(at..at.checked_add(length)?)
            }
        }
        // v_mov_b32 v1, v0; s_endpgm; s_code_end: a program that reads its vertex id.
        let mut bytes = [0x7e02_0300_u32, 0xbf81_0000, 0xbf9f_0000]
            .map(u32::to_le_bytes)
            .concat();
        bytes.resize(MAX_SHADER_BYTES, 0);
        let memory = Program(bytes);
        let mut pairs = one_point();
        pairs.push((VGT_PRIMITIVE_TYPE, 4));
        pairs.push((BUFFER_SIZE_0, 1000));
        pairs.extend((0..9).map(|i| (USER_DATA_VS_0 + i, 0)));
        let mut pipeline = Pipeline::new(Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave32,
        })
        .expect("a pipeline");
        let (mut modules, mut firsts) = (std::collections::BTreeSet::new(), Vec::new());
        for (vertices, offset) in [(70, 8), (45, 400)] {
            let at = Offsets([Some(offset), None, None, None]);
            for dispatch in plan(&writes(&pairs), 0x40, vertices, at)
                .unwrap()
                .dispatches
            {
                let prepared = pipeline
                    .prepare_dispatch(&dispatch, &memory)
                    .expect("the wave prepares");
                modules.insert(prepared.module_hash);
                firsts.push(prepared.push[FIRST_THREAD_PUSH_WORD as usize]);
            }
        }
        assert_eq!(
            firsts,
            [0, 30, 60, 0, 30],
            "each wave's first vertex, pushed"
        );
        assert_eq!(modules.len(), 1, "five waves of two draws, one module");
    }

    /// A pass whose rasterizer is killed is streamout alone; one left rasterizing is drawn too
    /// (D881). Either moves the counters by the primitives written and those it needed room for.
    #[test]
    fn a_pass_says_whether_it_is_drawn_and_what_it_counts() {
        let mut pairs = one_point();
        pairs.extend((0..9).map(|i| (USER_DATA_VS_0 + i, 0)));
        let at = Offsets([Some(0), None, None, None]);
        let killed = plan(&writes(&pairs), 0x40, 3, at).unwrap();
        // One dword of room: one point written of the three.
        assert_eq!((killed.rasterized, killed.primitives), (false, (1, 3)));
        pairs.push((crate::depth::PA_CL_CLIP_CNTL, 0));
        assert!(plan(&writes(&pairs), 0x40, 3, at).unwrap().rasterized);
        assert_eq!(stats_event_stream(0x320), Some(0));
        assert_eq!(stats_event_stream(0x302), Some(2));
        assert_eq!(
            stats_event_stream(0x115),
            None,
            "ZPASS_DONE is no streamout sample"
        );
        assert_eq!(stats_event_stream(0x120), None, "only at event index 3");
    }

    /// What is not carried out is refused by name.
    #[test]
    fn unmodelled_streamout_is_refused() {
        let at = Offsets([Some(0), None, None, None]);
        let refused = |extra: (u32, u32), vertices| {
            let mut pairs = one_point();
            pairs.push(extra);
            plan(&writes(&pairs), 0x40, vertices, at).unwrap_err()
        };
        assert!(refused((VGT_SHADER_STAGES_EN, 1 << 13), 1).contains("primitive shader"));
        assert!(refused((VGT_PRIMITIVE_TYPE, 6), 3).contains("strip"));
        assert!(refused((VGT_INDX_OFFSET, 5), 1).contains("base vertex"));
        assert!(refused((PGM_RSRC2_VS, 0x1113), 1).contains("scratch"));
        let unset = plan(&writes(&one_point()), 0x40, 1, Offsets::default()).unwrap_err();
        assert!(unset.contains("no update set"));
    }

    /// radeonsi's begin (offset 0 from the packet), append (from memory) and end (keep and store)
    /// decode; the filled-size source and a non-zero packet offset are refused.
    #[test]
    fn buffer_updates_decode() {
        assert_eq!(
            decode_update(&[0x80, 0, 0, 0, 0]),
            Ok(Update {
                buffer: 0,
                source: OffsetSource::Packet(0),
                store_to: None
            })
        );
        assert_eq!(
            decode_update(&[0x87, 0x0300_0000, 0x4, 0, 0]),
            Ok(Update {
                buffer: 0,
                source: OffsetSource::Keep,
                store_to: Some(0x4_0300_0000)
            })
        );
        assert_eq!(
            decode_update(&[0x184, 0, 0, 0x2000, 0x4]).map(|u| (u.buffer, u.source)),
            Ok((1, OffsetSource::Memory(0x4_0000_2000)))
        );
        assert!(decode_update(&[0x82, 0, 0, 0, 0]).is_err());
        assert!(decode_update(&[0x80, 0, 0, 4, 0]).is_err());
        assert!(decode_update(&[0x81, 0, 0, 0, 0]).is_err());
    }
}
