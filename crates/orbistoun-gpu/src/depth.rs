//! Depth, stencil and cull state: the registers a draw's depth target and rasteriser read, and the
//! commands that carry them to a backend.
//!
//! Every register index and field is `src/amd/registers/gfx103.json` in the collection's Mesa tree:
//! a context register's index is `0xA000` plus its byte address less `0x28000`, over four. A depth
//! target is named by its write base and never read: the guest's depth tiling is unmeasured, so a
//! backend keeps a depth attachment of its own, and a clear reaches it as a value - from
//! `DB_RENDER_CONTROL` or from a `DMA_DATA` fill of the whole surface, which is the same value in
//! any layout.

use crate::backend::{RenderCommand, ResourceId};
use crate::cp::{Fill, fill_of};
use crate::packet::{PacketKind, PacketWalk, build::measured};
use crate::registers::{
    ColourTargetExtent, DB_DEPTH_CONTROL, DB_STENCIL_CONTROL, DepthControl, DrawCall,
    RegisterSweep, RegisterWrite, StencilControl, decode_depth_control, decode_stencil_control,
};

/// `DB_RENDER_CONTROL`, byte `163840` (`0x28000`).
const DB_RENDER_CONTROL: u32 = 0xA000;
/// `DB_STENCIL_CLEAR`, byte `163880` (`0x28028`).
const DB_STENCIL_CLEAR: u32 = 0xA00A;
/// `DB_DEPTH_CLEAR`, byte `163884` (`0x2802C`).
const DB_DEPTH_CLEAR: u32 = 0xA00B;
/// `DB_Z_INFO`, byte `163904` (`0x28040`).
const DB_Z_INFO: u32 = 0xA010;
/// `DB_STENCIL_INFO`, byte `163908` (`0x28044`).
const DB_STENCIL_INFO: u32 = 0xA011;
/// `DB_HTILE_DATA_BASE`, byte `163860` (`0x28014`): the HTILE metadata's address in 256-byte units.
const DB_HTILE_DATA_BASE: u32 = 0xA005;
/// `DB_HTILE_DATA_BASE_HI`, byte `163960` (`0x28078`).
const DB_HTILE_DATA_BASE_HI: u32 = 0xA01E;
/// `DB_Z_INFO`'s `TILE_SURFACE_ENABLE`, bit 29: the depth surface has HTILE metadata.
const TILE_SURFACE_ENABLE: u32 = 1 << 29;
/// An HTILE word's `ZMASK`, bits 3:0 in every layout: zero is a tile in the cleared state.
const HTILE_ZMASK: u32 = 0xF;
/// `DB_Z_WRITE_BASE`, byte `163920` (`0x28050`).
const DB_Z_WRITE_BASE: u32 = 0xA014;
/// `DB_STENCIL_WRITE_BASE`, byte `163924` (`0x28054`).
const DB_STENCIL_WRITE_BASE: u32 = 0xA015;
/// `DB_Z_WRITE_BASE_HI`, byte `163952` (`0x28070`).
const DB_Z_WRITE_BASE_HI: u32 = 0xA01C;
/// `DB_STENCIL_WRITE_BASE_HI`, byte `163956` (`0x28074`).
const DB_STENCIL_WRITE_BASE_HI: u32 = 0xA01D;
/// `DB_STENCILREFMASK`, byte `164912` (`0x28430`).
const DB_STENCILREFMASK: u32 = 0xA10C;
/// `DB_STENCILREFMASK_BF`, byte `164916` (`0x28434`).
const DB_STENCILREFMASK_BF: u32 = 0xA10D;
/// `PA_SU_SC_MODE_CNTL`, byte `165908` (`0x28814`).
const PA_SU_SC_MODE_CNTL: u32 = 0xA205;

/// A depth surface's element format: `DB_Z_INFO`'s `FORMAT` (bits 0:1), the `ZFormat` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepthFormat {
    /// `Z_INVALID` (0): no depth surface is bound.
    Invalid,
    /// `Z_16` (1): sixteen-bit unsigned normalised.
    Z16,
    /// `Z_24` (2).
    Z24,
    /// `Z_32_FLOAT` (3).
    Z32Float,
}

