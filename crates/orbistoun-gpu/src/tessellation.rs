//! Tessellated draws: the registers that make one, the memory its stages share, and the
//! fixed-function tessellator between them (D928).
//!
//! On this GPU a tessellated draw runs four stages (Mesa `si_state_draw.cpp:104-131`):
//!
//! 1. the local shader (LS), the draw's vertex shader, merged into the hull shader's program;
//! 2. the hull shader (HS), which writes each patch's tessellation factors to the TF ring and its
//!    output control points and patch constants to the off-chip buffer
//!    (`ac_nir_lower_tess_io_to_mem.c:1126-1165`, `:1403-1466`);
//! 3. the tessellator, which reads the factors back from the ring and makes the domain points;
//! 4. the domain shader (DS), in the vertex or ES slot (`VS_EN`/`ES_EN` = `*_STAGE_DS`), run once
//!    per domain point and reading the off-chip buffer.
//!
//! D928 carries a draw out as the hardware does it, stage by stage: the LS+HS program runs as
//! compute waves (the D861 route) and writes the ring and the off-chip buffer in guest memory
//! itself, [`tessellator`] reads the factors out of the ring, and the domain shader runs over the
//! points through the mesh path. Until every stage is wired a tessellated draw is refused by name
//! ([`refusal`]) instead of drawing its domain shader as if it were a vertex shader.
//!
//! The registers are decoded from `src/amd/registers/gfx103.json` in oops-mesa. What the console's
//! driver writes into them, the ring's layout and the tessellator's output are each a row of the
//! tessellation census on obSCEne's bus (D928); a value here that the census has not measured cites
//! the public source it comes from.

pub mod tessellator;

/// `VGT_SHADER_STAGES_EN` (`gfx103.json`, context dword `0xA2D5`).
const VGT_SHADER_STAGES_EN: u32 = 0xA2D5;
/// Its `LS_EN`, bits 1:0: 1 is `LS_STAGE_ON`, 2 `CS_STAGE_ON` (`VGT_STAGES_LS_EN`).
const LS_EN: u32 = 0x3;
/// Its `HS_EN`, bit 2.
const HS_EN: u32 = 1 << 2;
/// Its `ES_EN`, bits 4:3, and `VS_EN`, bits 7:6; each is 1 for a domain shader in that slot
/// (`ES_STAGE_DS`, `VS_STAGE_DS`).
const ES_EN_SHIFT: u32 = 3;
const VS_EN_SHIFT: u32 = 6;
const STAGE_DS: u32 = 1;
/// Its `DYNAMIC_HS`, bit 8, which radeonsi sets on every tessellated draw.
const DYNAMIC_HS: u32 = 1 << 8;
/// Its `HS_W32_EN`, bit 21: the hull shader's waves are thirty-two lanes wide.
const HS_W32_EN: u32 = 1 << 21;

