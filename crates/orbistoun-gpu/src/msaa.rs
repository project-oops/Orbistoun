//! A draw's multisampling state, read from the registers that hold it.
//!
//! Every field here is placed by `src/amd/registers/gfx103.json` in oops-mesa (Mesa 26.2.2), the
//! register database radeonsi and radv emit from, and each register's dword index is its byte
//! offset over four. What the drivers write there, and so what each field means in use, is cited
//! from radv (`radv_device.c`, `radv_cmd_buffer.c`) and the common descriptor code
//! (`ac_descriptors.c`).
//!
//! [`MsaaState::at`] reads the state; [`MsaaState::rasterization`] says how the host draws it: at
//! one sample, or multisampled at a count with its sample positions and coverage mask. What it
//! cannot model it names, by the register and field and value that asked for it
//! ([`MsaaRefusal`]). Until the host draws multisampled targets, a draw not at one sample is drawn
//! at one, and [`MsaaState::tally`] counts it in the submission's report and says each reason once
//! a run (D931).

use crate::registers::RegisterWrite;

/// `PA_SC_AA_CONFIG` (`gfx103.json`, byte `0x28BE0`): `MSAA_NUM_SAMPLES` 2:0, `MAX_SAMPLE_DIST`
/// 16:13, `MSAA_EXPOSED_SAMPLES` 22:20, `COVERAGE_TO_SHADER_SELECT` 27:26. radv writes the
/// rasterizer's `log2` sample count to both counts (`radv_cmd_buffer.c:13224`).
pub const PA_SC_AA_CONFIG: u32 = 0xA2F8;
/// `DB_EQAA` (`gfx103.json`, byte `0x28804`): `MAX_ANCHOR_SAMPLES` 2:0, `PS_ITER_SAMPLES` 6:4,
/// `MASK_EXPORT_NUM_SAMPLES` 10:8, `ALPHA_TO_MASK_NUM_SAMPLES` 14:12, each a `log2`
/// (`radv_cmd_buffer.c:13234-13235`).
pub const DB_EQAA: u32 = 0xA201;
/// `CB_COLOR_CONTROL` (`gfx103.json`, byte `0x28808`): `MODE` 6:4, the `CBMode` enum.
pub const CB_COLOR_CONTROL: u32 = 0xA202;
/// `PA_SC_MODE_CNTL_0` (`gfx103.json`, byte `0x28A48`): `MSAA_ENABLE` bit 0.
pub const PA_SC_MODE_CNTL_0: u32 = 0xA292;
/// `PA_SC_MODE_CNTL_1` (`gfx103.json`, byte `0x28A4C`): `PS_ITER_SAMPLE` bit 16, the pixel shader
/// run once a sample.
pub const PA_SC_MODE_CNTL_1: u32 = 0xA293;
/// `DB_Z_INFO` (`gfx103.json`, byte `0x28040`): `FORMAT` 1:0, `NUM_SAMPLES` 3:2 as a `log2`
/// (`ac_descriptors.c:950`).
pub const DB_Z_INFO: u32 = 0xA010;
/// `PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0` (`gfx103.json`, byte `0x28BF8`), the first of sixteen, to
/// `_X1Y1_3` at byte `0x28C34`: four a pixel of the 2x2 quad, `X0Y0`, `X1Y0`, `X0Y1`, `X1Y1` in that
/// order, each `_0` holding samples 0-3 and `_1` samples 4-7 as signed four-bit `Sn_X`, `Sn_Y` pairs
/// (`radv_device.c:951-961`). `_2` and `_3` hold samples 8-15, which radv never writes.
pub const PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0: u32 = 0xA2FE;
/// Dwords from one pixel's sample locations to the next's: `X1Y0_0` is byte `0x28C08`.
const SAMPLE_LOCS_PIXEL_STRIDE: u32 = 4;
/// `PA_SC_AA_MASK_X0Y0_X1Y0` (`gfx103.json`, byte `0x28C38`): `AA_MASK_X0Y0` 15:0, `AA_MASK_X1Y0`
/// 31:16.
pub const PA_SC_AA_MASK_X0Y0_X1Y0: u32 = 0xA30E;
/// `PA_SC_AA_MASK_X0Y1_X1Y1` (`gfx103.json`, byte `0x28C3C`): `AA_MASK_X0Y1` 15:0, `AA_MASK_X1Y1`
/// 31:16.
pub const PA_SC_AA_MASK_X0Y1_X1Y1: u32 = 0xA30F;
/// `CB_COLOR0_INFO` (`gfx103.json`, byte `0x28C70`): `FORMAT` 6:2, `COLOR_INVALID` (0) for a target
/// that is not bound.
const CB_COLOR0_INFO: u32 = 0xA31C;
/// `CB_COLOR0_ATTRIB` (`gfx103.json`, byte `0x28C74`): `NUM_SAMPLES` 14:12, `NUM_FRAGMENTS` 16:15,
/// each a `log2` (`ac_descriptors.c:1348-1349`).
const CB_COLOR0_ATTRIB: u32 = 0xA31D;
/// Dwords from one colour target's registers to the next's: `CB_COLOR1_ATTRIB` is byte `0x28CB0`.
const CB_COLOR_STRIDE: u32 = 0xF;
/// Colour targets a draw can bind.
pub const COLOUR_TARGETS: usize = 8;