/// Decodes `DB_Z_INFO`'s format. Two bits, every value defined.
#[must_use]
pub const fn decode_depth_format(z_info: u32) -> DepthFormat {
    match z_info & 0x3 {
        0 => DepthFormat::Invalid,
        1 => DepthFormat::Z16,
        2 => DepthFormat::Z24,
        _ => DepthFormat::Z32Float,
    }
}

/// Whether `DB_STENCIL_INFO` names a stencil surface: `FORMAT` (bit 0), `StencilFormat`'s
/// `STENCIL_8` (1) rather than `STENCIL_INVALID` (0).
#[must_use]
pub const fn decode_stencil_format(stencil_info: u32) -> bool {
    stencil_info & 1 != 0
}

/// A depth or stencil surface's byte address from its base register pair.
///
/// The low register holds the address in 256-byte units and `BASE_HI` (bits 0:7 of the `_HI`
/// register) the bits above those 32: radeonsi writes `va >> 8` and then that value's high word
/// (`ac_descriptors.c:1022`, `si_state.c:3067-3072`).
#[must_use]
pub const fn surface_address(low: u32, high: u32) -> u64 {
    ((high as u64 & 0xFF) << 40) | ((low as u64) << 8)
}

/// The depth target a submission binds: what it holds and where the guest put it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DepthTarget {
    /// The depth element format.
    pub format: DepthFormat,
    /// Whether a stencil surface is bound beside it.
    pub stencil: bool,
    /// The depth surface's byte address (`DB_Z_WRITE_BASE`).
    pub base: u64,
    /// The stencil surface's byte address (`DB_STENCIL_WRITE_BASE`).
    pub stencil_base: u64,
    /// Where its HTILE metadata lies, when `DB_Z_INFO` enables it (D750).
    pub htile: Option<u64>,
    /// `DB_DEPTH_CLEAR`'s bits: the depth a tile HTILE marks cleared reads (D750).
    pub depth_clear: u32,
}

/// The depth target a stream binds, from the latest writes to its registers.
///
/// [`None`] when the stream never sets `DB_Z_INFO`, or sets it to `Z_INVALID`: no depth target is
/// guessed. An unwritten base register reads zero.
#[must_use]
pub fn depth_target_at(writes: &[RegisterWrite]) -> Option<DepthTarget> {
    let latest = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    };
    let format = decode_depth_format(latest(DB_Z_INFO)?);
    if format == DepthFormat::Invalid {
        return None;
    }
    let at = |register| latest(register).unwrap_or(0);
    let tiled = at(DB_Z_INFO) & TILE_SURFACE_ENABLE != 0;
    Some(DepthTarget {
        format,
        stencil: decode_stencil_format(at(DB_STENCIL_INFO)),
        base: surface_address(at(DB_Z_WRITE_BASE), at(DB_Z_WRITE_BASE_HI)),
        stencil_base: surface_address(at(DB_STENCIL_WRITE_BASE), at(DB_STENCIL_WRITE_BASE_HI)),
        htile: tiled.then(|| surface_address(at(DB_HTILE_DATA_BASE), at(DB_HTILE_DATA_BASE_HI))),
        depth_clear: at(DB_DEPTH_CLEAR),
    })
}

/// The id a backend keeps a depth target's attachment under: its depth surface's address.
///
/// Distinct surfaces are distinct attachments. Bit 62 keeps depth ids apart from colour targets'
/// (bit 63) and shaders' (counting up from one); an address in 256-byte units fits in 40 bits.
#[must_use]
pub const fn depth_target_id(target: &DepthTarget) -> ResourceId {
    const DEPTH_NAMESPACE: u64 = 1 << 62;
    ResourceId(DEPTH_NAMESPACE | ((target.base >> 8) & 0xFF_FFFF_FFFF))
}

/// `DB_RENDER_CONTROL`'s clear enables: the draw writes the clear value rather than its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderControl {
    /// `DEPTH_CLEAR_ENABLE` (bit 0): the draw writes `DB_DEPTH_CLEAR`.
    pub depth_clear: bool,
    /// `STENCIL_CLEAR_ENABLE` (bit 1): the draw writes `DB_STENCIL_CLEAR`.
    pub stencil_clear: bool,
}

/// Decodes `DB_RENDER_CONTROL`'s two clear enables; radeonsi sets them for a clear
/// (`si_state.c:1728`).
#[must_use]
pub const fn decode_render_control(value: u32) -> RenderControl {
    RenderControl {
        depth_clear: value & 1 != 0,
        stencil_clear: value & 2 != 0,
    }
}