/// `VGT_LS_HS_CONFIG` (context `0xA2D6`): `NUM_PATCHES` 7:0, `HS_NUM_INPUT_CP` 13:8,
/// `HS_NUM_OUTPUT_CP` 19:14.
pub const VGT_LS_HS_CONFIG: u32 = 0xA2D6;
/// `VGT_TF_PARAM` (context `0xA2DB`): `TYPE` 1:0, `PARTITIONING` 4:2, `TOPOLOGY` 7:5,
/// `DISABLE_DONUTS` 14, `DISTRIBUTION_MODE` 18:17, `DETECT_ONE` 19, `DETECT_ZERO` 20.
pub const VGT_TF_PARAM: u32 = 0xA2DB;
/// `VGT_TF_RING_SIZE` (uconfig `0xC24E`): `SIZE` 15:0, in dwords - Mesa writes the ring's bytes
/// over four (`ac_cmdbuf_cp.c:346`, `:359-360`).
pub const VGT_TF_RING_SIZE: u32 = 0xC24E;
/// `VGT_HS_OFFCHIP_PARAM` (uconfig `0xC24F`): `OFFCHIP_BUFFERING` 9:0, `OFFCHIP_GRANULARITY` 11:10.
pub const VGT_HS_OFFCHIP_PARAM: u32 = 0xC24F;
/// `VGT_TF_MEMORY_BASE` (uconfig `0xC250`): the ring's address over 256 (`ac_cmdbuf_cp.c:361`).
pub const VGT_TF_MEMORY_BASE: u32 = 0xC250;
/// `VGT_TF_MEMORY_BASE_HI` (uconfig `0xC261`): the ring's address bits 47:40
/// (`ac_cmdbuf_cp.c:366`).
pub const VGT_TF_MEMORY_BASE_HI: u32 = 0xC261;
/// `SPI_SHADER_PGM_LO_HS` (`0x2D08`): the hull program's address over 256.
pub const SPI_SHADER_PGM_LO_HS: u32 = 0x2D08;
/// `SPI_SHADER_PGM_HI_HS` (`0x2D09`): its address bits 47:40.
pub const SPI_SHADER_PGM_HI_HS: u32 = 0x2D09;
/// `SPI_SHADER_PGM_LO_LS` (`0x2D48`): the local program's address over 256.
pub const SPI_SHADER_PGM_LO_LS: u32 = 0x2D48;
/// `SPI_SHADER_PGM_HI_LS` (`0x2D49`): its address bits 47:40.
pub const SPI_SHADER_PGM_HI_LS: u32 = 0x2D49;

/// The patch's parameter space (`VGT_TF_PARAM.TYPE`, enum `VGT_TESS_TYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// `TESS_ISOLINE`: lines along `u`, stacked along `v`.
    Isoline,
    /// `TESS_TRIANGLE`: barycentric `(u, v, w)`.
    Triangle,
    /// `TESS_QUAD`: `(u, v)` over the unit square.
    Quad,
}

/// How a factor divides an edge (`VGT_TF_PARAM.PARTITIONING`, enum `VGT_TESS_PARTITION`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Partitioning {
    /// `PART_INTEGER`: the factor rounded up, equal segments.
    Integer,
    /// `PART_POW2`: tessellated as integer; the power of two is the hull shader's to round to.
    Pow2,
    /// `PART_FRAC_ODD`: an odd count of segments, the outer two growing from nothing.
    FractionalOdd,
    /// `PART_FRAC_EVEN`: an even count of segments, likewise.
    FractionalEven,
}

/// What the tessellator emits (`VGT_TF_PARAM.TOPOLOGY`, enum `VGT_TESS_TOPOLOGY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTopology {
    /// `OUTPUT_POINT`.
    Point,
    /// `OUTPUT_LINE`.
    Line,
    /// `OUTPUT_TRIANGLE_CW`.
    TriangleCw,
    /// `OUTPUT_TRIANGLE_CCW`.
    TriangleCcw,
}

/// Whether `VGT_SHADER_STAGES_EN` runs any tessellation stage: the local or hull shader, or a
/// domain shader in the vertex or ES slot. A stream that never wrote it runs none.
#[must_use]
pub fn tessellated(stages: Option<u32>) -> bool {
    stages.is_some_and(|value| {
        value & (LS_EN | HS_EN) != 0
            || (value >> ES_EN_SHIFT) & 3 == STAGE_DS
            || (value >> VS_EN_SHIFT) & 3 == STAGE_DS
    })
}

/// A tessellated draw's fixed-function configuration, decoded from its registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// The patch's parameter space.
    pub domain: Domain,
    /// How factors divide edges.
    pub partitioning: Partitioning,
    /// What the tessellator emits.
    pub topology: OutputTopology,
    /// Control points the hull shader reads per patch, 1 to 32.
    pub input_control_points: u32,
    /// Control points the hull shader writes per patch, 1 to 32.
    pub output_control_points: u32,
    /// Patches per hull-shader thread group (`NUM_PATCHES`).
    pub patches_per_group: u32,
    /// The hull shader's waves are thirty-two lanes wide.
    pub hull_w32: bool,
    /// `DYNAMIC_HS` is set.
    pub dynamic_hull: bool,
    /// `VGT_TF_PARAM`'s bits besides the shape's three fields, kept to name the draw: how patches
    /// are spread over domain-shader waves, and how factors of one and zero are detected.
    pub distribution: u32,
}

