//! The system values a pixel shader finds in its vector registers at entry.
//!
//! The hardware loads a pixel wave's inputs into its first vector registers in the field order of
//! `SPI_PS_INPUT_ENA` (Mesa `src/amd/registers/gfx103.json`): the barycentric pairs, then position,
//! facing, the ancillary word, coverage and the fixed-point position. `SPI_PS_INPUT_ADDR` decides
//! which fields take registers and `SPI_PS_INPUT_ENA` which are loaded. Interpolation reads host
//! inputs directly, so only the system values after the barycentrics are seeded.

use orbistoun_spirv::{Builder, Id, built_in, capability, decoration, op};

use super::{INPUT, Stage, Wavefront};
use crate::TranslateError;
use crate::model::Model;

/// A pixel shader's `SPI_PS_INPUT_ENA` and `SPI_PS_INPUT_ADDR`, as the stream last wrote them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PixelInputs {
    /// `SPI_PS_INPUT_ENA`: which fields the hardware loads.
    pub enable: u32,
    /// `SPI_PS_INPUT_ADDR`: which fields take a register. Zero means the same as `enable`, as Mesa
    /// reads a shader that sets none (`src/amd/common/ac_binary.c` line 101).
    pub address: u32,
}

/// A value the translated module places in a vector register at entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemValue {
    /// `gl_FragCoord.x`, the pixel centre.
    PositionX,
    /// `gl_FragCoord.y`.
    PositionY,
    /// `gl_FragCoord.z`, the depth.
    PositionZ,
    /// Clip-space `w`: Mesa reads this register as `frag_coord_w_rcp` and takes its reciprocal for
    /// `gl_FragCoord.w` (`ac_nir_lower_intrinsics_to_args.c` line 206, `nir_builder.c` line 809).
    PositionW,
    /// `+1.0` for a front face and `-1.0` for a back face: Mesa tests the register as a float
    /// greater than zero, and reads it directly as `front_face_fsign`
    /// (`ac_nir_lower_intrinsics_to_args.c` lines 361 and 364).
    FrontFace,
    /// The render-target layer in bits 16 upward, thirteen bits on this part
    /// (`radv_nir_lower_abi.c` line 308); the other bits are zero.
    Ancillary,
}

/// One seeded register: which value, and which vector register it lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seeded {
    /// The value.
    pub value: SystemValue,
    /// The vector register.
    pub register: u32,
}