/// `CBMode` (`gfx103.json` `enums`), `CB_COLOR_CONTROL.MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CbMode {
    /// `CB_DISABLE` (0): no colour is written.
    Disable,
    /// `CB_NORMAL` (1).
    Normal,
    /// `CB_ELIMINATE_FAST_CLEAR` (2).
    EliminateFastClear,
    /// `CB_RESOLVE` (3): target 0's samples resolved into target 1.
    Resolve,
    /// `CB_DECOMPRESS` (4).
    Decompress,
    /// `CB_FMASK_DECOMPRESS` (5).
    FmaskDecompress,
    /// `CB_DCC_DECOMPRESS` (6).
    DccDecompress,
    /// `CB_RESERVED` (7).
    Reserved,
}

impl CbMode {
    /// `CB_COLOR_CONTROL.MODE` from the register's value.
    #[must_use]
    pub fn decode(cb_color_control: u32) -> Self {
        match (cb_color_control >> 4) & 7 {
            0 => Self::Disable,
            1 => Self::Normal,
            2 => Self::EliminateFastClear,
            3 => Self::Resolve,
            4 => Self::Decompress,
            5 => Self::FmaskDecompress,
            6 => Self::DccDecompress,
            _ => Self::Reserved,
        }
    }

    /// The enum's name, for a refusal.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Disable => "CB_DISABLE",
            Self::Normal => "CB_NORMAL",
            Self::EliminateFastClear => "CB_ELIMINATE_FAST_CLEAR",
            Self::Resolve => "CB_RESOLVE",
            Self::Decompress => "CB_DECOMPRESS",
            Self::FmaskDecompress => "CB_FMASK_DECOMPRESS",
            Self::DccDecompress => "CB_DCC_DECOMPRESS",
            Self::Reserved => "CB_RESERVED",
        }
    }
}

/// A colour target's sample and fragment counts, `CB_COLORn_ATTRIB` as `log2`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourSamples {
    /// `NUM_SAMPLES`: coverage samples a pixel.
    pub log2_samples: u32,
    /// `NUM_FRAGMENTS`: colour values stored a pixel, the FMASK's palette. Fewer than the samples is
    /// EQAA.
    pub log2_fragments: u32,
}

impl ColourSamples {
    /// The counts from a `CB_COLORn_ATTRIB` value.
    #[must_use]
    pub fn decode(attrib: u32) -> Self {
        Self {
            log2_samples: (attrib >> 12) & 7,
            log2_fragments: (attrib >> 15) & 3,
        }
    }
}

/// Where a sample lies in its pixel: signed sixteenths of a pixel from its centre, as
/// `PA_SC_AA_SAMPLE_LOCS` holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SampleOffset {
    /// Sixteenths right of the centre, -8 to 7.
    pub x: i8,
    /// Sixteenths down from the centre, -8 to 7.
    pub y: i8,
}

impl SampleOffset {
    /// The position as `VkSampleLocationEXT` takes it: a fraction of the pixel from its top-left
    /// corner, in `[0, 1)`.
    #[must_use]
    pub fn vulkan(self) -> [f32; 2] {
        [
            f32::from(self.x) / 16.0 + 0.5,
            f32::from(self.y) / 16.0 + 0.5,
        ]
    }
}

/// The sample positions of each pixel of the 2x2 quad: `X0Y0`, `X1Y0`, `X0Y1`, `X1Y1`.
pub type QuadLocations = [Vec<SampleOffset>; 4];