/// What [`shape`] cannot decode, named.
fn field_name(domain: u32, partitioning: u32, topology: u32) -> Option<String> {
    if domain == 3 {
        return Some(format!("VGT_TF_PARAM.TYPE {domain} is reserved"));
    }
    if partitioning > 3 {
        return Some(format!(
            "VGT_TF_PARAM.PARTITIONING {partitioning} is reserved"
        ));
    }
    if topology > 3 {
        return Some(format!("VGT_TF_PARAM.TOPOLOGY {topology} is reserved"));
    }
    None
}

/// A tessellated draw's shape from the registers `latest` reads at the draw, or why it is not one
/// this decodes: a reserved field value, a control-point count of zero, or a domain and output
/// that do not go together (lines from a triangle or quad, triangles from an isoline).
///
/// # Errors
///
/// The reason, as a sentence fragment naming the register and value.
pub fn shape(mut latest: impl FnMut(u32) -> Option<u32>) -> Result<Shape, String> {
    let stages = latest(VGT_SHADER_STAGES_EN).unwrap_or(0);
    if stages & HS_EN == 0 {
        return Err(format!(
            "VGT_SHADER_STAGES_EN {stages:#x} runs a tessellation stage without the hull shader"
        ));
    }
    let param = latest(VGT_TF_PARAM).ok_or("the stream set no VGT_TF_PARAM")?;
    let (domain, partitioning, topology) = (param & 3, (param >> 2) & 7, (param >> 5) & 7);
    if let Some(why) = field_name(domain, partitioning, topology) {
        return Err(why);
    }
    let config = latest(VGT_LS_HS_CONFIG).ok_or("the stream set no VGT_LS_HS_CONFIG")?;
    let input_control_points = (config >> 8) & 0x3F;
    let output_control_points = (config >> 14) & 0x3F;
    if input_control_points == 0 || output_control_points == 0 {
        return Err(format!(
            "VGT_LS_HS_CONFIG {config:#x} has a patch of no control points"
        ));
    }
    let domain = [Domain::Isoline, Domain::Triangle, Domain::Quad][domain as usize];
    let topology = [
        OutputTopology::Point,
        OutputTopology::Line,
        OutputTopology::TriangleCw,
        OutputTopology::TriangleCcw,
    ][topology as usize];
    // D3D11's pairs: an isoline emits points or lines, a triangle or quad points or triangles.
    let paired = match domain {
        Domain::Isoline => matches!(topology, OutputTopology::Point | OutputTopology::Line),
        Domain::Triangle | Domain::Quad => topology != OutputTopology::Line,
    };
    if !paired {
        return Err(format!(
            "VGT_TF_PARAM {param:#x} asks a {domain:?} domain for {topology:?} output"
        ));
    }
    Ok(Shape {
        domain,
        partitioning: [
            Partitioning::Integer,
            Partitioning::Pow2,
            Partitioning::FractionalOdd,
            Partitioning::FractionalEven,
        ][partitioning as usize],
        topology,
        input_control_points,
        output_control_points,
        patches_per_group: config & 0xFF,
        hull_w32: stages & HS_W32_EN != 0,
        dynamic_hull: stages & DYNAMIC_HS != 0,
        distribution: param & !0xFF,
    })
}

/// Why a draw is not drawn, when its registers run a tessellation stage: `None` for a draw that
/// runs none. The reason names the draw's shape, decoded as far as it goes, so a run's report says
/// which tessellated draws a title issues.
pub fn refusal(mut latest: impl FnMut(u32) -> Option<u32>) -> Option<String> {
    if !tessellated(latest(VGT_SHADER_STAGES_EN)) {
        return None;
    }
    Some(match shape(latest) {
        Ok(shape) => format!(
            concat!(
                "a tessellated draw ({:?} domain, {:?} partitioning, {:?} output, {} to {} control ",
                "points, {} patches a group) is not drawn: no hull stage runs yet (D928)"
            ),
            shape.domain,
            shape.partitioning,
            shape.topology,
            shape.input_control_points,
            shape.output_control_points,
            shape.patches_per_group,
        ),
        Err(why) => format!("a tessellated draw is not drawn: {why} (D928)"),
    })
}