/// `DB_DEPTH_CLEAR`: the clear value, a 32-bit float's bits (radeonsi writes `fui(depth)`,
/// `si_state.c:2930`).
#[must_use]
pub const fn decode_depth_clear(value: u32) -> f32 {
    f32::from_bits(value)
}

/// `DB_STENCIL_CLEAR`'s `CLEAR` (bits 0:7).
#[must_use]
pub const fn decode_stencil_clear(value: u32) -> u8 {
    (value & 0xFF) as u8
}

/// One face's stencil reference and masks, from `DB_STENCILREFMASK` or its `_BF` twin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StencilRefMask {
    /// `STENCILTESTVAL` (bits 0:7): the reference the test compares against, and a replace writes.
    pub reference: u8,
    /// `STENCILMASK` (bits 8:15): the bits the comparison reads.
    pub compare_mask: u8,
    /// `STENCILWRITEMASK` (bits 16:23): the bits an operation writes.
    pub write_mask: u8,
    /// `STENCILOPVAL` (bits 24:31): the operand of the add, subtract and `REPLACE_OP` operations;
    /// radeonsi sets it to one (`si_state.c:1329`).
    pub op_value: u8,
}

/// Decodes a `DB_STENCILREFMASK` or `DB_STENCILREFMASK_BF` value; the two share a layout.
#[must_use]
pub const fn decode_stencil_ref_mask(value: u32) -> StencilRefMask {
    StencilRefMask {
        reference: (value & 0xFF) as u8,
        compare_mask: ((value >> 8) & 0xFF) as u8,
        write_mask: ((value >> 16) & 0xFF) as u8,
        op_value: ((value >> 24) & 0xFF) as u8,
    }
}

/// The depth and stencil tests a draw runs under: every register they read, together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DepthStencilState {
    /// `DB_DEPTH_CONTROL`: which tests run, with which comparisons.
    pub control: DepthControl,
    /// `DB_STENCIL_CONTROL`: what each stencil outcome does.
    pub ops: StencilControl,
    /// `DB_STENCILREFMASK`: the front face's reference and masks.
    pub front: StencilRefMask,
    /// `DB_STENCILREFMASK_BF`: the back face's, used when `control.backface_enable`.
    pub back: StencilRefMask,
}

/// The depth and stencil state in force, read a register at a time through `latest`.
///
/// [`None`] when `DB_DEPTH_CONTROL` was never written, so a stream that sets no depth state emits
/// none. The other three read zero when unwritten, as an unwritten user-data word does.
pub fn depth_stencil_from(mut latest: impl FnMut(u32) -> Option<u32>) -> Option<DepthStencilState> {
    let control = decode_depth_control(latest(DB_DEPTH_CONTROL)?);
    let mut at = |register| latest(register).unwrap_or(0);
    Some(DepthStencilState {
        control,
        ops: decode_stencil_control(at(DB_STENCIL_CONTROL)),
        front: decode_stencil_ref_mask(at(DB_STENCILREFMASK)),
        back: decode_stencil_ref_mask(at(DB_STENCILREFMASK_BF)),
    })
}

/// Which winding is a front face: `PA_SU_SC_MODE_CNTL`'s `FACE` (bit 2), in window coordinates
/// after the viewport transform. radeonsi writes `FACE(!front_ccw)` (`si_state.c:974`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrontFace {
    /// `FACE` 0: a counter-clockwise triangle faces front.
    CounterClockwise,
    /// `FACE` 1: a clockwise triangle faces front.
    Clockwise,
}

/// Which faces the rasteriser discards, and which winding is front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CullState {
    /// `CULL_FRONT` (bit 0).
    pub cull_front: bool,
    /// `CULL_BACK` (bit 1).
    pub cull_back: bool,
    /// `FACE` (bit 2).
    pub front_face: FrontFace,
}

/// Decodes `PA_SU_SC_MODE_CNTL`'s cull fields (`si_state.c:986-987`). Its polygon-mode, offset and
/// provoking-vertex fields are not decoded.
#[must_use]
pub const fn decode_cull(value: u32) -> CullState {
    CullState {
        cull_front: value & 1 != 0,
        cull_back: value & 2 != 0,
        front_face: if value & 4 == 0 {
            FrontFace::CounterClockwise
        } else {
            FrontFace::Clockwise
        },
    }
}