/// Whether `why` is said for the first time this run.
fn first_seen(why: &str) -> bool {
    static SEEN: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    SEEN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(why.to_owned())
}

/// Sign-extends a four-bit field.
fn nibble(word: u32, index: u32) -> i8 {
    let raw = ((word >> (index * 4)) & 0xF) as i8;
    (raw << 4) >> 4
}

/// How the host rasterizes a draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Rasterization {
    /// One sample a pixel at its centre.
    Single,
    /// Several.
    Multisampled(Multisampled),
}

/// A multisampled draw as the host draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct Multisampled {
    /// Samples a pixel: 2, 4 or 8.
    pub samples: u32,
    /// The guest's positions a pixel of the quad, when it wrote them. The same in every pixel takes a
    /// 1x1 `VkSampleLocationsInfoEXT` grid; otherwise the 2x2 grid in the order
    /// [`QuadLocations`] lists, which is `VK_EXT_sample_locations`' `(x + y * 2) * samples + s`.
    pub locations: QuadLocations,
    /// The coverage mask, `PA_SC_AA_MASK`, the same in every pixel of the quad. `None` where the
    /// stream never wrote it.
    pub sample_mask: Option<u16>,
    /// The pixel shader runs once a sample (`PA_SC_MODE_CNTL_1.PS_ITER_SAMPLE`), at
    /// `DB_EQAA.PS_ITER_SAMPLES` samples.
    pub per_sample_shading: Option<u32>,
}

impl Multisampled {
    /// Whether every pixel of the quad has the same positions: a 1x1 grid.
    #[must_use]
    pub fn uniform_locations(&self) -> bool {
        self.locations
            .iter()
            .all(|pixel| *pixel == self.locations[0])
    }
}

/// Why a draw's multisampling is refused, naming what asked for it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MsaaRefusal {
    /// A count the host does not draw: sixteen samples, or a reserved code.
    #[error("{register}.{field} = {value} asks for {samples} samples, which are not modelled")]
    Count {
        /// The register.
        register: &'static str,
        /// Its field.
        field: &'static str,
        /// The field's value, a `log2`.
        value: u32,
        /// The count it means.
        samples: u32,
    },
    /// Two registers that both give the draw's sample count disagree.
    #[error(
        "{first} gives {first_samples} samples and {second} {second_samples}: a target sampled at another count than the rasterizer is not modelled"
    )]
    Mismatch {
        /// The register the rasterizer's count comes from.
        first: &'static str,
        /// Its count.
        first_samples: u32,
        /// The target's register.
        second: String,
        /// Its count.
        second_samples: u32,
    },
    /// Fewer stored fragments than coverage samples.
    #[error(
        "CB_COLOR{target}_ATTRIB stores {fragments} fragments of {samples} samples: EQAA is not modelled"
    )]
    Eqaa {
        /// The colour target.
        target: usize,
        /// `NUM_FRAGMENTS`, as a count.
        fragments: u32,
        /// `NUM_SAMPLES`, as a count.
        samples: u32,
    },
    /// The depth target anchors fewer samples than the rasterizer runs.
    #[error(
        "DB_EQAA.MAX_ANCHOR_SAMPLES = {value} anchors {anchors} of {samples} samples: EQAA depth is not modelled"
    )]
    EqaaDepth {
        /// The field's value.
        value: u32,
        /// Its count.
        anchors: u32,
        /// The rasterizer's count.
        samples: u32,
    },
    /// Samples exposed to the coverage logic beyond the rasterized ones.
    #[error(
        "PA_SC_AA_CONFIG.MSAA_EXPOSED_SAMPLES = {exposed} with MSAA_NUM_SAMPLES = {rasterized}: conservative or underestimated coverage is not modelled"
    )]
    Exposed {
        /// `MSAA_EXPOSED_SAMPLES`.
        exposed: u32,
        /// `MSAA_NUM_SAMPLES`.
        rasterized: u32,
    },
    /// A multisampled draw whose sample positions the stream never wrote: the console's default
    /// positions are not measured.
    #[error(
        "{samples} samples with no PA_SC_AA_SAMPLE_LOCS_PIXEL_X{x}Y{y}_{word} written: the console's default positions are not measured"
    )]
    UnwrittenLocations {
        /// The count.
        samples: u32,
        /// The pixel of the quad.
        x: u32,
        /// The pixel of the quad.
        y: u32,
        /// The register of that pixel's four.
        word: u32,
    },
    /// The quad's pixels cover different samples, which one Vulkan sample mask cannot.
    #[error(
        "PA_SC_AA_MASK differs across the quad ({masks:04x?}): a per-pixel coverage mask is not modelled"
    )]
    QuadMask {
        /// `X0Y0`, `X1Y0`, `X0Y1`, `X1Y1`.
        masks: [u16; 4],
    },
    /// A colour mode that draws more than colour.
    #[error("CB_COLOR_CONTROL.MODE = {} is not modelled", .0.name())]
    Mode(CbMode),
}