/// What the module does with one field.
#[derive(Debug, Clone, Copy)]
enum Role {
    /// A barycentric or stipple input, which the host's own interpolation replaces.
    Interpolant,
    /// A system value the module seeds.
    System(SystemValue),
    /// A field whose content is unmeasured, refused by name when enabled.
    Refused(&'static str),
}

/// Each field in bit order, with its register count: Mesa's `ac_get_fs_input_vgpr_cnt`
/// (`src/amd/common/ac_shader_util.c` line 605) and `declare_ps_input_vgprs`
/// (`src/amd/vulkan/radv_shader_args.c` line 336).
const FIELDS: [(u32, Role); 16] = [
    (2, Role::Interpolant), // PERSP_SAMPLE
    (2, Role::Interpolant), // PERSP_CENTER
    (2, Role::Interpolant), // PERSP_CENTROID
    (3, Role::Interpolant), // PERSP_PULL_MODEL
    (2, Role::Interpolant), // LINEAR_SAMPLE
    (2, Role::Interpolant), // LINEAR_CENTER
    (2, Role::Interpolant), // LINEAR_CENTROID
    (1, Role::Interpolant), // LINE_STIPPLE_TEX
    (1, Role::System(SystemValue::PositionX)),
    (1, Role::System(SystemValue::PositionY)),
    (1, Role::System(SystemValue::PositionZ)),
    (1, Role::System(SystemValue::PositionW)),
    (1, Role::System(SystemValue::FrontFace)),
    (1, Role::System(SystemValue::Ancillary)),
    (
        1,
        Role::Refused(
            "the pixel shader enables SAMPLE_COVERAGE, whose register content is unmeasured",
        ),
    ),
    (
        1,
        Role::Refused(
            "the pixel shader enables POS_FIXED_PT, whose register content is unmeasured",
        ),
    ),
];

/// The fields from `POS_X_FLOAT` upward, whose placement a mismatch between the two registers
/// would make ambiguous.
const SYSTEM_FIELDS: u32 = 0xff00;

/// Bits of the ancillary word the layer occupies on this part.
const LAYER_BITS: u32 = 13;

/// Where the layer starts in the ancillary word.
const LAYER_SHIFT: u32 = 16;

impl PixelInputs {
    /// The system values to seed, in field order, with their registers.
    ///
    /// # Errors
    ///
    /// When an unmeasured field is enabled, or a system value is enabled while `enable` has a field
    /// `address` gives no register, which leaves where the hardware puts it unknown.
    pub fn seeded(self) -> Result<Vec<Seeded>, TranslateError> {
        let address = if self.address == 0 {
            self.enable
        } else {
            self.address
        };
        if self.enable & SYSTEM_FIELDS != 0 && self.enable & !address != 0 {
            return Err(refusal(concat!(
                "SPI_PS_INPUT_ENA enables a field SPI_PS_INPUT_ADDR gives no register, so where ",
                "the system values land is unknown"
            )));
        }
        let mut seeded = Vec::new();
        let mut register = 0;
        for (bit, (registers, role)) in (0u32..).zip(FIELDS) {
            let addressed = address >> bit & 1 != 0;
            let enabled = self.enable >> bit & 1 != 0;
            match role {
                Role::Refused(detail) if enabled => return Err(refusal(detail)),
                Role::System(value) if enabled && addressed => {
                    seeded.push(Seeded { value, register });
                }
                _ => {}
            }
            if addressed {
                register += registers;
            }
        }
        Ok(seeded)
    }
}

fn refusal(detail: &'static str) -> TranslateError {
    TranslateError::Unsupported { offset: 0, detail }
}

/// The built-in inputs a fragment module reads its system values from, reserved before the entry
/// point is written because its interface names them.
#[derive(Debug, Clone, Default)]
pub(super) struct SystemInputs {
    seeded: Vec<Seeded>,
    frag_coord: Option<Id>,
    front_facing: Option<Id>,
    layer: Option<Id>,
    /// The signed integer type `Layer` is declared with, once declared.
    layer_type: Option<Id>,
}

impl SystemInputs {
    /// Reserves a variable for each built-in a fragment module's seeded values read; nothing at
    /// another stage. `translate_with_user_data` refuses a layout `seeded` refuses before this.
    pub(super) fn reserve(b: &mut Builder, stage: Stage, inputs: Option<PixelInputs>) -> Self {
        let seeded = match (stage, inputs) {
            (Stage::Fragment, Some(inputs)) => inputs.seeded().unwrap_or_default(),
            _ => Vec::new(),
        };
        let reads = |wanted: &[SystemValue]| seeded.iter().any(|s| wanted.contains(&s.value));
        let frag_coord = reads(&[
            SystemValue::PositionX,
            SystemValue::PositionY,
            SystemValue::PositionZ,
            SystemValue::PositionW,
        ])
        .then(|| b.id());
        let front_facing = reads(&[SystemValue::FrontFace]).then(|| b.id());
        let layer = reads(&[SystemValue::Ancillary]).then(|| b.id());
        Self {
            seeded,
            frag_coord,
            front_facing,
            layer,
            layer_type: None,
        }
    }

    /// The variables the entry point lists.
    pub(super) fn interface(&self) -> impl Iterator<Item = u32> + '_ {
        [self.frag_coord, self.front_facing, self.layer]
            .into_iter()
            .flatten()
            .map(|id| id.0)
    }

    /// Declares the reserved variables: `vec4` and `bool` are the module's own types.
    pub(super) fn declare(&mut self, b: &mut Builder, vec4: Id, bool_type: Id) {
        let input = |b: &mut Builder, variable: Id, pointee: Id, built_in: u32| {
            let pointer = b.id();
            b.declare(op::TYPE_POINTER, &[pointer.0, INPUT, pointee.0]);
            b.declare(op::VARIABLE, &[pointer.0, variable.0, INPUT]);
            b.annotate(op::DECORATE, &[variable.0, decoration::BUILT_IN, built_in]);
        };
        if let Some(variable) = self.frag_coord {
            input(b, variable, vec4, built_in::FRAG_COORD);
        }
        if let Some(variable) = self.front_facing {
            input(b, variable, bool_type, built_in::FRONT_FACING);
        }
        if let Some(variable) = self.layer {
            // Declared a signed integer as the reference compiler does, and `Flat` as every integer
            // fragment input must be; reading it in a fragment shader is a geometry capability.
            b.header(op::CAPABILITY, &[capability::GEOMETRY]);
            let i32_type = b.id();
            b.declare(op::TYPE_INT, &[i32_type.0, 32, 1]);
            input(b, variable, i32_type, built_in::LAYER);
            b.annotate(op::DECORATE, &[variable.0, decoration::FLAT]);
            self.layer_type = Some(i32_type);
        }
    }