/// The per-draw depth, stencil and cull commands, emitted before each draw when what is in force
/// changed, as the blend state is.
#[derive(Debug, Default)]
pub(crate) struct DrawStateSent {
    depth_stencil: Option<DepthStencilState>,
    cull: Option<u32>,
}

impl DrawStateSent {
    /// Pushes the commands the draw at packet offset `at` needs: its depth and stencil state and
    /// cull state when they changed, and a clear of `target` when `DB_RENDER_CONTROL` asks the draw
    /// for one.
    pub(crate) fn push(
        &mut self,
        sweep: &mut RegisterSweep<'_>,
        at: u32,
        target: Option<ResourceId>,
        commands: &mut Vec<RenderCommand>,
    ) {
        if let Some(state) = depth_stencil_from(|register| sweep.latest(at, register))
            && self.depth_stencil != Some(state)
        {
            commands.push(RenderCommand::SetDepthStencil(state));
            self.depth_stencil = Some(state);
        }
        if let Some(value) = sweep.latest(at, PA_SU_SC_MODE_CNTL)
            && self.cull != Some(value & 0x7)
        {
            commands.push(RenderCommand::SetCull(decode_cull(value)));
            self.cull = Some(value & 0x7);
        }
        // A clear draw writes the clear value wherever it covers; the backend clears the whole
        // target, which is what a clear draw over the whole target does.
        let control = decode_render_control(sweep.latest(at, DB_RENDER_CONTROL).unwrap_or(0));
        if let Some(target) = target
            && (control.depth_clear || control.stencil_clear)
        {
            let depth = control
                .depth_clear
                .then(|| decode_depth_clear(sweep.latest(at, DB_DEPTH_CLEAR).unwrap_or(0)));
            let stencil = control
                .stencil_clear
                .then(|| decode_stencil_clear(sweep.latest(at, DB_STENCIL_CLEAR).unwrap_or(0)));
            commands.push(RenderCommand::ClearDepthStencil {
                target,
                depth,
                stencil,
            });
        }
    }
}

/// Every `DMA_DATA` fill in a walked stream, with its packet's offset.
pub(crate) fn fills_in(walked: &PacketWalk, stream: &[u8]) -> Vec<(u32, Fill)> {
    walked
        .packets
        .iter()
        .filter(|packet| {
            packet.kind
                == PacketKind::Command {
                    opcode: measured::DMA_DATA,
                }
        })
        .filter_map(|packet| {
            let start = packet.body_offset() as usize;
            let end = start + packet.body_length() as usize;
            let body: Vec<u32> = stream
                .get(start..end)?
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                .collect();
            Some((packet.offset, fill_of(&body)?))
        })
        .collect()
}

/// The depth value a fill leaves in every element of `target`'s depth surface, or [`None`] when it
/// does not cover the surface with one value in `0.0..=1.0`.
///
/// A `Z_32_FLOAT` element is the pattern's bits; a `Z_16` one is either half of it, when the two
/// agree. `Z_24`'s packing is not decoded.
fn depth_fill_value(fill: &Fill, target: &DepthTarget, pixels: u64) -> Option<f32> {
    if fill.destination != target.base {
        return None;
    }
    let value = match target.format {
        DepthFormat::Z32Float if fill.count >= pixels * 4 => f32::from_bits(fill.pattern),
        DepthFormat::Z16
            if fill.count >= pixels * 2 && fill.pattern >> 16 == fill.pattern & 0xFFFF =>
        {
            f32::from(u16::try_from(fill.pattern & 0xFFFF).ok()?) / f32::from(u16::MAX)
        }
        _ => return None,
    };
    (0.0..=1.0).contains(&value).then_some(value)
}