/// The multisampling registers as they stand before a draw. A register the stream never wrote is
/// `None`; [`Self::rasterization`] reads an unwritten count as its reset value, zero, one sample.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MsaaState {
    /// `PA_SC_AA_CONFIG`.
    pub aa_config: Option<u32>,
    /// `DB_EQAA`.
    pub eqaa: Option<u32>,
    /// `CB_COLOR_CONTROL`.
    pub color_control: Option<u32>,
    /// `PA_SC_MODE_CNTL_0`.
    pub mode_cntl_0: Option<u32>,
    /// `PA_SC_MODE_CNTL_1`.
    pub mode_cntl_1: Option<u32>,
    /// `DB_Z_INFO`.
    pub z_info: Option<u32>,
    /// Each bound colour target's counts: `CB_COLORn_INFO.FORMAT` other than `COLOR_INVALID`.
    pub colour: [Option<ColourSamples>; COLOUR_TARGETS],
    /// `PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0` onward, all sixteen.
    pub sample_locs: [Option<u32>; 16],
    /// `PA_SC_AA_MASK_X0Y0_X1Y0` and `_X0Y1_X1Y1`.
    pub aa_mask: [Option<u32>; 2],
}

impl MsaaState {
    /// The registers' live values among `writes`: each the last write to it.
    #[must_use]
    pub fn at(writes: &[RegisterWrite]) -> Self {
        Self::read(|register| {
            writes
                .iter()
                .rev()
                .find(|write| write.register == register)
                .map(|write| write.value)
        })
    }

    /// The registers as `latest` gives each one's value in force, `None` for one never written: a
    /// draw's state, read the way the draw sweep reads the rest of it.
    pub fn read(mut last: impl FnMut(u32) -> Option<u32>) -> Self {
        let colour = std::array::from_fn(|target| {
            let step = CB_COLOR_STRIDE * target as u32;
            let info = last(CB_COLOR0_INFO + step)?;
            if info & (0x1F << 2) == 0 {
                return None;
            }
            Some(ColourSamples::decode(
                last(CB_COLOR0_ATTRIB + step).unwrap_or(0),
            ))
        });
        Self {
            aa_config: last(PA_SC_AA_CONFIG),
            eqaa: last(DB_EQAA),
            color_control: last(CB_COLOR_CONTROL),
            mode_cntl_0: last(PA_SC_MODE_CNTL_0),
            mode_cntl_1: last(PA_SC_MODE_CNTL_1),
            z_info: last(DB_Z_INFO),
            colour,
            sample_locs: std::array::from_fn(|i| {
                last(PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0 + i as u32)
            }),
            aa_mask: [last(PA_SC_AA_MASK_X0Y0_X1Y0), last(PA_SC_AA_MASK_X0Y1_X1Y1)],
        }
    }

    /// Why a draw under this state is not drawn as the guest asked, `None` for one sample: the
    /// refusal [`Self::rasterization`] names, or a multisampled draw, which the host draws at one
    /// sample until its targets are multisampled.
    #[must_use]
    pub fn unmodelled(&self) -> Option<String> {
        match self.rasterization() {
            Ok(Rasterization::Single) => None,
            Ok(Rasterization::Multisampled(draw)) => Some(format!(
                "PA_SC_AA_CONFIG.MSAA_NUM_SAMPLES asks for {} samples, which are drawn at one",
                draw.samples
            )),
            Err(refusal) => Some(refusal.to_string()),
        }
    }