    /// Stores each seeded value into its register, at entry.
    pub(super) fn seed(&self, module: &mut Wavefront<'_>) {
        for seeded in &self.seeded {
            let value = self.value(module, seeded.value);
            module.store_lane_masked(seeded.register, 0, value);
        }
    }

    /// One system value, as the register's 32 bits.
    fn value(&self, module: &mut Wavefront<'_>, value: SystemValue) -> Id {
        let u32_type = module.u32_type;
        match value {
            SystemValue::PositionX
            | SystemValue::PositionY
            | SystemValue::PositionZ
            | SystemValue::PositionW => {
                let component = match value {
                    SystemValue::PositionX => 0,
                    SystemValue::PositionY => 1,
                    SystemValue::PositionZ => 2,
                    _ => 3,
                };
                let coordinate = Self::load(module, self.frag_coord, module.vec4);
                let f32_type = module.f32_type;
                let b = &mut module.builder;
                let mut float = b.id();
                b.function(
                    op::COMPOSITE_EXTRACT,
                    &[f32_type.0, float.0, coordinate.0, component],
                );
                if value == SystemValue::PositionW {
                    // The host's `w` is the reciprocal of the register's.
                    let one_bits = module.constant(0x3f80_0000);
                    let one = module.as_float(one_bits);
                    let b = &mut module.builder;
                    let reciprocal = b.id();
                    b.function(op::FDIV, &[f32_type.0, reciprocal.0, one.0, float.0]);
                    float = reciprocal;
                }
                let b = &mut module.builder;
                let bits = b.id();
                b.function(op::BITCAST, &[u32_type.0, bits.0, float.0]);
                bits
            }
            SystemValue::FrontFace => {
                let front = Self::load(module, self.front_facing, module.bool_type);
                let plus = module.constant(0x3f80_0000);
                let minus = module.constant(0xbf80_0000);
                let b = &mut module.builder;
                let bits = b.id();
                b.function(op::SELECT, &[u32_type.0, bits.0, front.0, plus.0, minus.0]);
                bits
            }
            SystemValue::Ancillary => {
                let layer_type = self.layer_type.unwrap_or(u32_type);
                let layer = Self::load(module, self.layer, layer_type);
                let mask = module.constant((1 << LAYER_BITS) - 1);
                let shift = module.constant(LAYER_SHIFT);
                let b = &mut module.builder;
                let unsigned = b.id();
                b.function(op::BITCAST, &[u32_type.0, unsigned.0, layer.0]);
                let masked = b.id();
                b.function(op::BITWISE_AND, &[u32_type.0, masked.0, unsigned.0, mask.0]);
                let placed = b.id();
                b.function(
                    op::SHIFT_LEFT_LOGICAL,
                    &[u32_type.0, placed.0, masked.0, shift.0],
                );
                placed
            }
        }
    }