/// The depth a fill of `target`'s HTILE leaves every tile reading (D750): `DB_DEPTH_CLEAR`'s, when
/// the fill covers every 8x8 tile's 32 bits from the metadata's start with tiles in the cleared
/// state - `ZMASK`, bits 3:0 in every layout, zero (Mesa `ac_descriptors.h`, `HTILE_Z_CLEAR_REG` and
/// `HTILE_ZS_CLEAR_REG`). Each tile takes 32 bits and the metadata's blocks only pad it
/// (`gfx10addrlib.cpp` `HwlComputeHtileInfo`), so a fill shorter than that covers part of the
/// surface and is not this clear.
fn htile_fill_value(fill: &Fill, target: &DepthTarget, (width, height): (u32, u32)) -> Option<f32> {
    let htile = target.htile?;
    let tiles = u64::from(width.div_ceil(8)) * u64::from(height.div_ceil(8));
    (fill.destination == htile && fill.count >= tiles * 4 && fill.pattern & HTILE_ZMASK == 0)
        .then(|| decode_depth_clear(target.depth_clear))
        .filter(|value| (0.0..=1.0).contains(value))
}

/// The stencil value a fill leaves in every element of `target`'s stencil surface: one byte
/// repeated through the pattern, over at least one byte a pixel.
fn stencil_fill_value(fill: &Fill, target: &DepthTarget, pixels: u64) -> Option<u8> {
    let byte = fill.pattern & 0xFF;
    (target.stencil
        && fill.destination == target.stencil_base
        && fill.count >= pixels
        && fill.pattern == byte * 0x0101_0101)
        .then_some(u8::try_from(byte).ok()?)
}

/// How many fills a pipeline keeps from submissions with no draws, waiting for one that binds the
/// surface they cleared. A clear-only submission is followed by its frame's draws.
const PENDING_FILLS: usize = 32;

/// Fills of guest memory not yet matched against a depth target, carried from one submission to the
/// next: a guest may clear its depth surface in a submission of its own, or after its last draw.
#[derive(Debug, Default)]
pub(crate) struct PendingFills(Vec<Fill>);

impl PendingFills {
    /// [`Self::clear_before_draws`] over a walked stream's own fills and draws.
    pub(crate) fn clear_in_stream(
        &mut self,
        (walked, stream): (&PacketWalk, &[u8]),
        draws: &[DrawCall],
        target: Option<&(ResourceId, DepthTarget, ColourTargetExtent)>,
    ) -> Option<RenderCommand> {
        let span = draws
            .first()
            .zip(draws.last())
            .map(|(first, last)| (first.packet_offset, last.packet_offset));
        self.clear_before_draws(
            &fills_in(walked, stream),
            span,
            target.map(|(id, target, extent)| (*id, target, *extent)),
        )
    }

    /// The clear of `target` a submission's draws start from, from the fills before them - those
    /// carried from earlier submissions, then the submission's own before its first draw - and
    /// keeps those after its last draw for the next. A submission with no draws, or no depth
    /// target, keeps all of its fills.
    ///
    /// `draws` is the first and last draw's packet offset. The latest fill of each surface wins.
    pub(crate) fn clear_before_draws(
        &mut self,
        fills: &[(u32, Fill)],
        draws: Option<(u32, u32)>,
        target: Option<(ResourceId, &DepthTarget, ColourTargetExtent)>,
    ) -> Option<RenderCommand> {
        // Draws with no depth target leave every depth surface as it was, so their fills wait too.
        let (Some((first, last)), Some((id, target, extent))) = (draws, target) else {
            self.keep(fills.iter().map(|(_, fill)| *fill));
            return None;
        };
        let mut before = std::mem::take(&mut self.0);
        before.extend(
            fills
                .iter()
                .filter(|(offset, _)| *offset < first)
                .map(|(_, fill)| *fill),
        );
        self.keep(
            fills
                .iter()
                .filter(|(offset, _)| *offset > last)
                .map(|(_, fill)| *fill),
        );
        let pixels = u64::from(extent.width) * u64::from(extent.height);
        let depth = before.iter().rev().find_map(|fill| {
            depth_fill_value(fill, target, pixels)
                .or_else(|| htile_fill_value(fill, target, (extent.width, extent.height)))
        });
        let stencil = before
            .iter()
            .rev()
            .find_map(|fill| stencil_fill_value(fill, target, pixels));
        (depth.is_some() || stencil.is_some()).then_some(RenderCommand::ClearDepthStencil {
            target: id,
            depth,
            stencil,
        })
    }

    /// Keeps fills made outside a stream's packets for the next submission's draws (D750).
    pub(crate) fn note(&mut self, fills: Vec<Fill>) {
        self.keep(fills);
    }