/// `VGT_HS_OFFCHIP_PARAM` decoded (`gfx103.json`; Mesa `ac_gpu_info.c:1413-1415` writes one less
/// than the work groups in `OFFCHIP_BUFFERING` on this generation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffchipParam {
    /// Hull-shader work groups the off-chip buffer holds at once.
    pub work_groups: u32,
    /// Dwords of off-chip memory each work group has (`OFFCHIP_GRANULARITY`: 8K, 4K, 2K or 1K).
    pub dwords_per_group: u32,
}

impl OffchipParam {
    /// The register's fields.
    #[must_use]
    pub fn decode(value: u32) -> Self {
        Self {
            work_groups: (value & 0x3FF) + 1,
            dwords_per_group: 8192 >> ((value >> 10) & 3),
        }
    }
}

/// Where the tessellator reads factors from: `VGT_TF_MEMORY_BASE` and `_HI`, and the ring's size
/// in bytes from `VGT_TF_RING_SIZE`. `None` until the stream set the base.
pub fn factor_ring(mut latest: impl FnMut(u32) -> Option<u32>) -> Option<(u64, u64)> {
    let base = u64::from(latest(VGT_TF_MEMORY_BASE)?) << 8
        | u64::from(latest(VGT_TF_MEMORY_BASE_HI).unwrap_or(0) & 0xFF) << 40;
    let size = u64::from(latest(VGT_TF_RING_SIZE).unwrap_or(0) & 0xFFFF) * 4;
    Some((base, size))
}

/// One patch's factors as the hull shader stores them to the TF ring, in the tessellator's order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PatchFactors {
    /// The segments along each line, then the count of lines.
    Isoline {
        /// Segments along each line, the ring's first dword.
        detail: f32,
        /// Lines, the ring's second.
        density: f32,
    },
    /// The edges where `u`, `v` and `w` are zero, then the inside.
    Triangle {
        /// Edge factors.
        outside: [f32; 3],
        /// The inside factor.
        inside: f32,
    },
    /// The edges `u=0, v=0, u=1, v=1`, then the inside along `u` and `v`.
    Quad {
        /// Edge factors.
        outside: [f32; 4],
        /// Inside factors.
        inside: [f32; 2],
    },
}

impl Domain {
    /// Dwords one patch's factors take in the TF ring: the domain's outer and inner factors
    /// (Mesa `ac_nir_lower_tess_io_to_mem.c:1140-1141`, `mesa_count_tess_level_components`).
    #[must_use]
    pub fn factor_dwords(self) -> usize {
        match self {
            Domain::Isoline => 2,
            Domain::Triangle => 4,
            Domain::Quad => 6,
        }
    }
}

/// The factors of the patch `relative_patch` patches into `ring` - the TF ring's dwords from where
/// the hull shader's work group stores, its `tess_factors_offset` - or `None` past its end.
///
/// The layout is radeonsi's on this generation (`ac_nir_lower_tess_io_to_mem.c:1126-1165`): no
/// leading control word past GFX8, each patch's factors packed at `relative_patch` times
/// [`Domain::factor_dwords`], outer before inner, and an isoline's two outer factors stored in the
/// reverse of GLSL's order, line detail first.
#[must_use]
pub fn patch_factors(domain: Domain, ring: &[u32], relative_patch: usize) -> Option<PatchFactors> {
    let stride = domain.factor_dwords();
    let words = ring.get(relative_patch * stride..(relative_patch + 1) * stride)?;
    let float = |n: usize| f32::from_bits(words[n]);
    Some(match domain {
        Domain::Isoline => PatchFactors::Isoline {
            detail: float(0),
            density: float(1),
        },
        Domain::Triangle => PatchFactors::Triangle {
            outside: [float(0), float(1), float(2)],
            inside: float(3),
        },
        Domain::Quad => PatchFactors::Quad {
            outside: [float(0), float(1), float(2), float(3)],
            inside: [float(4), float(5)],
        },
    })
}