    /// Counts a draw whose state `latest` gives into `tally` when it is [`Self::unmodelled`],
    /// keeping the first reason: what the draw sweep calls at each draw. Each reason is also said
    /// once a run, so a run's log names every multisampling shape its title draws with (D931).
    pub fn tally(latest: impl FnMut(u32) -> Option<u32>, tally: &mut (usize, Option<String>)) {
        if let Some(why) = Self::read(latest).unmodelled() {
            if first_seen(&why) {
                tracing::warn!("a draw is drawn at one sample: {why}");
            }
            tally.0 += 1;
            tally.1.get_or_insert(why);
        }
    }

    /// `PA_SC_AA_CONFIG.MSAA_NUM_SAMPLES`, a `log2`.
    fn log2_rasterized(&self) -> u32 {
        self.aa_config.unwrap_or(0) & 7
    }

    /// The pixel `quad` (0-3) of the 2x2's first `samples` positions, or the register of them that
    /// was never written.
    fn quad_locations(&self, quad: u32, samples: u32) -> Result<Vec<SampleOffset>, (u32, u32)> {
        (0..samples)
            .map(|sample| {
                let word = sample / 4;
                let index = (quad * SAMPLE_LOCS_PIXEL_STRIDE + word) as usize;
                let value = self.sample_locs[index].ok_or((quad, word))?;
                let field = (sample % 4) * 2;
                Ok(SampleOffset {
                    x: nibble(value, field),
                    y: nibble(value, field + 1),
                })
            })
            .collect()
    }