    /// Keeps `fills` for the next submission, the most recent [`PENDING_FILLS`].
    fn keep(&mut self, fills: impl IntoIterator<Item = Fill>) {
        self.0.extend(fills);
        let excess = self.0.len().saturating_sub(PENDING_FILLS);
        self.0.drain(..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CullState, DepthFormat, DepthTarget, FrontFace, PendingFills, StencilRefMask, decode_cull,
        decode_depth_clear, decode_depth_format, decode_render_control, decode_stencil_clear,
        decode_stencil_format, decode_stencil_ref_mask, depth_stencil_from, depth_target_at,
        depth_target_id, surface_address,
    };
    use crate::backend::{RenderCommand, ResourceId};
    use crate::cp::Fill;
    use crate::registers::{ColourTargetExtent, CompareFunc, RegisterWrite, StencilOp};

    fn write(register: u32, value: u32) -> RegisterWrite {
        RegisterWrite {
            packet_offset: 0,
            register,
            value,
        }
    }

    /// `DB_Z_INFO`'s `FORMAT` is bits 0:1 of the `ZFormat` enum; the swizzle mode above it does
    /// not leak in. `0x8000_0183` is the open-toolchain GL context's `Z_32_FLOAT` with `SW_MODE`
    /// 24 and `ZRANGE_PRECISION`.
    #[test]
    fn the_depth_format_is_the_two_low_bits() {
        assert_eq!(decode_depth_format(0), DepthFormat::Invalid);
        assert_eq!(decode_depth_format(1), DepthFormat::Z16);
        assert_eq!(decode_depth_format(2), DepthFormat::Z24);
        assert_eq!(decode_depth_format(0x8000_0183), DepthFormat::Z32Float);
    }

    /// `DB_STENCIL_INFO`'s `FORMAT` is bit 0 alone: `0x2000_0180` (swizzle mode and
    /// `TILE_STENCIL_DISABLE`) names no stencil surface.
    #[test]
    fn the_stencil_format_is_bit_zero() {
        assert!(!decode_stencil_format(0x2000_0180));
        assert!(decode_stencil_format(0x2000_0181));
    }

    /// A base pair is `va >> 8` split at 32 bits, so the high register's byte lands at bit 40.
    #[test]
    fn a_surface_address_is_its_base_pair_in_256_byte_units() {
        assert_eq!(surface_address(0x0200_0e00, 0), 0x2_000e_0000);
        assert_eq!(surface_address(0x0000_0001, 0x12), 0x1200_0000_0100);
        assert_eq!(
            surface_address(0, 0xFFFF_FF01),
            1 << 40,
            "BASE_HI is eight bits"
        );
    }

    /// A depth target is bound only by a valid `DB_Z_INFO`; its bases and stencil come from the
    /// latest writes, and an invalid format binds none.
    #[test]
    fn a_depth_target_needs_a_valid_z_format() {
        let writes = [
            write(0xA010, 0x8000_0183),
            write(0xA011, 0x2000_0181),
            write(0xA014, 0x0003_0000),
            write(0xA01C, 0x1),
            write(0xA015, 0x0004_0000),
        ];
        assert_eq!(
            depth_target_at(&writes),
            Some(DepthTarget {
                format: DepthFormat::Z32Float,
                stencil: true,
                base: 0x100_0300_0000,
                stencil_base: 0x400_0000,
                htile: None,
                depth_clear: 0,
            })
        );
        assert_eq!(depth_target_at(&[write(0xA010, 0)]), None, "Z_INVALID");
        assert_eq!(depth_target_at(&[write(0xA014, 0x1)]), None, "no DB_Z_INFO");
    }

    /// A depth target's id is its address, apart from colour targets' and shaders'.
    #[test]
    fn a_depth_target_id_is_its_base_in_the_depth_namespace() {
        let target = DepthTarget {
            format: DepthFormat::Z32Float,
            stencil: false,
            base: 0x2_000e_0000,
            stencil_base: 0,
            htile: None,
            depth_clear: 0,
        };
        assert_eq!(
            depth_target_id(&target),
            ResourceId((1 << 62) | 0x0200_0e00)
        );
    }

    /// `DB_RENDER_CONTROL` bit 0 clears depth and bit 1 stencil; `DB_DEPTH_CLEAR` is float bits and
    /// `DB_STENCIL_CLEAR` its low byte.
    #[test]
    fn the_clear_registers_decode() {
        let both = decode_render_control(0x3);
        assert!(both.depth_clear && both.stencil_clear);
        let neither = decode_render_control(0x4);
        assert!(
            !neither.depth_clear && !neither.stencil_clear,
            "DEPTH_COPY is not a clear"
        );
        assert!((decode_depth_clear(0x3f80_0000) - 1.0).abs() < f32::EPSILON);
        assert_eq!(decode_stencil_clear(0x1_23), 0x23);
    }

    /// Each `DB_STENCILREFMASK` byte is its own field, as the open-toolchain GL context packs them.
    #[test]
    fn a_stencil_ref_mask_is_four_bytes() {
        assert_eq!(
            decode_stencil_ref_mask(0x01_0f_f0_7a),
            StencilRefMask {
                reference: 0x7a,
                compare_mask: 0xf0,
                write_mask: 0x0f,
                op_value: 1,
            }
        );
    }

    /// `PA_SU_SC_MODE_CNTL`'s bit 0 culls front faces, bit 1 back faces, and bit 2 makes clockwise
    /// the front; `0x240` is the open-toolchain GL context's default, culling nothing.
    #[test]
    fn the_cull_fields_are_the_three_low_bits() {
        assert_eq!(
            decode_cull(0x240),
            CullState {
                cull_front: false,
                cull_back: false,
                front_face: FrontFace::CounterClockwise,
            }
        );
        assert_eq!(
            decode_cull(0x246),
            CullState {
                cull_front: false,
                cull_back: true,
                front_face: FrontFace::Clockwise,
            }
        );
        assert!(decode_cull(0x1).cull_front);
    }

    /// The state is present only with `DB_DEPTH_CONTROL`; the stencil registers beside it read zero
    /// when unwritten.
    #[test]
    fn depth_stencil_state_needs_the_depth_control() {
        let written = |values: &'static [(u32, u32)]| {
            move |register: u32| values.iter().find(|(r, _)| *r == register).map(|(_, v)| *v)
        };
        assert!(depth_stencil_from(written(&[(0xA10B, 0x30)])).is_none());
        let state = depth_stencil_from(written(&[(0xA200, 0x16), (0xA10C, 0x0100_ff05)]))
            .expect("DB_DEPTH_CONTROL is written");
        assert!(state.control.depth_test_enable && state.control.depth_write_enable);
        assert_eq!(state.control.depth_func, CompareFunc::Less);
        assert_eq!(state.ops.fail_op, StencilOp::Keep);
        assert_eq!(state.front.reference, 5);
        assert_eq!(state.back, StencilRefMask::default());
    }