impl tessellator::Tessellator {
    /// Tessellates one patch of `factors`, whose domain is the one they were read for.
    pub fn patch(&mut self, factors: PatchFactors) {
        match factors {
            PatchFactors::Isoline { detail, density } => self.isoline(density, detail),
            PatchFactors::Triangle { outside, inside } => self.triangle(outside, inside),
            PatchFactors::Quad { outside, inside } => self.quad(outside, inside),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Domain, HS_EN, OffchipParam, OutputTopology, Partitioning, PatchFactors, VGT_LS_HS_CONFIG,
        VGT_SHADER_STAGES_EN, VGT_TF_MEMORY_BASE, VGT_TF_MEMORY_BASE_HI, VGT_TF_PARAM,
        VGT_TF_RING_SIZE, factor_ring, patch_factors, refusal, shape, tessellated, tessellator,
    };

    /// radeonsi's stages for a tessellated NGG draw on GFX10: `LS_STAGE_ON`, `HS_EN`, `DYNAMIC_HS`
    /// (`si_state_draw.cpp:109-111`) and `PRIMGEN_EN`.
    const TESSELLATED_NGG: u32 = 1 | HS_EN | 1 << 8 | 1 << 13;

    fn registers(pairs: &[(u32, u32)]) -> impl FnMut(u32) -> Option<u32> + '_ {
        |register| {
            pairs
                .iter()
                .find(|(r, _)| *r == register)
                .map(|&(_, value)| value)
        }
    }

    #[test]
    fn a_draw_with_no_tessellation_stage_is_not_tessellated() {
        assert!(!tessellated(None));
        // A primitive shader alone, and a legacy vertex shader alone.
        assert!(!tessellated(Some(1 << 13)));
        assert!(!tessellated(Some(0)));
        assert!(refusal(registers(&[(VGT_SHADER_STAGES_EN, 1 << 13)])).is_none());
    }

    #[test]
    fn every_tessellation_stage_enable_is_seen() {
        assert!(tessellated(Some(TESSELLATED_NGG)));
        assert!(tessellated(Some(HS_EN)));
        assert!(tessellated(Some(1)));
        // A domain shader in the ES slot, and in the vertex slot.
        assert!(tessellated(Some(1 << 3)));
        assert!(tessellated(Some(1 << 6)));
        // A real ES (`ES_STAGE_REAL`) and a copy shader (`VS_STAGE_COPY_SHADER`) are not.
        assert!(!tessellated(Some(2 << 3)));
        assert!(!tessellated(Some(2 << 6)));
    }

    #[test]
    fn a_tessellated_draw_is_refused_by_its_shape() {
        // A triangle domain, fractional odd, clockwise triangles; 3 control points in and out,
        // 4 patches a group.
        let param = 1 | 2 << 2 | 2 << 5;
        let config = 4 | 3 << 8 | 3 << 14;
        let pairs = [
            (VGT_SHADER_STAGES_EN, TESSELLATED_NGG),
            (VGT_TF_PARAM, param),
            (VGT_LS_HS_CONFIG, config),
        ];
        let decoded = shape(registers(&pairs)).unwrap();
        assert_eq!(decoded.domain, Domain::Triangle);
        assert_eq!(decoded.partitioning, Partitioning::FractionalOdd);
        assert_eq!(decoded.topology, OutputTopology::TriangleCw);
        assert_eq!(
            (decoded.input_control_points, decoded.output_control_points),
            (3, 3)
        );
        assert_eq!(decoded.patches_per_group, 4);
        assert!(decoded.dynamic_hull);
        assert_eq!(
            refusal(registers(&pairs)).unwrap(),
            concat!(
                "a tessellated draw (Triangle domain, FractionalOdd partitioning, TriangleCw ",
                "output, 3 to 3 control points, 4 patches a group) is not drawn: no hull stage ",
                "runs yet (D928)"
            )
        );
    }