    /// How the host draws under this state, or why it cannot.
    ///
    /// # Errors
    ///
    /// [`MsaaRefusal`], naming the register and field whose value is not modelled.
    pub fn rasterization(&self) -> Result<Rasterization, MsaaRefusal> {
        let count = |register: &'static str, field: &'static str, value: u32| match value {
            0..=3 => Ok(1 << value),
            _ => Err(MsaaRefusal::Count {
                register,
                field,
                value,
                samples: 1 << value,
            }),
        };
        let log2 = self.log2_rasterized();
        let samples = count("PA_SC_AA_CONFIG", "MSAA_NUM_SAMPLES", log2)?;
        let exposed = (self.aa_config.unwrap_or(0) >> 20) & 7;
        if exposed != log2 {
            return Err(MsaaRefusal::Exposed {
                exposed,
                rasterized: log2,
            });
        }
        for (target, colour) in self.colour.iter().enumerate() {
            let Some(colour) = colour else { continue };
            let target_samples = count("CB_COLORn_ATTRIB", "NUM_SAMPLES", colour.log2_samples)?;
            if target_samples != samples {
                return Err(MsaaRefusal::Mismatch {
                    first: "PA_SC_AA_CONFIG.MSAA_NUM_SAMPLES",
                    first_samples: samples,
                    second: format!("CB_COLOR{target}_ATTRIB.NUM_SAMPLES"),
                    second_samples: target_samples,
                });
            }
            if colour.log2_fragments != colour.log2_samples {
                return Err(MsaaRefusal::Eqaa {
                    target,
                    fragments: 1 << colour.log2_fragments,
                    samples: target_samples,
                });
            }
        }
        // A depth target is bound by a `FORMAT` other than `Z_INVALID` (0).
        if let Some(z_info) = self.z_info.filter(|z| z & 3 != 0) {
            let depth_samples = 1 << ((z_info >> 2) & 3);
            if depth_samples != samples {
                return Err(MsaaRefusal::Mismatch {
                    first: "PA_SC_AA_CONFIG.MSAA_NUM_SAMPLES",
                    first_samples: samples,
                    second: "DB_Z_INFO.NUM_SAMPLES".to_owned(),
                    second_samples: depth_samples,
                });
            }
        }
        if samples == 1 {
            return Ok(Rasterization::Single);
        }
        let eqaa = self.eqaa.unwrap_or(0);
        let anchors = eqaa & 7;
        if anchors != log2 {
            return Err(MsaaRefusal::EqaaDepth {
                value: anchors,
                anchors: 1 << anchors,
                samples,
            });
        }
        let mode = CbMode::decode(self.color_control.unwrap_or(1 << 4));
        if !matches!(mode, CbMode::Normal | CbMode::Disable) {
            return Err(MsaaRefusal::Mode(mode));
        }
        let mut locations: QuadLocations = Default::default();
        for (quad, pixel) in locations.iter_mut().enumerate() {
            *pixel = self
                .quad_locations(quad as u32, samples)
                .map_err(|(quad, word)| MsaaRefusal::UnwrittenLocations {
                    samples,
                    x: quad & 1,
                    y: quad >> 1,
                    word,
                })?;
        }
        let sample_mask = match self.aa_mask {
            [Some(top), Some(bottom)] => {
                let masks = [
                    top as u16,
                    (top >> 16) as u16,
                    bottom as u16,
                    (bottom >> 16) as u16,
                ];
                if masks.iter().any(|&mask| mask != masks[0]) {
                    return Err(MsaaRefusal::QuadMask { masks });
                }
                Some(masks[0])
            }
            _ => None,
        };
        let per_sample_shading =
            (self.mode_cntl_1.unwrap_or(0) & (1 << 16) != 0).then(|| 1 << ((eqaa >> 4) & 7));
        Ok(Rasterization::Multisampled(Multisampled {
            samples,
            locations,
            sample_mask,
            per_sample_shading,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(register: u32, value: u32) -> RegisterWrite {
        RegisterWrite {
            packet_offset: 0,
            register,
            value,
        }
    }

    /// radv's `FILL_SREG` (`radv_device.c:951-954`): eight signed nibbles, sample 0's x lowest.
    fn fill_sreg(pairs: [(i32, i32); 4]) -> u32 {
        pairs.iter().enumerate().fold(0, |word, (i, &(x, y))| {
            word | ((x as u32 & 0xF) << (i * 8)) | ((y as u32 & 0xF) << (i * 8 + 4))
        })
    }

    /// radv's 4x default positions (`radv_device.c:974`), the same in every pixel.
    fn radv_4x() -> u32 {
        fill_sreg([(-2, -6), (6, -2), (-6, 2), (2, 6)])
    }

    /// A target of format `COLOR_8_8_8_8` (10) at `log2` samples and fragments.
    fn colour_target(n: u32, log2_samples: u32, log2_fragments: u32) -> [RegisterWrite; 2] {
        [
            write(CB_COLOR0_INFO + n * CB_COLOR_STRIDE, 10 << 2),
            write(
                CB_COLOR0_ATTRIB + n * CB_COLOR_STRIDE,
                log2_samples << 12 | log2_fragments << 15,
            ),
        ]
    }

    /// radv's 4x state: `MSAA_NUM_SAMPLES` and `MSAA_EXPOSED_SAMPLES` 2 (`radv_cmd_buffer.c:13224`),
    /// `MAX_SAMPLE_DIST` 6 (`radv_device.c:975`), `MAX_ANCHOR_SAMPLES` 2 (`:13234`), its default
    /// positions in all four pixels, every sample covered.
    fn four_x() -> Vec<RegisterWrite> {
        let mut writes = vec![
            write(PA_SC_AA_CONFIG, 2 | 6 << 13 | 2 << 20),
            write(DB_EQAA, 2 | 2 << 8 | 2 << 12),
            write(CB_COLOR_CONTROL, 1 << 4),
            write(PA_SC_AA_MASK_X0Y0_X1Y0, 0xFFFF_FFFF),
            write(PA_SC_AA_MASK_X0Y1_X1Y1, 0xFFFF_FFFF),
        ];
        writes.extend(colour_target(0, 2, 2));
        for quad in 0..4 {
            writes.push(write(
                PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0 + quad * SAMPLE_LOCS_PIXEL_STRIDE,
                radv_4x(),
            ));
        }
        writes
    }

    /// The register indices are `gfx103.json`'s byte offsets over four.
    #[test]
    fn registers_sit_at_their_byte_offsets() {
        for (dword, byte) in [
            (PA_SC_AA_CONFIG, 0x28BE0),
            (DB_EQAA, 0x28804),
            (CB_COLOR_CONTROL, 0x28808),
            (PA_SC_MODE_CNTL_0, 0x28A48),
            (PA_SC_MODE_CNTL_1, 0x28A4C),
            (DB_Z_INFO, 0x28040),
            (PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0, 0x28BF8),
            (
                PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0 + SAMPLE_LOCS_PIXEL_STRIDE,
                0x28C08,
            ),
            (PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0 + 15, 0x28C34),
            (PA_SC_AA_MASK_X0Y0_X1Y0, 0x28C38),
            (PA_SC_AA_MASK_X0Y1_X1Y1, 0x28C3C),
            (CB_COLOR0_INFO, 0x28C70),
            (CB_COLOR0_ATTRIB, 0x28C74),
            (CB_COLOR0_ATTRIB + CB_COLOR_STRIDE, 0x28CB0),
            (CB_COLOR0_ATTRIB + 7 * CB_COLOR_STRIDE, 0x28E18),
        ] {
            assert_eq!(dword - 0xA000, (byte - 0x28000) / 4, "{byte:#x}");
        }
    }

    /// No multisampling register written is one sample: the state every title so far has drawn.
    #[test]
    fn an_unwritten_state_is_one_sample() {
        assert_eq!(
            MsaaState::at(&[]).rasterization(),
            Ok(Rasterization::Single)
        );
        let mut writes = colour_target(0, 0, 0).to_vec();
        writes.push(write(DB_Z_INFO, 1));
        assert_eq!(
            MsaaState::at(&writes).rasterization(),
            Ok(Rasterization::Single)
        );
    }

    /// radv's 4x draw decodes to four samples at its positions, a 1x1 grid, all covered.
    #[test]
    fn radv_four_x_is_four_samples_at_its_positions() {
        let Ok(Rasterization::Multisampled(draw)) = MsaaState::at(&four_x()).rasterization() else {
            panic!("four samples");
        };
        assert_eq!(draw.samples, 4);
        assert!(draw.uniform_locations());
        let at = |x, y| SampleOffset { x, y };
        assert_eq!(
            draw.locations[0],
            [at(-2, -6), at(6, -2), at(-6, 2), at(2, 6)]
        );
        assert_eq!(
            draw.locations[0][0].vulkan().map(f32::to_bits),
            [0.375_f32, 0.125].map(f32::to_bits)
        );
        assert_eq!(draw.sample_mask, Some(0xFFFF));
        assert_eq!(draw.per_sample_shading, None);
    }

    /// A draw is reported unmodelled for any state but one sample, by what asked for it.
    #[test]
    fn only_one_sample_is_modelled() {
        assert_eq!(MsaaState::at(&[]).unmodelled(), None);
        assert_eq!(
            MsaaState::at(&four_x()).unmodelled().as_deref(),
            Some("PA_SC_AA_CONFIG.MSAA_NUM_SAMPLES asks for 4 samples, which are drawn at one")
        );
        let mut eqaa = four_x();
        eqaa.extend(colour_target(1, 2, 0));
        assert_eq!(
            MsaaState::at(&eqaa).unmodelled().as_deref(),
            Some("CB_COLOR1_ATTRIB stores 1 fragments of 4 samples: EQAA is not modelled")
        );
        let latest = |writes: Vec<RegisterWrite>| {
            move |register| {
                writes
                    .iter()
                    .rev()
                    .find(|write: &&RegisterWrite| write.register == register)
                    .map(|write| write.value)
            }
        };
        let mut tally = (0, None);
        MsaaState::tally(latest(Vec::new()), &mut tally);
        MsaaState::tally(latest(eqaa), &mut tally);
        MsaaState::tally(latest(four_x()), &mut tally);
        assert_eq!(tally.0, 2, "the one-sample draw is not counted");
        assert!(
            tally.1.is_some_and(|first| first.contains("EQAA")),
            "the first reason kept"
        );
    }

    /// A reason is said once a run: the second sighting is not first.
    #[test]
    fn a_reason_is_seen_first_once() {
        let why = "a reason only this test gives";
        assert!(first_seen(why));
        assert!(!first_seen(why));
    }

    /// radv's 8x positions (`radv_device.c:979-981`) span two words a pixel.
    #[test]
    fn eight_samples_read_two_words_a_pixel() {
        let mut writes = four_x();
        writes.retain(|w| w.register != PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0);
        writes.push(write(PA_SC_AA_CONFIG, 3 | 7 << 13 | 3 << 20));
        writes.push(write(DB_EQAA, 3));
        writes.extend(colour_target(0, 3, 3));
        let low = fill_sreg([(1, -3), (-1, 3), (5, 1), (-3, -5)]);
        let high = fill_sreg([(-5, 5), (-7, -1), (3, 7), (7, -7)]);
        for quad in 0..4 {
            let base = PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0 + quad * SAMPLE_LOCS_PIXEL_STRIDE;
            writes.push(write(base, low));
            writes.push(write(base + 1, high));
        }
        let Ok(Rasterization::Multisampled(draw)) = MsaaState::at(&writes).rasterization() else {
            panic!("eight samples");
        };
        assert_eq!(draw.samples, 8);
        assert_eq!(draw.locations[3][7], SampleOffset { x: 7, y: -7 });
        assert_eq!(
            draw.locations[3][7].vulkan().map(f32::to_bits),
            [0.9375_f32, 0.0625].map(f32::to_bits)
        );
    }

    /// A quad whose pixels sample at different positions keeps each pixel's own.
    #[test]
    fn a_quad_keeps_each_pixels_positions() {
        let mut writes = four_x();
        writes.push(write(
            PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0 + 3 * SAMPLE_LOCS_PIXEL_STRIDE,
            fill_sreg([(1, 1), (2, 2), (3, 3), (4, 4)]),
        ));
        let Ok(Rasterization::Multisampled(draw)) = MsaaState::at(&writes).rasterization() else {
            panic!("four samples");
        };
        assert!(!draw.uniform_locations());
        assert_eq!(draw.locations[3][2], SampleOffset { x: 3, y: 3 });
        assert_eq!(draw.locations[0][2], SampleOffset { x: -6, y: 2 });
    }

    /// Per-sample shading reads `PS_ITER_SAMPLE` and `DB_EQAA.PS_ITER_SAMPLES`.
    #[test]
    fn per_sample_shading_reads_its_count() {
        let mut writes = four_x();
        writes.push(write(PA_SC_MODE_CNTL_1, 1 << 16));
        writes.push(write(DB_EQAA, 2 | 2 << 4));
        let Ok(Rasterization::Multisampled(draw)) = MsaaState::at(&writes).rasterization() else {
            panic!("four samples");
        };
        assert_eq!(draw.per_sample_shading, Some(4));
    }

    /// Every shape not modelled is refused by the register, field and value that asked for it.
    #[test]
    fn unmodelled_shapes_refuse_by_name() {
        let refusal = |extra: Vec<RegisterWrite>| {
            let mut writes = four_x();
            writes.extend(extra);
            MsaaState::at(&writes).rasterization().unwrap_err()
        };
        assert_eq!(
            refusal(colour_target(0, 2, 1).to_vec()),
            MsaaRefusal::Eqaa {
                target: 0,
                fragments: 2,
                samples: 4
            }
        );
        assert!(matches!(
            refusal(colour_target(2, 3, 3).to_vec()),
            MsaaRefusal::Mismatch { second, second_samples: 8, .. } if second == "CB_COLOR2_ATTRIB.NUM_SAMPLES"
        ));
        assert!(matches!(
            refusal(vec![write(DB_Z_INFO, 1)]),
            MsaaRefusal::Mismatch {
                second_samples: 1,
                ..
            }
        ));
        assert_eq!(
            refusal(vec![write(DB_EQAA, 1)]),
            MsaaRefusal::EqaaDepth {
                value: 1,
                anchors: 2,
                samples: 4
            }
        );
        assert_eq!(
            refusal(vec![write(PA_SC_AA_CONFIG, 4 | 4 << 20)]),
            MsaaRefusal::Count {
                register: "PA_SC_AA_CONFIG",
                field: "MSAA_NUM_SAMPLES",
                value: 4,
                samples: 16
            }
        );
        assert_eq!(
            refusal(vec![write(PA_SC_AA_CONFIG, 2)]),
            MsaaRefusal::Exposed {
                exposed: 0,
                rasterized: 2
            }
        );
        assert_eq!(
            refusal(vec![write(CB_COLOR_CONTROL, 3 << 4)]),
            MsaaRefusal::Mode(CbMode::Resolve)
        );
        assert_eq!(
            refusal(vec![write(PA_SC_AA_MASK_X0Y1_X1Y1, 0x0001_FFFF)]),
            MsaaRefusal::QuadMask {
                masks: [0xFFFF, 0xFFFF, 0xFFFF, 0x0001]
            }
        );
        let unwritten: Vec<RegisterWrite> = four_x()
            .into_iter()
            .filter(|w| w.register != PA_SC_AA_SAMPLE_LOCS_PIXEL_X0Y0_0 + 8)
            .collect();
        assert_eq!(
            MsaaState::at(&unwritten).rasterization(),
            Err(MsaaRefusal::UnwrittenLocations {
                samples: 4,
                x: 0,
                y: 1,
                word: 0
            })
        );
        assert_eq!(
            MsaaRefusal::Mode(CbMode::Resolve).to_string(),
            "CB_COLOR_CONTROL.MODE = CB_RESOLVE is not modelled"
        );
    }
}