    fn z32_target() -> DepthTarget {
        DepthTarget {
            format: DepthFormat::Z32Float,
            stencil: true,
            base: 0x10_0000,
            stencil_base: 0x20_0000,
            htile: None,
            depth_clear: 0,
        }
    }

    const EXTENT: ColourTargetExtent = ColourTargetExtent {
        width: 4,
        height: 2,
    };

    /// A fill of the whole depth surface before the draws clears the host attachment to its value;
    /// one after the last draw waits for the next submission; one too short, or elsewhere, is not a
    /// clear.
    #[test]
    fn a_whole_surface_fill_before_the_draws_is_a_clear() {
        let target = z32_target();
        let id = ResourceId(7);
        let one = Fill {
            destination: 0x10_0000,
            pattern: 1.0f32.to_bits(),
            count: 32,
        };
        let stencil = Fill {
            destination: 0x20_0000,
            pattern: 0x8080_8080,
            count: 8,
        };
        let short = Fill { count: 28, ..one };
        let mut pending = PendingFills::default();
        let clear = pending.clear_before_draws(
            &[(0, short), (4, one), (8, stencil), (100, one)],
            Some((20, 60)),
            Some((id, &target, EXTENT)),
        );
        assert_eq!(
            clear,
            Some(RenderCommand::ClearDepthStencil {
                target: id,
                depth: Some(1.0),
                stencil: Some(0x80),
            })
        );
        // The fill after the last draw clears before the next submission's draws.
        let next = pending.clear_before_draws(&[], Some((0, 0)), Some((id, &target, EXTENT)));
        assert_eq!(
            next,
            Some(RenderCommand::ClearDepthStencil {
                target: id,
                depth: Some(1.0),
                stencil: None,
            })
        );
        assert_eq!(
            pending.clear_before_draws(&[(0, short)], Some((4, 4)), Some((id, &target, EXTENT))),
            None,
            "a fill short of the surface does not clear it"
        );
    }