    #[test]
    fn shapes_that_do_not_decode_are_named() {
        let stages = (VGT_SHADER_STAGES_EN, TESSELLATED_NGG);
        let config = (VGT_LS_HS_CONFIG, 3 << 8 | 3 << 14);
        let refused = |pairs: &[(u32, u32)]| refusal(registers(pairs)).unwrap();
        assert!(refused(&[stages, config]).contains("no VGT_TF_PARAM"));
        assert!(refused(&[stages, (VGT_TF_PARAM, 3), config]).contains("TYPE 3 is reserved"));
        assert!(
            refused(&[stages, (VGT_TF_PARAM, 1 | 5 << 2), config])
                .contains("PARTITIONING 5 is reserved")
        );
        assert!(
            refused(&[stages, (VGT_TF_PARAM, 1 | 6 << 5), config])
                .contains("TOPOLOGY 6 is reserved")
        );
        // Triangles from an isoline, and lines from a quad.
        assert!(refused(&[stages, (VGT_TF_PARAM, 2 << 5), config]).contains("Isoline domain"));
        assert!(refused(&[stages, (VGT_TF_PARAM, 2 | 1 << 5), config]).contains("Quad domain"));
        assert!(
            refused(&[stages, (VGT_TF_PARAM, 1), (VGT_LS_HS_CONFIG, 3 << 8)])
                .contains("no control points")
        );
        // A domain shader with no hull shader.
        assert!(refused(&[(VGT_SHADER_STAGES_EN, 1 << 6)]).contains("without the hull shader"));
    }

    #[test]
    fn the_offchip_parameter_counts_groups_and_their_dwords() {
        assert_eq!(
            OffchipParam::decode(0x1FF | 3 << 10),
            OffchipParam {
                work_groups: 512,
                dwords_per_group: 1024
            }
        );
        assert_eq!(OffchipParam::decode(0).dwords_per_group, 8192);
    }

    #[test]
    fn the_factor_ring_is_its_base_over_256_and_its_dwords() {
        let pairs = [
            (VGT_TF_MEMORY_BASE, 0x0123_4567),
            (VGT_TF_MEMORY_BASE_HI, 0x89),
            (VGT_TF_RING_SIZE, 0x4000),
        ];
        assert_eq!(
            factor_ring(registers(&pairs)),
            Some((0x8901_2345_6700, 0x1_0000))
        );
        assert_eq!(factor_ring(registers(&[])), None);
    }

    #[test]
    fn patch_factors_are_packed_per_patch_outer_before_inner() {
        let ring: Vec<u32> = (0..12u16).map(|n| f32::from(n).to_bits()).collect();
        assert_eq!(
            patch_factors(Domain::Triangle, &ring, 1),
            Some(PatchFactors::Triangle {
                outside: [4.0, 5.0, 6.0],
                inside: 7.0
            })
        );
        assert_eq!(
            patch_factors(Domain::Quad, &ring, 1),
            Some(PatchFactors::Quad {
                outside: [6.0, 7.0, 8.0, 9.0],
                inside: [10.0, 11.0]
            })
        );
        assert_eq!(
            patch_factors(Domain::Isoline, &ring, 5),
            Some(PatchFactors::Isoline {
                detail: 10.0,
                density: 11.0
            })
        );
        assert_eq!(patch_factors(Domain::Quad, &ring, 2), None);
    }

    #[test]
    fn a_patch_from_the_ring_tessellates_in_its_domain() {
        // Line detail 3 then density 2, as the ring holds them: two lines of three segments.
        let ring = [3.0f32.to_bits(), 2.0f32.to_bits()];
        let factors = patch_factors(Domain::Isoline, &ring, 0).unwrap();
        let mut tessellator =
            tessellator::Tessellator::new(Partitioning::Integer, OutputTopology::Line);
        tessellator.patch(factors);
        assert_eq!(tessellator.points().len(), 8);
        assert_eq!(tessellator.indices().len(), 12);
    }
}