    /// Loads a reserved variable; `reserve` declared every variable a seeded value reads.
    fn load(module: &mut Wavefront<'_>, variable: Option<Id>, pointee: Id) -> Id {
        let variable = variable.unwrap_or(Id(0));
        let b = &mut module.builder;
        let loaded = b.id();
        b.function(op::LOAD, &[pointee.0, loaded.0, variable.0]);
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::{PixelInputs, Seeded, SystemValue};
    use crate::Width;
    use crate::wavefront::{MeshPrimitive, Stage, UserData, Window, translate_with_user_data};
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
    use orbistoun_spirv::{built_in, capability, decoration, op};

    fn seeded(enable: u32, address: u32) -> Vec<(SystemValue, u32)> {
        PixelInputs { enable, address }
            .seeded()
            .unwrap_or_else(|e| panic!("{enable:#x}/{address:#x} refused: {e}"))
            .into_iter()
            .map(|Seeded { value, register }| (value, register))
            .collect()
    }

    /// Position follows the one barycentric pair a typical shader enables, a register each.
    #[test]
    fn position_follows_the_centre_barycentrics() {
        assert_eq!(
            seeded(0x302, 0x302),
            [(SystemValue::PositionX, 2), (SystemValue::PositionY, 3)]
        );
    }

    /// Every seeded field, in field order, one register each after the pair.
    #[test]
    fn every_system_value_lands_in_field_order() {
        assert_eq!(
            seeded(0x3f02, 0x3f02),
            [
                (SystemValue::PositionX, 2),
                (SystemValue::PositionY, 3),
                (SystemValue::PositionZ, 4),
                (SystemValue::PositionW, 5),
                (SystemValue::FrontFace, 6),
                (SystemValue::Ancillary, 7),
            ]
        );
    }

    /// A field the address register places but the enable register leaves off still takes its
    /// register, and is not seeded.
    #[test]
    fn an_addressed_field_that_is_not_enabled_takes_its_register_unseeded() {
        assert_eq!(seeded(0x2002, 0x2302), [(SystemValue::Ancillary, 4)]);
    }

    /// An address register of zero reads as the enable register.
    #[test]
    fn a_zero_address_is_the_enable() {
        assert_eq!(seeded(0x1001, 0), [(SystemValue::FrontFace, 2)]);
    }

    /// The pull-model field takes three registers and the linear pairs two each.
    #[test]
    fn pull_model_takes_three_registers() {
        assert_eq!(seeded(0x0118, 0x0118), [(SystemValue::PositionX, 5)]);
        assert_eq!(seeded(0x01ff, 0x01ff), [(SystemValue::PositionX, 16)]);
    }

    /// No system field enabled seeds nothing, whatever the barycentrics.
    #[test]
    fn barycentrics_alone_seed_nothing() {
        assert_eq!(seeded(0x2, 0x2), []);
        assert_eq!(seeded(0x3, 0x2), []);
    }

    /// The unmeasured fields are refused when enabled, and so is a layout whose enable register
    /// names a field the address register gives no register while a system value is seeded.
    #[test]
    fn unmeasured_fields_and_ambiguous_layouts_are_refused() {
        for (enable, address) in [(0x4002, 0x4002), (0x8002, 0x8002), (0x303, 0x302)] {
            assert!(
                PixelInputs { enable, address }.seeded().is_err(),
                "{enable:#x}/{address:#x} was accepted"
            );
        }
        // Addressed but not enabled: the register exists and holds nothing the shader asked for.
        assert!(
            PixelInputs {
                enable: 0x2,
                address: 0xc002
            }
            .seeded()
            .is_ok()
        );
    }

    /// Each instruction's opcode and operands, from the header on.
    fn instructions(module: &[u32]) -> Vec<(u16, &[u32])> {
        let mut found = Vec::new();
        let mut at = 5;
        while at < module.len() {
            let length = (module[at] >> 16) as usize;
            let opcode = (module[at] & 0xffff) as u16;
            found.push((opcode, &module[at + 1..at + length.max(1)]));
            at += length.max(1);
        }
        found
    }

    fn translated(enable: u32) -> Vec<u32> {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let decode = decode_program(&0xBF81_0000u32.to_le_bytes(), &encodings, &operands);
        translate_with_user_data(
            &decode,
            &encodings,
            Width::default(),
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                pixel_inputs: Some(PixelInputs {
                    enable,
                    address: enable,
                }),
                ..UserData::default()
            },
        )
        .unwrap_or_else(|e| panic!("{enable:#x}: {e}"))
        .0
    }

    /// A module declares the built-in each seeded value reads and no other, and asks for the
    /// geometry capability only when it reads the layer.
    #[test]
    fn a_module_declares_only_the_built_ins_it_reads() {
        let built_ins = |module: &[u32]| {
            instructions(module)
                .into_iter()
                .filter(|(opcode, operands)| {
                    *opcode == op::DECORATE && operands.get(1) == Some(&decoration::BUILT_IN)
                })
                .map(|(_, operands)| operands[2])
                .collect::<Vec<_>>()
        };
        let geometry = |module: &[u32]| {
            instructions(module).into_iter().any(|(opcode, operands)| {
                opcode == op::CAPABILITY && operands == [capability::GEOMETRY]
            })
        };

        let position = translated(0x302);
        assert_eq!(built_ins(&position), [built_in::FRAG_COORD]);
        assert!(!geometry(&position));

        let everything = translated(0x3f02);
        let mut declared = built_ins(&everything);
        declared.sort_unstable();
        assert_eq!(
            declared,
            [
                built_in::LAYER,
                built_in::FRAG_COORD,
                built_in::FRONT_FACING
            ]
        );
        assert!(geometry(&everything));

        assert_eq!(built_ins(&translated(0x2)), []);
    }
}