    /// A fill of the depth surface's HTILE with tiles in the cleared state clears the attachment to
    /// `DB_DEPTH_CLEAR` (D750), as radeonsi's fast clear does; a fill short of every tile, one
    /// leaving a tile compressed, or one with HTILE disabled is not a clear. The registers are
    /// CRFT00001's: `DB_Z_INFO` `0xaf80_0183` enables HTILE at `0x4018700` in 256-byte units.
    #[test]
    fn an_htile_fill_in_the_cleared_state_clears_the_depth() {
        let writes = [
            write(0xA010, 0xaf80_0183),
            write(0xA005, 0x0401_8700),
            write(0xA014, 0x0401_0000),
            write(0xA00B, 1.0f32.to_bits()),
        ];
        let target = depth_target_at(&writes).expect("a depth target");
        assert_eq!(target.htile, Some(0x4_0187_0000));
        let id = ResourceId(7);
        // 16x8 pixels: two tiles, eight bytes. `HTILE_ZS_CLEAR_REG(1.0)`: ZMASK 0, SR0/SR1 3, ZBASE
        // 0x3FFF.
        let cleared = Fill {
            destination: 0x4_0187_0000,
            pattern: 0xFFFC_00F0,
            count: 8,
        };
        let extent = ColourTargetExtent {
            width: 16,
            height: 8,
        };
        let clear = |fill: Fill, target: &DepthTarget| {
            PendingFills::default().clear_before_draws(
                &[(0, fill)],
                Some((4, 4)),
                Some((id, target, extent)),
            )
        };
        assert_eq!(
            clear(cleared, &target),
            Some(RenderCommand::ClearDepthStencil {
                target: id,
                depth: Some(1.0),
                stencil: None,
            })
        );
        assert_eq!(
            clear(
                Fill {
                    count: 4,
                    ..cleared
                },
                &target
            ),
            None,
            "one tile of two"
        );
        assert_eq!(
            clear(
                Fill {
                    pattern: 0xFFFC_00FF,
                    ..cleared
                },
                &target
            ),
            None,
            "ZMASK 0xF: uncompressed, not cleared"
        );
        let untiled = DepthTarget {
            htile: None,
            ..target
        };
        assert_eq!(clear(cleared, &untiled), None, "no HTILE");
    }

    /// A submission with no draws keeps its fills for the one that has them.
    #[test]
    fn a_clear_only_submission_clears_before_the_next_draws() {
        let target = z32_target();
        let id = ResourceId(7);
        let half = Fill {
            destination: 0x10_0000,
            pattern: 0.5f32.to_bits(),
            count: 32,
        };
        let mut pending = PendingFills::default();
        assert_eq!(pending.clear_before_draws(&[(0, half)], None, None), None);
        // Draws with no depth target between leave the fill waiting.
        assert_eq!(pending.clear_before_draws(&[], Some((0, 0)), None), None);
        assert_eq!(
            pending.clear_before_draws(&[], Some((0, 0)), Some((id, &target, EXTENT))),
            Some(RenderCommand::ClearDepthStencil {
                target: id,
                depth: Some(0.5),
                stencil: None,
            })
        );
    }

    /// A `Z_16` surface clears from a pattern whose halves agree, as a unorm value.
    #[test]
    fn a_z16_fill_clears_to_its_unorm_value() {
        let target = DepthTarget {
            format: DepthFormat::Z16,
            ..z32_target()
        };
        let fill = Fill {
            destination: 0x10_0000,
            pattern: 0xFFFF_FFFF,
            count: 16,
        };
        let mut pending = PendingFills::default();
        assert_eq!(
            pending.clear_before_draws(
                &[(0, fill)],
                Some((4, 4)),
                Some((ResourceId(1), &target, EXTENT))
            ),
            Some(RenderCommand::ClearDepthStencil {
                target: ResourceId(1),
                depth: Some(1.0),
                stencil: None,
            })
        );
        let uneven = Fill {
            pattern: 0x0000_FFFF,
            ..fill
        };
        assert_eq!(
            pending.clear_before_draws(
                &[(0, uneven)],
                Some((4, 4)),
                Some((ResourceId(1), &target, EXTENT))
            ),
            None
        );
    }
}
