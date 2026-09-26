//! What every wavefront model provides, and the instruction dispatch they share.
//!
//! An instruction is translated once, here, against the [`Model`] trait, so the fidelity levels
//! cannot disagree about what an instruction means. The models differ only in how many lanes an
//! invocation emits, where a register lives, and whether a write is masked.

use orbistoun_shader::{EncodingTable, Instruction, Operand};
use orbistoun_spirv::{Builder, Id, image_operands, op};

use crate::TranslateError;
use crate::modifiers::Modifiers;

/// Every instruction the translator understands, by name (D139).
///
/// Opcode numbers belong to one architecture generation and move between generations, so a list of
/// numbers retargeted elsewhere binds silently to whatever occupies them. A name the loaded target
/// lacks is reported by [`unresolved`] instead. The names are the ones the reference assembler
/// prints, recorded by the probe solver alongside each opcode's operand layout.
pub const SUPPORTED: &[&str] = &[
    "exp",
    "v_interp_mov_f32_e32",
    "v_interp_p1_f32_e32",
    "v_interp_p2_f32_e32",
    "buffer_load_dword",
    "buffer_load_dwordx2",
    "buffer_load_dwordx3",
    "buffer_load_dwordx4",
    "buffer_store_dword",
    "buffer_store_dwordx2",
    "buffer_store_dwordx3",
    "buffer_store_dwordx4",
    "tbuffer_load_format_x",
    "tbuffer_load_format_xy",
    "tbuffer_load_format_xyz",
    "tbuffer_load_format_xyzw",
    "tbuffer_store_format_x",
    "tbuffer_store_format_xy",
    "tbuffer_store_format_xyz",
    "tbuffer_store_format_xyzw",
    "ds_read_b32",
    "ds_write_b32",
    "image_load",
    "image_sample",
    "image_sample_l",
    "image_sample_lz",
    "image_store",
    "global_load_dword",
    "global_load_dwordx2",
    "global_load_dwordx4",
    "global_store_dword",
    "global_store_dwordx2",
    "global_store_dwordx4",
    "s_add_i32",
    "s_addk_i32",
    "s_and_b32",
    "s_and_b64",
    "s_andn2_b64",
    "s_branch",
    "s_cbranch_execnz",
    "s_cbranch_execz",
    "s_cbranch_scc0",
    "s_cbranch_scc1",
    "s_cbranch_vccnz",
    "s_cbranch_vccz",
    "s_clause",
    "s_cmp_eq_i32",
    "s_cmp_ge_i32",
    "s_cmp_gt_i32",
    "s_cmp_le_i32",
    "s_cmp_lg_i32",
    "s_cmp_lt_i32",
    "s_cmpk_eq_i32",
    "s_cmpk_lg_i32",
    "s_endpgm",
    "s_load_dword",
    "s_load_dwordx2",
    "s_load_dwordx4",
    "s_load_dwordx8",
    "s_mov_b32",
    "s_nop",
    "s_sendmsg",
    "s_mov_b64",
    "s_movk_i32",
    "s_inst_prefetch",
    "s_mulk_i32",
    "s_or_b32",
    "s_or_b64",
    "s_sub_i32",
    "s_waitcnt",
    "s_wqm_b32",
    "s_wqm_b64",
    "s_xor_b32",
    "v_add_co_u32",
    "v_add_f32_e32",
    "v_add_f32_e64",
    "v_add_nc_u32_e32",
    "v_add_co_ci_u32_e32",
    "v_add_co_ci_u32_e64",
    "v_cmp_eq_f32_e32",
    "v_cmp_gt_f32_e32",
    "v_cmp_lt_f32_e32",
    "v_cmp_lt_u32_e32",
    "v_cndmask_b32_e64",
    "v_cos_f32_e32",
    "v_cvt_pkrtz_f16_f32_e32",
    "v_div_fixup_f32",
    "v_fmac_f32_e32",
    "v_div_scale_f32",
    "v_div_fmas_f32",
    "v_exp_f32_e32",
    "v_fma_f32",
    "v_log_f32_e32",
    "v_lshlrev_b32_e32",
    "v_lshrrev_b32_e32",
    "v_max_f32_e32",
    "v_mbcnt_hi_u32_b32",
    "v_mbcnt_lo_u32_b32",
    "v_min_f32_e32",
    "v_mov_b32_e32",
    "v_mul_f32_e32",
    "v_mul_f32_e64",
    "v_rcp_f32_e32",
    "v_rsq_f32_e32",
    "v_sin_f32_e32",
    "v_sqrt_f32_e32",
    "v_sub_co_u32",
    "v_sub_f32_e32",
    "v_sub_f32_e64",
    "v_subrev_f32_e32",
    "v_subrev_f32_e64",
];

/// Names this translator understands that the loaded generation does not have.
///
/// Empty on the target the tables were built for; after a retarget it names exactly what needs
/// attention.
pub fn unresolved(encodings: &EncodingTable) -> Vec<&'static str> {
    SUPPORTED
        .iter()
        .copied()
        .filter(|name| encodings.find_by_name(name).is_none())
        .collect()
}

/// Whether the translator understands the instruction at a family and opcode.
///
/// Resolved through the loaded table's names, so the answer follows the generation the tables were
/// generated for. An opcode the table cannot name is not supported: nothing has observed it.
pub fn supports_named(encodings: &EncodingTable, family: &str, opcode: u32) -> bool {
    encodings
        .mnemonic_for(family, opcode)
        .is_some_and(|name| SUPPORTED.contains(&name))
}

/// The operand code a flat access uses to say it has no scalar base register.
///
/// The reference assembler encodes `global_store_dword v[8:9], v10, off offset:4` as `0xdc708004
/// 0x007d0a08`, so the marker is 0x7d, which the operand table names `null`; the same bytes are in
/// `primitive.gcn`. Matched by name because the same code means different things in different
/// fields.
pub const FLAT_NO_BASE: &str = NO_DESTINATION;

/// The operand that names no register at all.
///
/// In an address field ([`FLAT_NO_BASE`]) the access has no base register; in a destination field
/// the result is not wanted and is written nowhere. The division pre-scale, for example, may
/// discard its flag this way, and translating that as a register write would invent a destination.
pub const NO_DESTINATION: &str = "null";

/// Whether a destination operand says the result is not wanted.
fn discards(operand: &Operand) -> bool {
    matches!(operand, Operand::Named(name) if name == NO_DESTINATION)
}

/// Scalar registers the guest has.
///
/// The operand numbering continues past this into specials and inline constants, so a scalar
/// destination at or above it is not a register. A wide load such as `s_load_dwordx8` at s100 would
/// otherwise write into specials.
pub const SCALAR_REGISTERS: u32 = 102;

/// Scalar registers an image descriptor occupies.
///
/// Eight, as the instruction's resource field names. This translation reads none of the fields (see
/// [`Model::sampled_image`]), but a write anywhere in the group means the descriptor is no longer
/// the one the last sample used.
pub const IMAGE_DESCRIPTOR_REGISTERS: u32 = 8;

/// Scalar registers a sampler descriptor occupies.
pub const SAMPLER_DESCRIPTOR_REGISTERS: u32 = 4;

/// Components a sampling instruction can return, one per bit of its mask.
const IMAGE_COMPONENTS: u32 = 4;

/// Where an image instruction says how many dimensions its coordinate has.
///
/// The field is in the instruction, not the descriptor. Assembling one instruction at each of the
/// eight dimensionalities and differencing the encodings puts it at these bits and nowhere else.
/// The disassembler prints it symbolically (`dim:SQ_RSRC_IMG_2D`), which the operand solver skips,
/// so it is read here directly.
const IMAGE_DIMENSION: (u32, u32) = (3, 0b111);

/// The dimensionality code for a two-dimensional image.
///
/// Codes run upward in the order a disassembler prints them: 1D, 2D, 3D, cube, 1D array, 2D array,
/// 2D multi-sampled, 2D multi-sampled array. Measured from the same eight encodings.
const IMAGE_DIMENSION_2D: u32 = 1;

/// What a translated texture sample needs from the model.
///
/// Four identifiers declared together on first use: the variable, what loading it produces, the
/// coordinate type, and the result type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texture {
    /// The variable holding the image and its sampler together.
    pub variable: Id,
    /// The type a load of [`variable`](Self::variable) produces.
    pub sampled: Id,
    /// The image type inside that, which a fetch takes and a sample does not.
    ///
    /// A guest's `image_load` reads a texel by integer coordinate with no sampler. On the host that
    /// is a fetch, which takes an image, so the bound sampled image is unwrapped to this and
    /// reading a texel needs no binding of its own.
    pub image: Id,
    /// The two-component float vector a two-dimensional sampling coordinate is.
    pub coordinate: Id,
    /// The two-component unsigned vector a two-dimensional texel coordinate is.
    ///
    /// A different type from [`coordinate`](Self::coordinate): a sample takes a normalised
    /// position, a fetch the texel's own index.
    pub texel: Id,
    /// The four-component float vector a sample or a fetch answers with.
    pub result: Id,
}

/// What a translated texture store needs from the model.
///
/// Separate from [`Texture`] because a storage image is a separate binding, and declaring it
/// declares a capability the device may lack (D692).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    /// The variable holding the storage image.
    pub variable: Id,
    /// The image type a load of [`variable`](Self::variable) produces.
    pub image: Id,
    /// The two-component unsigned vector a texel coordinate is.
    pub texel: Id,
    /// The four-component float vector a texel's value is.
    pub value: Id,
}

/// How the execution mask's low half arrives from the decoder.
///
/// A sixty-four-bit operand names its pair by the low register, which the operand table names as a
/// thirty-two-bit register, so `exec` decodes as `exec_lo` and the width comes from the opcode.
pub const EXEC_LOW_HALF: &str = "exec_lo";

/// How the condition mask's low half arrives from the decoder.
///
/// Where a comparison puts its answer, one bit per lane, which a shader then ands into the
/// execution mask.
pub const VCC_LOW_HALF: &str = "vcc_lo";

/// The `m0` register, as the operand table names it.
///
/// One word of scalar state outside the register file: a pixel shader passes the interpolator its
/// primitive mask through it, and a primitive shader its allocation counts. Held apart from the
/// scalar file because its operand code (124) is past the last scalar register.
pub const M0: &str = "m0";

/// A lane mask, by whichever spelling reached the translator.
///
/// A source field holding code 106 decodes through the operand table as `vcc_lo`; a comparison's
/// implicit destination comes from the operand layout as the reference printed it, `vcc`. Neither
/// carries the width, which comes from the opcode. Normalising here keeps both tables as observed.
pub fn lane_mask_name(name: &str) -> Option<&'static str> {
    match name {
        "exec" | EXEC_LOW_HALF => Some(EXEC_LOW_HALF),
        "vcc" | VCC_LOW_HALF => Some(VCC_LOW_HALF),
        _ => None,
    }
}

/// Whether an instruction reads or writes a lane mask.
///
/// Mostly a property of the operands: `s_mov_b64` needs a mask when its destination is `exec` and
/// not for an ordinary register pair, so keying on the opcode would push every 64-bit move onto the
/// slow model. Branches such as `s_cbranch_execz` name no mask in their operands, so for those the
/// opcode decides; missing them would make [`Fidelity::Auto`](crate::Fidelity::Auto) refuse a
/// shader it can translate. The family is required because an opcode number means nothing without
/// it.
pub fn touches_mask(instruction: &Instruction, name: &str) -> bool {
    let branches_on_a_mask = matches!(
        name,
        "s_cbranch_vccz" | "s_cbranch_vccnz" | "s_cbranch_execz" | "s_cbranch_execnz"
    );

    // `v_cndmask_b32` reads one bit of a mask per lane, and whole quad mode operates on a lane mask
    // whose operands may be ordinary register pairs; neither is visible to the operand check below.
    let whole_quad = matches!(name, "s_wqm_b64" | "s_wqm_b32");

    // The local data share is shared between the lanes of a wavefront. A model with one lane per
    // invocation would give each lane its own copy.
    let shares_between_lanes = name.starts_with("ds_");

    // `v_div_fmas_f32` reads the condition mask implicitly, so it is listed rather than detected.
    let selects_per_lane = name == CNDMASK || name == "v_div_fmas_f32";

    branches_on_a_mask
        || whole_quad
        || shares_between_lanes
        || selects_per_lane
        || instruction.operands.iter().any(
            |operand| matches!(operand, Operand::Named(name) if lane_mask_name(name).is_some()),
        )
}

/// Reads a sixty-four-bit source as two thirty-two-bit halves.
///
/// A register pair reads both registers. A constant is sign-extended, not repeated: -1 fills both
/// halves, and 1 sets the low half to one and the high half to zero. `s_mov_b64 exec, -1` is the
/// common case, which repeating would also get right.
fn sixty_four_bit_source<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    source: &Operand,
) -> Result<(Id, Id), TranslateError> {
    match source {
        Operand::Scalar(from) => {
            let from = u32::from(*from);
            if from + 2 > SCALAR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a 64-bit source reads past the end of the register file",
                });
            }
            Ok((model.read_scalar(from), model.read_scalar(from + 1)))
        }
        Operand::Integer(value) => {
            let value = i32::try_from(*value).map_err(|_| TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "inline constant does not fit in a register",
            })?;
            let high = if value < 0 { u32::MAX } else { 0 };
            Ok((model.constant(value as u32), model.constant(high)))
        }
        Operand::Named(name) if lane_mask_name(name).is_some() => {
            let name = lane_mask_name(name).unwrap_or(EXEC_LOW_HALF);
            model.read_lane_mask(name)
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a 64-bit source is neither a register pair, an inline constant, ",
                "nor the execution mask"
            ),
        }),
    }
}

/// Whether an instruction writes the scalar condition code.
///
/// A side effect on hidden state is invisible in the encoding and operand layout, so this list is
/// taken from the published instruction set. `the_corpus_agrees_about_hidden_side_effects` checks
/// it against compiled fixtures: a compiler places only non-writing instructions between a
/// condition code write and the branch that reads it.
pub fn writes_condition_code(name: &str) -> bool {
    matches!(
        name,
        // The comparisons exist to write it.
        "s_cmp_eq_i32"
            | "s_cmp_lg_i32"
            | "s_cmp_gt_i32"
            | "s_cmp_ge_i32"
            | "s_cmp_lt_i32"
            | "s_cmp_le_i32"
            | "s_cmpk_eq_i32"
            | "s_cmpk_lg_i32"
            // The arithmetic sets it on signed overflow.
            | "s_add_i32"
            | "s_sub_i32"
            | "s_addk_i32"
            // The logic sets it to whether the result is non-zero, at either width.
            | "s_and_b32"
            | "s_or_b32"
            | "s_xor_b32"
            | "s_and_b64"
            | "s_or_b64"
            | "s_andn2_b64"
    )
}

/// Instructions that read the condition code: only the branches.
pub fn reads_condition_code(name: &str) -> bool {
    matches!(name, "s_cbranch_scc0" | "s_cbranch_scc1")
}

/// The refusal an instruction gets for not being in [`SUPPORTED`].
///
/// A named constant so a test can tell it from the other refusals a supported instruction can get,
/// for operands or for a fidelity with no lane mask.
pub const NO_TRANSLATION: &str = "no translation for this instruction";

/// Whether the translator understands an instruction, by name.
///
/// Callers holding an instruction rather than a name want [`supports_named`], which resolves
/// through the loaded table.
pub fn supports(mnemonic: &str) -> bool {
    SUPPORTED.contains(&mnemonic)
}

/// Instructions understood well enough to say what they are waiting on (D104).
///
/// Distinguishes "nobody has looked at this" from "this waits on something that is not encoding
/// work", so a worklist does not send effort at a refusal that cannot move. Empty is the goal.
pub const BLOCKED: &[(&str, &str)] = &[];

/// Why an instruction is blocked, if it is one this translator recognises. Keyed by name (D139).
pub fn blocked(name: &str) -> Option<&'static str> {
    BLOCKED
        .iter()
        .find(|(blocked, _)| *blocked == name)
        .map(|(_, reason)| *reason)
}

/// The parts of emission that differ between wavefront models.
pub trait Model {
    /// The encoding table, for naming an instruction's family.
    fn encodings(&self) -> &EncodingTable;

    /// The shader's `DX10_CLAMP` mode (`SPI_SHADER_PGM_RSRC1` bit 21): whether an output clamp
    /// turns a NaN into zero. `None`, the default, refuses a clamped instruction rather than
    /// choosing.
    fn dx10_clamp(&self) -> Option<bool> {
        None
    }

    /// How many lanes this model emits code for: one where an invocation is a lane, the full
    /// wavefront where one invocation simulates all of them.
    fn lanes(&self) -> u32;

    /// Whether `lane` may be active here. `false` only where the execution mask is known at
    /// translation time with the lane's bit clear, so a masked-write-only instruction can emit
    /// nothing for that lane.
    ///
    /// The default knows nothing, so every lane may run.
    fn lane_may_run(&self, _lane: u32) -> bool {
        true
    }

    /// Control has reached the start of a guest block, which more than one place may branch to, so
    /// anything inferred from the preceding instructions is dropped.
    fn enter_block(&mut self) {}

    /// A constant of the given value, declared once however often it is used.
    fn constant(&mut self, value: u32) -> Id;

    /// The colour output this module exports to, and its vector type, if it has one.
    ///
    /// [`None`] for every module that is not a fragment shader, which makes an export into a
    /// compute dispatch a refusal (D553).
    fn colour_output(&self) -> Option<(Id, Id)> {
        None
    }

    /// Declares how many vertices and primitives this workgroup will emit.
    ///
    /// [`None`] at any stage but mesh; the caller turns that into a refusal naming the
    /// instruction's offset. Both counts are values rather than literals, because the guest writes
    /// them into `m0` and may compute them.
    fn set_mesh_outputs(&mut self, _vertices: Id, _primitives: Id) -> Option<()> {
        None
    }

    /// Writes one vertex's clip-space position, for the lane that is that vertex.
    fn write_mesh_position(&mut self, _lane: u32, _components: [Id; 4]) -> Option<()> {
        None
    }

    /// Writes one vertex's parameter at a location, for the lane that is that vertex.
    fn write_mesh_parameter(
        &mut self,
        _location: u32,
        _lane: u32,
        _components: [Id; 4],
    ) -> Option<()> {
        None
    }

    /// Writes one primitive's vertex indices, for the lane that is that primitive: as many as
    /// [`Self::mesh_primitive`] carries, one for a point, two for a line, three for a triangle.
    fn write_mesh_indices(&mut self, _lane: u32, _indices: &[Id]) -> Option<()> {
        None
    }

    /// The primitive this module assembles: meaningful only for a mesh module, and the default
    /// ([`crate::wavefront::MeshPrimitive::Triangles`]) for every other.
    fn mesh_primitive(&self) -> crate::wavefront::MeshPrimitive {
        crate::wavefront::MeshPrimitive::default()
    }

    /// The fragment input carrying an attribute, and its vector type, if this module has one.
    ///
    /// [`None`] for a compute module, and for a fragment module not told that the shader
    /// interpolates that attribute: inputs are declared in the header, before any instruction is
    /// seen.
    fn attribute_input(&self, _attribute: u32) -> Option<(Id, Id)> {
        None
    }

    /// The texture this module samples, declaring it on first use.
    ///
    /// `descriptor` and `sampler` are the first scalar register of the two groups a sampling
    /// instruction names: eight registers of image descriptor and four of sampler. No host image
    /// stands behind that guest surface, so a sample reads the texture the pipeline bound, and a
    /// module that names a different descriptor, or reuses registers written since the last sample,
    /// is refused (D690). The sampler is [`None`] for a fetch, which names only an image
    /// descriptor; a fetch and a sample of the same descriptor are one texture.
    ///
    /// The error is a reason rather than a [`TranslateError`], because only the caller knows the
    /// offset.
    fn sampled_image(
        &mut self,
        _descriptor: u32,
        _sampler: Option<u32>,
    ) -> Result<Texture, &'static str> {
        Err(concat!(
            "this model has no texture to sample - a sampled image is bound by a graphics ",
            "pipeline, and a compute dispatch has nowhere to put one"
        ))
    }

    /// The storage image this module writes, declaring it on first use.
    ///
    /// A different binding from [`sampled_image`](Self::sampled_image)'s: one is read through a
    /// sampler and cannot be written, the other is written and has no sampler. The module declares
    /// no format, because the guest's format is in a descriptor that is not decoded (D692).
    fn storage_image(&mut self, _descriptor: u32) -> Result<Stored, &'static str> {
        Err(concat!(
            "this model has no storage image to write - one is bound by a graphics pipeline, ",
            "and a compute dispatch has nowhere to put one"
        ))
    }

    /// The value of a source operand, for one lane.
    fn read_source(
        &mut self,
        instruction: &Instruction,
        operand: &Operand,
        lane: u32,
    ) -> Result<Id, TranslateError>;

    /// Writes one lane of a vector register, honouring the execution mask.
    ///
    /// Where an invocation is a lane there is nothing to mask; where one invocation holds the
    /// wavefront every write is a select against the mask.
    fn write_vector_lane(&mut self, register: u32, lane: u32, value: Id);

    /// Writes a scalar register. Never masked: the scalar unit runs regardless of which lanes are
    /// active.
    fn write_scalar(&mut self, register: u32, value: Id);

    /// Counts an instruction as translated.
    fn count(&mut self);

    /// The module under construction.
    fn builder(&mut self) -> &mut Builder;

    /// The unsigned 32-bit type, which every register is.
    fn u32_type(&self) -> Id;

    /// The 32-bit float type, for arithmetic.
    fn f32_type(&self) -> Id;

    /// The id of the imported `GLSL.std.450` extended instruction set, importing it on first use.
    ///
    /// Lazy so a shader using no extended instruction emits no import; cached because a second
    /// import would be a second set.
    fn glsl_set(&mut self) -> Id;

    /// The 16-bit float type, the intermediate a packed half is read as before the driver's own
    /// conversion widens it to [`f32_type`](Self::f32_type).
    ///
    /// Declared on first use with its capability, so modules without sixteen-bit values ask the
    /// device for no extra features. Only a typed buffer load of a half-format channel reaches it.
    fn f16_type(&mut self) -> Id;

    /// The 16-bit unsigned type a packed half's field is narrowed to before it is read as a half: a
    /// bitcast needs both sides the same width. Declared on first use, like
    /// [`f16_type`](Self::f16_type).
    fn u16_type(&mut self) -> Id;

    /// Reads one word of the local data share.
    ///
    /// Storage shared between a wavefront's lanes; a model with one lane per invocation cannot
    /// represent it, so this and its write are fallible.
    fn read_local(&mut self, word_index: Id) -> Result<Id, TranslateError>;

    /// Writes one word of the local data share, honouring the execution mask.
    fn write_local(&mut self, word_index: Id, value: Id, lane: u32) -> Result<(), TranslateError>;

    /// The guest-memory buffer.
    fn memory_buffer(&self) -> Id;

    /// Pointer type for one word of guest memory.
    fn memory_element_ptr(&self) -> Id;

    /// Reads a scalar register, for an address held in one.
    fn read_scalar(&mut self, register: u32) -> Id;

    /// Reads a sixty-four-bit lane mask, by the name of its low half: [`EXEC_LOW_HALF`] or
    /// [`VCC_LOW_HALF`]. The two masks differ only in which registers they occupy.
    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError>;

    /// Writes a sixty-four-bit lane mask.
    ///
    /// Errors when the model has no such mask, because a model that cannot represent disabled lanes
    /// would produce a plausible wrong answer. The per-lane model refuses; the wavefront model
    /// writes it. This is what keeps [`Fidelity::Lane`](crate::Fidelity::Lane) safe.
    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError>;

    /// Sets bit `lane` of a pair of half-masks from a boolean: the low half for lanes 0-31, the
    /// high half for the rest.
    fn set_lane_bit(&mut self, halves: (Id, Id), lane: u32, condition: Id) -> (Id, Id) {
        let (low, high) = halves;
        let bit = self.constant(1u32 << (lane % 32));
        let zero = self.constant(0);
        let u32_type = self.u32_type();

        let b = self.builder();
        let contribution = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, contribution.0, condition.0, bit.0, zero.0],
        );
        let updated = self.binary(
            op::BITWISE_OR,
            if lane < 32 { low } else { high },
            contribution,
        );
        if lane < 32 {
            (updated, high)
        } else {
            (low, updated)
        }
    }

    /// Writes one word of guest memory, honouring the execution mask.
    ///
    /// Required, because whether an inactive lane's store is suppressed is what distinguishes the
    /// models.
    fn write_memory(&mut self, word_index: Id, value: Id, lane: u32);

    /// Turns a byte address into a word index, inside the window.
    ///
    /// The index is masked, so it is always a legal index: an out-of-range storage-buffer access is
    /// undefined in SPIR-V. Whether the address was in range is a separate question, answered by
    /// [`Model::address_within_window`], which callers act on.
    fn word_index(&mut self, address: Id) -> Id {
        let two = self.constant(2);
        // Relative to where the window starts: an address below the base wraps to a large number,
        // which the check beside this refuses.
        let base = self.constant(self.memory_base());
        // The window is a power of two words, so masking keeps the index legal.
        let limit = self.constant(self.memory_words() - 1);
        let u32_type = self.u32_type();
        let b = self.builder();
        let offset = b.id();
        b.function(op::ISUB, &[u32_type.0, offset.0, address.0, base.0]);
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, offset.0, two.0],
        );
        let index = b.id();
        b.function(op::BITWISE_AND, &[u32_type.0, index.0, shifted.0, limit.0]);
        index
    }

    /// Whether a byte address falls inside the guest-memory window.
    ///
    /// [`Model::word_index`] masks, so an address past the window wraps onto its start. Callers ask
    /// this first: an access outside the window reads zero and writes nothing, as the hardware
    /// answers an out-of-range buffer access (D101).
    fn address_within_window(&mut self, address: Id) -> Id {
        let two = self.constant(2);
        let base = self.constant(self.memory_base());
        let words = self.constant(self.memory_words());
        let u32_type = self.u32_type();
        let bool_type = self.bool_type();
        let b = self.builder();
        // Unsigned arithmetic makes one comparison do the work of two: an address below the base
        // wraps to a large offset and fails the same test as one past the end.
        let offset = b.id();
        b.function(op::ISUB, &[u32_type.0, offset.0, address.0, base.0]);
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, offset.0, two.0],
        );
        let inside = b.id();
        b.function(op::ULESS_THAN, &[bool_type.0, inside.0, shifted.0, words.0]);
        inside
    }

    /// The guest address the memory window starts at.
    ///
    /// Zero for a module whose addresses are the test's own; a guest's buffers are elsewhere, and a
    /// window anchored at zero refuses every access to them. Thirty-two bits, because a flat
    /// access's address pair is read by its low half here.
    fn memory_base(&self) -> u32 {
        0
    }

    /// How many words of guest memory this module addresses. A property of the module, so a test
    /// can widen the window.
    fn memory_words(&self) -> u32;

    /// Reinterprets a register's bits as a float.
    fn as_float(&mut self, value: Id) -> Id {
        let f32_type = self.f32_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::BITCAST, &[f32_type.0, result.0, value.0]);
        result
    }

    /// A comparison of two values, producing a boolean. The opcode decides whether they are
    /// read as floats or as integers, so this serves both.
    fn compare(&mut self, opcode: u16, lhs: Id, rhs: Id) -> Id {
        let bool_type = self.bool_type();
        let b = self.builder();
        let result = b.id();
        b.function(opcode, &[bool_type.0, result.0, lhs.0, rhs.0]);
        result
    }

    /// The boolean type.
    fn bool_type(&mut self) -> Id;

    /// The scalar condition code, as a value.
    ///
    /// One bit of hidden state written by the scalar compares and read by the `scc` branches. A
    /// private variable, because it is written in one arm of the dispatch switch and read in
    /// another. It is wavefront-wide rather than per lane, so both models represent it.
    fn condition_code(&mut self) -> Id;

    /// The `m0` register: a private word, like the condition code.
    ///
    /// A shader that copies it back into a register must read what it wrote. What the hardware does
    /// with the value (address the parameter cache, size an allocation) has no host counterpart, so
    /// the models hold it and do not act on it.
    fn read_m0(&mut self) -> Id;

    /// Writes the `m0` register. Never masked, like every scalar write.
    fn write_m0(&mut self, value: Id);

    /// The program counter: a private variable holding the index of the block to run. Private
    /// because it is written in one arm of the dispatch switch and read in the header.
    fn program_counter(&mut self) -> Id;

    /// How many guest instructions have been translated so far.
    fn instructions(&self) -> usize;

    /// Stores a boolean into the scalar condition code, widened to a word because the code lives in
    /// a private variable and a boolean has no defined size in a storage class.
    fn set_condition_code(&mut self, condition: Id) {
        let one = self.constant(1);
        let zero = self.constant(0);
        let (u32_type, pointer) = (self.u32_type(), self.condition_code());
        let b = self.builder();
        let value = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, value.0, condition.0, one.0, zero.0],
        );
        b.function(op::STORE, &[pointer.0, value.0]);
    }

    /// Whether either of two booleans holds.
    fn either(&mut self, left: Id, right: Id) -> Id {
        let bool_type = self.bool_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::LOGICAL_OR, &[bool_type.0, result.0, left.0, right.0]);
        result
    }

    /// Whether a value is not zero.
    fn is_not_zero(&mut self, value: Id) -> Id {
        let zero = self.constant(0);
        let bool_type = self.bool_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::INOT_EQUAL, &[bool_type.0, result.0, value.0, zero.0]);
        result
    }

    /// Whether bit `lane` of a sixty-four-bit mask, held as two halves, is set. `v_cndmask_b32`
    /// takes an arbitrary register pair rather than a named mask.
    fn lane_bit(&mut self, low: Id, high: Id, lane: u32) -> Id {
        let half = if lane < 32 { low } else { high };
        let shift = self.constant(lane % 32);
        let one = self.constant(1);
        let zero = self.constant(0);
        let (u32_type, bool_type) = (self.u32_type(), self.bool_type());

        let b = self.builder();
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, half.0, shift.0],
        );
        let bit = b.id();
        b.function(op::BITWISE_AND, &[u32_type.0, bit.0, shifted.0, one.0]);
        let result = b.id();
        b.function(op::INOT_EQUAL, &[bool_type.0, result.0, bit.0, zero.0]);
        result
    }

    /// Counts the set bits of a value.
    fn bit_count(&mut self, value: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::BIT_COUNT, &[u32_type.0, result.0, value.0]);
        result
    }

    /// Bitwise complement.
    fn not(&mut self, value: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::NOT, &[u32_type.0, result.0, value.0]);
        result
    }

    /// A binary integer operation on two values.
    fn binary(&mut self, opcode: u16, lhs: Id, rhs: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(opcode, &[u32_type.0, result.0, lhs.0, rhs.0]);
        result
    }

    /// Adds two values.
    fn add(&mut self, lhs: Id, rhs: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let sum = b.id();
        b.function(op::IADD, &[u32_type.0, sum.0, lhs.0, rhs.0]);
        sum
    }

    /// Reads one word of guest memory.
    fn read_memory(&mut self, word_index: Id) -> Id {
        let (u32_type, element_ptr, buffer) = (
            self.u32_type(),
            self.memory_element_ptr(),
            self.memory_buffer(),
        );
        let member = self.constant(0);
        let b = self.builder();
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[element_ptr.0, pointer.0, buffer.0, member.0, word_index.0],
        );
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        value
    }

    /// The address a flat access refers to, for one lane.
    ///
    /// A base of [`FLAT_NO_BASE`] means none and the vector address stands alone. Any other named
    /// operand is refused: an unknown base would put the access at the wrong address.
    fn flat_address(
        &mut self,
        instruction: &Instruction,
        vaddr: &Operand,
        base: &Operand,
        lane: u32,
    ) -> Result<Id, TranslateError> {
        let mut address = self.read_source(instruction, vaddr, lane)?;
        // The byte offset the instruction carries. The reference prints it only when it is not
        // zero, so the operand is absent from an access at offset zero.
        if let Some(Operand::Immediate(byte_offset)) = instruction.operands.get(3)
            && let Ok(byte_offset) = u32::try_from(*byte_offset)
        {
            let constant = self.constant(byte_offset);
            address = self.add(address, constant);
        }
        let offset = address;
        match base {
            Operand::Named(name) if name == FLAT_NO_BASE => Ok(offset),
            Operand::Scalar(register) => {
                let base = self.read_scalar(u32::from(*register));
                Ok(self.add(base, offset))
            }
            _ => Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "flat access has a base this translator does not understand",
            }),
        }
    }

    /// Applies a floating-point operation to two register values.
    ///
    /// A register holds thirty-two untyped bits and the instruction decides how to read them, so
    /// the operands are reinterpreted with `OpBitcast`. `OpConvertUToF` would turn the bits of 1.0
    /// into 1065353216.0.
    fn f32_binary(&mut self, operation: u16, lhs: Id, rhs: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();

        let lhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, lhs_f.0, lhs.0]);
        let rhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, rhs_f.0, rhs.0]);

        let result_f = b.id();
        b.function(operation, &[f32_type.0, result_f.0, lhs_f.0, rhs_f.0]);

        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// Applies a `GLSL.std.450` extended float operation (by instruction number) to two register
    /// values, returning the result's bits.
    ///
    /// Bitcast in and out as in [`f32_binary`](Self::f32_binary); the operation is an `OpExtInst`,
    /// how min, max and the transcendentals are spelled.
    fn f32_ext_binary(&mut self, instruction: u32, lhs: Id, rhs: Id) -> Id {
        let (u32_type, f32_type, set) = (self.u32_type(), self.f32_type(), self.glsl_set());
        let b = self.builder();

        let lhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, lhs_f.0, lhs.0]);
        let rhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, rhs_f.0, rhs.0]);

        let result_f = b.ext_inst(f32_type, set, instruction, &[lhs_f, rhs_f]);

        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// Applies a `GLSL.std.450` extended float operation (by instruction number) to one register
    /// value, returning the result's bits, bitcast in and out as
    /// [`f32_ext_binary`](Self::f32_ext_binary) does.
    fn f32_ext_unary(&mut self, instruction: u32, operand: Id) -> Id {
        let (u32_type, f32_type, set) = (self.u32_type(), self.f32_type(), self.glsl_set());
        let b = self.builder();

        let operand_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, operand_f.0, operand.0]);

        let result_f = b.ext_inst(f32_type, set, instruction, &[operand_f]);

        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// Converts a register's value, read as an unsigned integer, to the equal float, and returns
    /// that float's bits.
    ///
    /// The counterpart to [`f32_binary`](Self::f32_binary)'s bitcast: here the value becomes the
    /// float, so 255 becomes 255.0. Used to lift a packed normalised component and its range into
    /// floats before the division that normalises it.
    fn unsigned_to_float_bits(&mut self, value: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();
        let as_float = b.id();
        b.function(op::CONVERT_U_TO_F, &[f32_type.0, as_float.0, value.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, as_float.0]);
        bits
    }

    /// As [`unsigned_to_float_bits`](Self::unsigned_to_float_bits), reading the value as signed:
    /// `-128` becomes `-128.0`, so a sign-extended packed field keeps its sign.
    fn signed_to_float_bits(&mut self, value: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();
        let as_float = b.id();
        b.function(op::CONVERT_S_TO_F, &[f32_type.0, as_float.0, value.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, as_float.0]);
        bits
    }

    /// Widens a packed float narrower than a half (the 11- and 10-bit channels of formats like
    /// `10_11_11`) to a single-precision float, and returns that float's bits.
    ///
    /// These are not IEEE types, so there is no conversion instruction: each channel is unsigned,
    /// with a five-bit exponent biased by 15 and `width - 5` bits of mantissa. The rules are pinned
    /// by hardware measurements in `tests/execute.rs` (`MEASURED_10_11_11`):
    ///
    /// - Exponent 31 is Inf/NaN, with the mantissa at the top of the single's mantissa field: the
    ///   all-ones word gives `0x7ffe0000` for an 11-bit channel and `0x7ffc0000` for a 10-bit one.
    /// - Exponent 0 is subnormal: the mantissa as an integer times `2^-(14 + mantissa)` gives the
    ///   measured bits exactly, with no leading-zero count needed.
    ///
    /// The caller must have masked the field to its own bits.
    fn narrow_float_to_float_bits(&mut self, field: Id, width: u32) -> Id {
        let mantissa_bits = width - 5;

        let exponent_shift = self.constant(mantissa_bits);
        let exponent = self.binary(op::SHIFT_RIGHT_LOGICAL, field, exponent_shift);
        let mantissa_mask = self.constant((1u32 << mantissa_bits) - 1);
        let mantissa = self.binary(op::BITWISE_AND, field, mantissa_mask);

        // The mantissa sits at the top of the single's 23-bit field in every case, so it is placed
        // once.
        let place = self.constant(23 - mantissa_bits);
        let placed = self.binary(op::SHIFT_LEFT_LOGICAL, mantissa, place);

        // Normal: rebias 15 to 127 and drop the exponent into place.
        let rebias = self.constant(127 - 15);
        let biased = self.binary(op::IADD, exponent, rebias);
        let to_exponent = self.constant(23);
        let exponent_field = self.binary(op::SHIFT_LEFT_LOGICAL, biased, to_exponent);
        let normal = self.binary(op::BITWISE_OR, exponent_field, placed);

        // Infinity or NaN: the single's all-ones exponent over the same placed mantissa.
        let infinity = self.constant(0x7f80_0000);
        let inf_or_nan = self.binary(op::BITWISE_OR, infinity, placed);

        // Subnormal: the mantissa read as an integer, scaled by 2^-(14 + mantissa_bits). The
        // scale is exact in single precision, so this is a rounding-free multiply.
        let scale_bits = self.constant(0x3f80_0000 - ((14 + mantissa_bits) << 23));
        let subnormal = {
            let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
            let b = self.builder();
            let as_float = b.id();
            b.function(op::CONVERT_U_TO_F, &[f32_type.0, as_float.0, mantissa.0]);
            let scale = b.id();
            b.function(op::BITCAST, &[f32_type.0, scale.0, scale_bits.0]);
            let scaled = b.id();
            b.function(op::FMUL, &[f32_type.0, scaled.0, as_float.0, scale.0]);
            let bits = b.id();
            b.function(op::BITCAST, &[u32_type.0, bits.0, scaled.0]);
            bits
        };

        // In this order so exponent 0 wins over the normal arithmetic, which would give 2^-15 times
        // the mantissa rather than 2^-14.
        let all_ones = self.constant(31);
        let is_inf_or_nan = self.compare(op::IEQUAL, exponent, all_ones);
        let finite = self.select(is_inf_or_nan, inf_or_nan, normal);
        let zero = self.constant(0);
        let is_subnormal = self.compare(op::IEQUAL, exponent, zero);
        self.select(is_subnormal, subnormal, finite)
    }

    /// Packs a single-precision float into an unsigned N-bit packed float, the inverse of
    /// [`Self::narrow_float_to_float_bits`], for storing a channel of a format like `10_11_11`.
    ///
    /// The rule is clamp to `[0, max finite]`, then truncate, pinned by `MEASURED_10_11_11_STORE`
    /// in `tests/execute.rs`: `1.009375` stores as mantissa 0 (rounding toward zero), and `100000`
    /// stores as the largest finite value, not infinity. A negative or underflowing value packs to
    /// zero, and infinity or NaN to the largest finite, as the same clamp gives. `value_bits` is
    /// the register's bits.
    fn float_to_narrow_float_bits(&mut self, value_bits: Id, width: u32) -> Id {
        let mantissa_bits = width - 5;

        // Decompose the single into its sign, eight-bit exponent and mantissa.
        let sign_shift = self.constant(31);
        let sign = self.binary(op::SHIFT_RIGHT_LOGICAL, value_bits, sign_shift);
        let exponent = {
            let exponent_shift = self.constant(23);
            let shifted = self.binary(op::SHIFT_RIGHT_LOGICAL, value_bits, exponent_shift);
            let mask = self.constant(0xFF);
            self.binary(op::BITWISE_AND, shifted, mask)
        };
        let mantissa = {
            let mask = self.constant(0x7F_FFFF);
            self.binary(op::BITWISE_AND, value_bits, mask)
        };

        // The truncated normal: drop the low mantissa bits, rebias the exponent 127 -> 15, assemble.
        let drop = self.constant(23 - mantissa_bits);
        let mantissa_field = self.binary(op::SHIFT_RIGHT_LOGICAL, mantissa, drop);
        let rebias = self.constant(127 - 15);
        let exponent15 = self.binary(op::ISUB, exponent, rebias);
        let place = self.constant(mantissa_bits);
        let exponent_field = self.binary(op::SHIFT_LEFT_LOGICAL, exponent15, place);
        let normal = self.binary(op::BITWISE_OR, exponent_field, mantissa_field);

        // The clamp: below the smallest normal (`exp <= 112`) or negative packs to zero; above the
        // largest finite (`exp >= 143`, where Inf and NaN sit) to the largest finite; otherwise the
        // normal.
        let zero = self.constant(0);
        let max_finite = self.constant((30u32 << mantissa_bits) | ((1u32 << mantissa_bits) - 1));
        let over_edge = self.constant(142);
        let is_over = self.compare(op::UGREATER_THAN, exponent, over_edge);
        let clamped_high = self.select(is_over, max_finite, normal);
        let under_edge = self.constant(113);
        let is_under = self.compare(op::ULESS_THAN, exponent, under_edge);
        let non_negative = self.select(is_under, zero, clamped_high);
        let one = self.constant(1);
        let is_negative = self.compare(op::IEQUAL, sign, one);
        self.select(is_negative, zero, non_negative)
    }

    /// `condition ? when_true : when_false`, on `u32` values.
    fn select(&mut self, condition: Id, when_true: Id, when_false: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, result.0, condition.0, when_true.0, when_false.0],
        );
        result
    }

    /// Widens a packed half, a sixteen-bit IEEE float in the low bits of the register, to a
    /// single-precision float, and returns that float's bits.
    ///
    /// The field is narrowed to sixteen bits (`UConvert`), read as a half (`Bitcast`, which needs
    /// equal widths), and widened by `FConvert`, so subnormals, infinities and NaNs take the
    /// device's IEEE path. The caller must have masked the field to its sixteen bits.
    fn half_to_float_bits(&mut self, field: Id) -> Id {
        // Fetched one at a time because each may declare a type and its capability, so each needs
        // the builder to itself.
        let u32_type = self.u32_type();
        let f32_type = self.f32_type();
        let f16_type = self.f16_type();
        let u16_type = self.u16_type();
        let b = self.builder();
        let narrowed = b.id();
        b.function(op::UCONVERT, &[u16_type.0, narrowed.0, field.0]);
        let half = b.id();
        b.function(op::BITCAST, &[f16_type.0, half.0, narrowed.0]);
        let widened = b.id();
        b.function(op::FCONVERT, &[f32_type.0, widened.0, half.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, widened.0]);
        bits
    }

    /// Narrows a register's float to a half and returns the half's sixteen bits, zero-extended.
    ///
    /// The inverse of [`Self::half_to_float_bits`], through `OpFConvert`, so overflow, denormals
    /// and NaNs are the device's. See `pack_halves` for the rounding this does not promise.
    fn float_bits_to_half(&mut self, bits: Id) -> Id {
        let u32_type = self.u32_type();
        let f32_type = self.f32_type();
        let f16_type = self.f16_type();
        let u16_type = self.u16_type();
        let b = self.builder();
        let float = b.id();
        b.function(op::BITCAST, &[f32_type.0, float.0, bits.0]);
        let half = b.id();
        b.function(op::FCONVERT, &[f16_type.0, half.0, float.0]);
        let narrowed = b.id();
        b.function(op::BITCAST, &[u16_type.0, narrowed.0, half.0]);
        let widened = b.id();
        b.function(op::UCONVERT, &[u32_type.0, widened.0, narrowed.0]);
        widened
    }
}

/// A destination and two sources, in the order the specification prints them.
fn three_operands(
    instruction: &Instruction,
) -> Result<(&Operand, &Operand, &Operand), TranslateError> {
    match instruction.operands.as_slice() {
        [destination, first, second] => Ok((destination, first, second)),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "expected exactly three operands",
        }),
    }
}

/// The first three operands, of an instruction that may carry a fourth.
///
/// A flat access decodes a fourth operand only when the reference printed a non-zero offset, so the
/// flat paths take the first three and read the offset themselves.
fn first_three_operands(
    instruction: &Instruction,
) -> Result<(&Operand, &Operand, &Operand), TranslateError> {
    match instruction.operands.as_slice() {
        [first, second, third, ..] => Ok((first, second, third)),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "expected at least three operands",
        }),
    }
}

/// A destination and a source, in that order.
fn two_operands(instruction: &Instruction) -> Result<(&Operand, &Operand), TranslateError> {
    match instruction.operands.as_slice() {
        [destination, source] => Ok((destination, source)),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "expected exactly two operands",
        }),
    }
}

/// What this target calls an instruction, or a refusal saying why it cannot be named.
///
/// Everything downstream dispatches on the name, because an opcode number belongs to one
/// architecture generation (D139).
fn resolve<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<String, TranslateError> {
    let family = instruction
        .encoding
        .and_then(|i| model.encodings().encodings().get(usize::from(i)))
        .map(|e| e.name.clone())
        .ok_or(TranslateError::Unrecognised {
            offset: instruction.offset,
        })?;

    model
        .encodings()
        .mnemonic_for(&family, instruction.opcode)
        .map(str::to_owned)
        // Nothing has observed this opcode on this target, so there is no name to dispatch on, and
        // a bare number is not acted on.
        .ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "this target has no recorded name for that opcode, so there is ",
                "nothing to translate it as"
            ),
        })
}
/// `v_interp_p1_f32` / `v_interp_p2_f32`: an interpolated fragment attribute.
///
/// On the guest `p1` computes `P10 * I + P0` and `p2` adds `P20 * J`, from a parameter cache the
/// hardware fills. A fragment `Input` variable is already the interpolated attribute, so both
/// halves read the whole value (D555). That holds even when the two do not share a destination
/// (`unreached.s` does this), because the answer depends on no register history.
///
/// Assumed: a shader using `p1`'s intermediate for anything but `p2` gets the finished value here
/// instead of a partial sum. Both `vsrc` operands (the barycentric I and J) are ignored; the host
/// interpolates with its own.
fn interpolate<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [
        Operand::Vector(destination),
        _source,
        Operand::Immediate(attribute),
        Operand::Immediate(channel),
    ] = instruction.operands.as_slice()
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "an interpolation decodes to a destination, a source, an attribute and a ",
                "channel; this one did not"
            ),
        });
    };
    let attribute = u32::try_from(*attribute).unwrap_or(u32::MAX);
    let Some((vec4, input)) = model.attribute_input(attribute) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "an interpolation needs a fragment input to read, and this module has none ",
                "for that attribute - translate at the fragment stage"
            ),
        });
    };
    let Ok(channel) = u32::try_from(*channel) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the channel is not a component index",
        });
    };

    let f32_type = model.f32_type();
    let u32_type = model.u32_type();
    let b = model.builder();
    let whole = b.id();
    b.function(op::LOAD, &[vec4.0, whole.0, input.0]);
    let component = b.id();
    b.function(
        op::COMPOSITE_EXTRACT,
        &[f32_type.0, component.0, whole.0, channel],
    );
    // Registers hold bits, not floats, as the export reads them back.
    let bits = b.id();
    b.function(op::BITCAST, &[u32_type.0, bits.0, component.0]);

    // Lane zero only: at the fragment stage one invocation is one pixel, and the export reads lane
    // zero too.
    model.write_vector_lane(u32::from(*destination), 0, bits);
    Ok(())
}

/// The parameter a `v_interp_mov_f32` reads, as the operand field encodes it.
///
/// `orbistoun-gen`'s symbolic-code solver assembles the instruction with each spelling and reads
/// the bits that moved: `p10` is 0, `p20` is 1, `p0` is 2. From the interpolation formulas, `P0` is
/// the value at the primitive's first vertex and the other two are deltas, so `P0` is flat shading
/// with a first-vertex provoking convention, the host default.
const PARAMETER_CONSTANT_TERM: i64 = 2;

/// `v_interp_mov_f32`: a fragment attribute read without interpolating.
///
/// The host has no parameter cache: an input variable is the attribute, and whether reading it
/// interpolates is the variable's `Flat` decoration, set by the declaration pass. What remains here
/// is the same component read the interpolating pair does. `P10` and `P20` are deltas between
/// vertices, which the host never exposes, so they are refused.
fn parameter_move<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let Some(Operand::Immediate(parameter)) = instruction.operands.get(1) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a parameter move decodes to a destination, a parameter, an attribute and 
                     a channel; this one did not",
        });
    };
    if *parameter != PARAMETER_CONSTANT_TERM {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "this moves one of the interpolation deltas, P10 or P20, rather than the 
                     constant term. The host interpolates with its own barycentrics and offers 
                     no way to read the gradient it used",
        });
    }
    interpolate(model, instruction)
}

/// The export targets, as the field encodes them.
///
/// `orbistoun-gen`'s symbolic-code solver assembles an export with each spelling and reads the bits
/// that moved: colour attachments from 0, `pos0` at 12, `param0` at 32, the primitive export at 20.
const EXPORT_POSITION: i64 = 12;
/// The first parameter target; `param<n>` is this plus n.
const EXPORT_PARAMETER: i64 = 32;
/// How many parameter targets there are, so a code past them is not read as one.
const EXPORT_PARAMETERS: i64 = 32;
/// The primitive export: the triangle's vertex indices, packed into one register.
const EXPORT_PRIMITIVE: i64 = 20;

/// Which parameter location an export target names, if it names one.
///
/// Public because the declaration pass needs it before any instruction is translated.
#[must_use]
pub fn export_parameter_location(target: i64) -> Option<u32> {
    let location = target.checked_sub(EXPORT_PARAMETER)?;
    (0..EXPORT_PARAMETERS)
        .contains(&location)
        .then(|| u32::try_from(location).unwrap_or(0))
}

/// The message that asks the geometry engine for room to export: the reference assembler encodes
/// `s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)` as `0xbf900009`.
const MSG_GS_ALLOC_REQ: i64 = 9;

/// Where the vertex and primitive counts sit in `m0`, for the allocation request.
///
/// The low field is the vertex count and the high field the primitive count (D688). Hardware runs
/// of a one-triangle body distinguish the order: `0x1003` and `0x1004` draw, and `0x3001`, one
/// vertex for a body that writes three, never completes. Those runs fit any split between bits 3
/// and 12; twelve reads the SDK's `0x1003` as one primitive of three vertices, and real counts stay
/// far below 2^12.
const MESH_COUNT_BITS: u32 = 12;

/// `s_sendmsg`: a message to a fixed-function unit outside the shader core.
///
/// Only the allocation request is translated, and only at the mesh stage, where the host also
/// declares its output before emitting. Every other message is refused by name.
fn send_message<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let Some(Operand::Immediate(message)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a message's payload did not decode, so which message it is is unknown",
        });
    };
    if *message != MSG_GS_ALLOC_REQ {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "only the geometry allocation request is translated; every other message 
                     asks a fixed-function unit for something this translator has no host 
                     counterpart for, and dropping one silently is not an answer",
        });
    }

    // The counts the guest wrote into `m0`, read back as values, since the host's declaration takes
    // ids and a shader may compute its counts.
    let counts = model.read_m0();
    let width = model.constant((1 << MESH_COUNT_BITS) - 1);
    let vertices = model.binary(op::BITWISE_AND, counts, width);
    let shift = model.constant(MESH_COUNT_BITS);
    let high = model.binary(op::SHIFT_RIGHT_LOGICAL, counts, shift);
    let primitives = model.binary(op::BITWISE_AND, high, width);

    if model.set_mesh_outputs(vertices, primitives).is_none() {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the geometry allocation request declares how much a workgroup will emit, 
                     and only a mesh module has somewhere to declare it - translate at the mesh 
                     stage, which is what a primitive shader is (D688)",
        });
    }
    Ok(())
}

/// `exp pos0` and `exp param<n>`: one vertex's position or one of its parameters.
///
/// The guest narrows its execution mask to the vertex lanes and exports once; each active lane is a
/// vertex. A mesh shader writes an array indexed the same way.
fn mesh_vertex_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    sources: [&Operand; 4],
    location: Option<u32>,
) -> Result<(), TranslateError> {
    for lane in running_lanes(model) {
        let mut components = [Id(0); 4];
        for (slot, source) in sources.into_iter().enumerate() {
            let bits = model.read_source(instruction, source, lane)?;
            components[slot] = model.as_float(bits);
        }
        let written = match location {
            Some(location) => model.write_mesh_parameter(location, lane, components),
            None => model.write_mesh_position(lane, components),
        };
        if written.is_none() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a position or parameter export needs the vertex outputs a mesh module 
                         declares, and this module is not one - translate at the mesh stage, 
                         which is what a primitive shader is (D688)",
            });
        }
    }
    Ok(())
}

/// `exp prim`: the triangle's three vertex indices, packed into one register.
///
/// The SDK's vertex program writes `0x20280600` for vertices 0, 1, 2 with edge flags (oracle record
/// A): `0 | 1 << 10 | 2 << 20` with bits 9, 19 and 29 set, three nine-bit indices each followed by
/// an edge flag. The edge flags select wireframe edges, which the host decides itself, so they are
/// not translated.
fn mesh_primitive_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    packed: &Operand,
) -> Result<(), TranslateError> {
    /// Bits per index; the tenth bit of each field is an edge flag.
    const INDEX_BITS: u32 = 9;
    /// Where each index starts.
    const INDEX_SHIFTS: [u32; 3] = [0, 10, 20];

    // How many indices this primitive uses: three for a triangle, two for a line, one for a point.
    // Assumed for points and lines: the measured packing (oracle record A) is a triangle, and this
    // reads the low `count` nine-bit fields in the same order.
    let count = model.mesh_primitive().indices() as usize;

    for lane in running_lanes(model) {
        let word = model.read_source(instruction, packed, lane)?;
        let mask = model.constant((1 << INDEX_BITS) - 1);
        let mut indices = Vec::with_capacity(count);
        for shift in INDEX_SHIFTS.into_iter().take(count) {
            let amount = model.constant(shift);
            let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word, amount);
            indices.push(model.binary(op::BITWISE_AND, shifted, mask));
        }
        if model.write_mesh_indices(lane, &indices).is_none() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a primitive export needs the index array a mesh module declares, and 
                         this module is not one - translate at the mesh stage (D688)",
            });
        }
    }
    Ok(())
}

/// The only export target translated: `mrt0`, colour attachment zero.
///
/// Which attachment another target selects is register state the guest writes, which is not
/// invented (D104), so other targets are refused by name.
const MRT0: i64 = 0;

/// An export's `COMPR` bit, in its first word: the sources carry packed halves
/// (`aco_assembler.cpp:1001` in the collection's Mesa tree).
const EXPORT_COMPRESSED: u32 = 1 << 10;

/// An export's `EN` field, bits 0-3: which of the four channels it writes
/// (`aco_assembler.cpp:1005`).
const EXPORT_ENABLE_MASK: u32 = 0xf;

/// `exp`: hands four registers to a render target.
///
/// The sources are read for lane zero, reinterpreted as floats (not converted), assembled into a
/// `vec4` and stored to the module's colour output. Refused separately:
///
/// - A module with no colour output: a compute dispatch has nowhere to export to
///   ([`Model::colour_output`]).
/// - Any target but `mrt0`; see [`MRT0`].
/// - A write mask other than all or none of the four channels. The mask and the compressed bit are
///   read from the instruction's first word. A compressed export is unpacked from its two
///   half-packed sources; `done` and `vm` change nothing a single-export translation does.
fn export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [target, sources @ ..] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export decodes to a target and four sources; this one did not",
        });
    };
    let Operand::Immediate(target) = target else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export's target is not an immediate, so which one it names is unknown",
        });
    };
    let [a, b, c, d] = sources else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export takes four sources, and this one decoded a different number",
        });
    };

    // A geometry stage's exports: the vertices' positions and parameters, and the primitive that
    // joins them. One lane is one vertex (D688).
    if *target == EXPORT_POSITION {
        return mesh_vertex_export(model, instruction, [a, b, c, d], None);
    }
    if let Some(location) = export_parameter_location(*target) {
        return mesh_vertex_export(model, instruction, [a, b, c, d], Some(location));
    }
    if *target == EXPORT_PRIMITIVE {
        return mesh_primitive_export(model, instruction, a);
    }

    let Some((vec4, colour)) = model.colour_output() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export needs somewhere to go: a colour attachment at the fragment 
                     stage, or the vertex and primitive outputs at the mesh one. This module 
                     is a compute dispatch and has neither",
        });
    };
    if *target != MRT0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "only mrt0 is translated - which attachment another target selects is 
                     register state a guest writes, and inventing it would render a frame that 
                     looks right and is not (D104)",
        });
    }

    // Every channel, no channel, or refused (`EN`, bits 0-3, `aco_assembler.cpp:1005`). No channel
    // is an export that writes nothing, such as one raised only to end the wave, so nothing is
    // stored. A partial mask keeps channels a whole `vec4` store would overwrite.
    let enabled = instruction.word & EXPORT_ENABLE_MASK;
    if enabled == 0 {
        return Ok(());
    }
    if enabled != EXPORT_ENABLE_MASK {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a colour export that enables fewer than all four channels is not translated",
        });
    }

    let mut components = Vec::with_capacity(4);
    if instruction.word & EXPORT_COMPRESSED != 0 {
        // Compressed: two registers of two halves each, (r, g) in the first source and (b, a) in
        // the second, low half first, as `v_cvt_pkrtz_f16_f32` packs them; the third and fourth
        // sources are unused (`aco_select_ps_epilog.cpp:170-185`).
        let mask = model.constant(0xffff);
        let sixteen = model.constant(16);
        for source in [a, b] {
            let packed = model.read_source(instruction, source, 0)?;
            let low = model.binary(op::BITWISE_AND, packed, mask);
            let high = model.binary(op::SHIFT_RIGHT_LOGICAL, packed, sixteen);
            for half in [low, high] {
                let bits = model.half_to_float_bits(half);
                components.push(model.as_float(bits).0);
            }
        }
    } else {
        for source in [a, b, c, d] {
            // Lane zero: one fragment is one pixel.
            let bits = model.read_source(instruction, source, 0)?;
            components.push(model.as_float(bits).0);
        }
    }

    let builder = model.builder();
    let value = builder.id();
    let mut construct = vec![vec4.0, value.0];
    construct.extend(components);
    builder.function(op::COMPOSITE_CONSTRUCT, &construct);
    builder.function(op::STORE, &[colour.0, value.0]);
    Ok(())
}

/// The lanes a masked-write-only instruction emits code for: every lane but those the model knows
/// are inactive. An instruction that writes a mask loops over every lane instead, because its
/// answer for an inactive lane is a bit that is read.
fn running_lanes<M: Model + ?Sized>(model: &M) -> Vec<u32> {
    (0..model.lanes())
        .filter(|&lane| model.lane_may_run(lane))
        .collect()
}

/// Translates one instruction into whichever model it is handed.
///
/// Refusing is the default. An instruction with no arm here is an error, never a no-op: a shader
/// missing one instruction computes the wrong thing while appearing to work.
pub fn instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let name = resolve(model, instruction)?;

    if !supports(&name) {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: blocked(&name).unwrap_or(NO_TRANSLATION),
        });
    }
    let name = name.as_str();

    match name {
        // Instructions that emit nothing, matched explicitly so "emits nothing" never looks like
        // "nobody handled it":
        //
        // - `s_endpgm` ends the shader; the epilogue is emitted when the module is finished.
        // - `s_waitcnt` waits for memory operations; SPIR-V orders memory through the operations'
        //   own semantics.
        // - `s_nop` inserts wait states for a hardware hazard the host does not have.
        // - `s_inst_prefetch` is an instruction-cache hint with no architectural effect.
        // - `s_clause` groups following instructions for uninterrupted issue; scheduling belongs to
        //   the host driver.
        "s_endpgm" | "s_waitcnt" | "s_clause" | "s_nop" | "s_inst_prefetch" => Ok(()),

        // The export, which is why a fragment stage exists at all (D553).
        "exp" => export(model, instruction),

        // Both halves of the interpolation pair answer the whole value (D555).
        "v_interp_p1_f32_e32" | "v_interp_p2_f32_e32" => interpolate(model, instruction),

        // The same read, from an input the declaration pass decorated `Flat`.
        "v_interp_mov_f32_e32" => parameter_move(model, instruction),

        // The message a primitive shader opens with, and a mesh module's first act.
        "s_sendmsg" => send_message(model, instruction),

        _ => scalar_instruction(model, instruction, name),
    }
}

/// Translates the scalar ALU instructions, passing any other on to the vector family.
fn scalar_instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // The scalar moves, split out because they turn on operand width.
        "s_mov_b32" | "s_mov_b64" => scalar_move(model, instruction, name),

        // s_wqm_b64: whole quad mode. Sets each group of four bits of the result if any of the four
        // in the source is set, so a derivative across a quad has all four pixels live.
        "s_wqm_b64" => whole_quad_mode(model, instruction),
        // s_wqm_b32: the same for a 32-lane wavefront, whose mask is one register (`s_wqm_b32
        // exec_lo, exec_lo` before a texture sample).
        "s_wqm_b32" => whole_quad_mode_32(model, instruction),

        // The 64-bit scalar logic a guest computes masks with: narrow by anding with a comparison
        // result, widen by oring, and take the other branch's lanes with `s_andn2_b64`.
        "s_and_b64" | "s_or_b64" | "s_andn2_b64" => scalar_logic(model, instruction, name),

        // The 32-bit scalar arithmetic and logic, each of which writes the condition code as well
        // as its destination.
        "s_add_i32" | "s_sub_i32" | "s_and_b32" | "s_or_b32" | "s_xor_b32" => {
            scalar_integer(model, instruction, name)
        }

        // The compact scalar form: a destination and a sixteen-bit immediate.
        "s_movk_i32" | "s_cmpk_eq_i32" | "s_cmpk_lg_i32" | "s_addk_i32" | "s_mulk_i32" => {
            scalar_immediate(model, instruction, name)
        }

        // The scalar compares, which write the condition code the `scc` branches read and have no
        // destination operand.
        "s_cmp_eq_i32" | "s_cmp_lg_i32" | "s_cmp_gt_i32" | "s_cmp_ge_i32" | "s_cmp_lt_i32"
        | "s_cmp_le_i32" => scalar_compare(model, instruction, name),

        _ => vector_instruction(model, instruction, name),
    }
}

/// Translates the vector ALU instructions, passing any other on to the memory family.
fn vector_instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // Comparisons, which produce masks: every lane compares, and the answers become one value
        // the shader can and into `exec`.
        "v_cmp_lt_f32_e32" | "v_cmp_eq_f32_e32" | "v_cmp_gt_f32_e32" | "v_cmp_lt_u32_e32" => {
            compare(model, instruction, name)
        }

        // A lane learns its own index by counting the mask bits below itself; no instruction hands
        // it over.
        "v_mbcnt_lo_u32_b32" | "v_mbcnt_hi_u32_b32" => mask_bit_count(model, instruction, name),

        // The long-form vector ALU: the short forms' arithmetic plus per-source negate and absolute
        // flags, in bits neither the operand layout nor the encoding table describes; read
        // separately and refused where not implemented.
        "v_cndmask_b32_e64" | "v_add_f32_e64" | "v_sub_f32_e64" | "v_subrev_f32_e64"
        | "v_mul_f32_e64" | "v_fma_f32" | "v_div_fixup_f32" | "v_div_fmas_f32" => {
            long_form_arithmetic(model, instruction, name)
        }

        // The division pre-scale, and the carry-producing arithmetic that writes a per-lane carry
        // mask as a second destination; 64-bit address arithmetic is built from these.
        "v_div_scale_f32" => division_scale(model, instruction),
        "v_add_co_u32" | "v_sub_co_u32" | "v_add_co_ci_u32_e64" | "v_add_co_ci_u32_e32" => {
            carry_arithmetic(model, instruction, name)
        }

        // v_rcp_f32: a reciprocal, per lane.
        //
        // The guest's is an approximation accurate to roughly one part in a million; this emits an
        // exact division. Bit-exact framebuffer comparisons can differ in the last bit.
        "v_rcp_f32_e32" => {
            let (destination, source) = two_operands(instruction)?;
            let Operand::Vector(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "v_rcp_f32 destination is not a vector register",
                });
            };
            let one = model.constant(ONE_F32);
            for lane in running_lanes(model) {
                let value = model.read_source(instruction, source, lane)?;
                let quotient = model.f32_binary(op::FDIV, one, value);
                model.write_vector_lane(u32::from(*register), lane, quotient);
            }
            model.count();
            Ok(())
        }

        "v_mov_b32_e32" => {
            let (destination, source) = two_operands(instruction)?;
            let Operand::Vector(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "v_mov_b32 destination is not a vector register",
                });
            };
            for lane in running_lanes(model) {
                let value = model.read_source(instruction, source, lane)?;
                model.write_vector_lane(u32::from(*register), lane, value);
            }
            model.count();
            Ok(())
        }

        // The short-form vector ALU: integer address arithmetic and float arithmetic.
        "v_add_f32_e32" | "v_sub_f32_e32" | "v_subrev_f32_e32" | "v_mul_f32_e32"
        | "v_lshlrev_b32_e32" | "v_lshrrev_b32_e32" | "v_add_nc_u32_e32" | "v_fmac_f32_e32" => {
            short_form_arithmetic(model, instruction, name)
        }

        // Float minimum and maximum, emitted as `GLSL.std.450` FMax/FMin because the core opcode
        // set has none.
        "v_max_f32_e32" | "v_min_f32_e32" => float_min_max(model, instruction, name),

        // Two floats packed into one register as halves, which is how a pixel shader prepares a
        // compressed export for an 8_8_8_8 target.
        "v_cvt_pkrtz_f16_f32_e32" => pack_halves(model, instruction),

        // Unary vector float ALU and transcendentals: square root, reciprocal square root, sin,
        // cos, base-2 exp and base-2 log.
        "v_sqrt_f32_e32" | "v_rsq_f32_e32" | "v_sin_f32_e32" | "v_cos_f32_e32"
        | "v_exp_f32_e32" | "v_log_f32_e32" => float_unary(model, instruction, name),

        _ => memory_instruction(model, instruction, name),
    }
}

/// Translates the memory and image instructions, refusing any other.
fn memory_instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // Anything that reaches guest memory.
        "s_load_dword"
        | "s_load_dwordx2"
        | "s_load_dwordx4"
        | "s_load_dwordx8"
        | "global_load_dword"
        | "global_load_dwordx2"
        | "global_load_dwordx4"
        | "global_store_dword"
        | "global_store_dwordx2"
        | "global_store_dwordx4" => memory(model, instruction, name),

        // Untyped buffer access at any width: `buffer_memory` reads the width from the name and
        // refuses one it does not know. `tbuffer_` does not start with `buffer_`, so the families
        // stay distinct. The local data share follows.
        name if name.starts_with("buffer_") => buffer_memory(model, instruction, name),
        name if name.starts_with("tbuffer_") => typed_buffer_memory(model, instruction, name),
        "ds_write_b32" | "ds_read_b32" => local_share(model, instruction, name),

        // A texture sample or fetch; the descriptor rules are D690's.
        "image_sample_lz" | "image_sample" | "image_sample_l" | "image_load" => {
            image_sample(model, instruction, name)
        }
        // A store, the one image instruction that needs its own binding, since nothing writes a
        // sampled image (D692).
        "image_store" => image_store(model, instruction, name),

        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this instruction",
        }),
    }
}

/// Translates `v_mbcnt_lo_u32_b32` / `v_mbcnt_hi_u32_b32`.
///
/// Counts the set bits of the mask strictly below this lane and adds the second source; `lo` looks
/// at bits 0-31 and `hi` at 32-63. Run as a pair (`lo` with an all-ones mask, then `hi` with the
/// first result as addend) it yields the lane index, the only way a shader learns which lane it is.
/// Including the lane's own bit would shift every active lane's index by one.
fn mask_bit_count<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let high_half = name == MBCNT_HI;
    let (destination, mask, addend) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_mbcnt destination is not a vector register",
        });
    };

    for lane in running_lanes(model) {
        let mask_value = model.read_source(instruction, mask, lane)?;
        let base = model.read_source(instruction, addend, lane)?;

        // Which bits of this half sit below this lane: all of it for a lane in the other half, only
        // what precedes it for a lane in this half.
        let below = match (high_half, lane < 32) {
            // `lo` for a lane in the high half, or `hi` for a lane in the low half: nothing or
            // everything.
            (false, false) => u32::MAX,
            (true, true) => 0,
            _ => {
                let within = lane % 32;
                // Shifting a 32-bit value by 32 is undefined, and lane 0 has nothing below it, so
                // the identity is written out.
                if within == 0 {
                    0
                } else {
                    u32::MAX >> (32 - within)
                }
            }
        };

        let below = model.constant(below);
        let selected = model.binary(op::BITWISE_AND, mask_value, below);
        let counted = model.bit_count(selected);
        let total = model.add(base, counted);
        model.write_vector_lane(u32::from(*register), lane, total);
    }
    model.count();
    Ok(())
}

/// `v_mbcnt_hi_u32_b32`, which looks at the mask's high half.
const MBCNT_HI: &str = "v_mbcnt_hi_u32_b32";

/// Translates the short-form vector ALU instructions. The short encoding has no source modifiers.
fn short_form_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a short-form vector destination is not a vector register",
        });
    };
    let register = u32::from(*register);

    for lane in running_lanes(model) {
        let lhs = model.read_source(instruction, first, lane)?;
        let rhs = model.read_source(instruction, second, lane)?;
        let value = match name {
            // Integer: address arithmetic, and shifts whose amount comes first; read in written
            // order, `v_lshlrev` would compute `2 << index` instead of `index << 2`.
            "v_add_nc_u32_e32" => model.binary(op::IADD, lhs, rhs),
            "v_lshlrev_b32_e32" => model.binary(op::SHIFT_LEFT_LOGICAL, rhs, lhs),
            // Logical, not arithmetic: the guest has a separate `v_ashrrev_i32` for the
            // sign-propagating shift.
            "v_lshrrev_b32_e32" => model.binary(op::SHIFT_RIGHT_LOGICAL, rhs, lhs),
            // Float. `v_subrev_f32` reverses its operands, as its name says and its encoding does
            // not. `v_fmac_f32` accumulates into its destination, read through the ordinary source
            // path because the destination names a vector register.
            "v_fmac_f32_e32" => {
                let previous = model.read_source(instruction, destination, lane)?;
                let product = model.f32_binary(op::FMUL, lhs, rhs);
                model.f32_binary(op::FADD, product, previous)
            }
            "v_add_f32_e32" | "v_sub_f32_e32" | "v_subrev_f32_e32" | "v_mul_f32_e32" => {
                let reversed = name == "v_subrev_f32_e32";
                let (lhs, rhs) = if reversed { (rhs, lhs) } else { (lhs, rhs) };
                let operation = match name {
                    "v_add_f32_e32" => op::FADD,
                    "v_sub_f32_e32" | "v_subrev_f32_e32" => op::FSUB,
                    _ => op::FMUL,
                };
                model.f32_binary(operation, lhs, rhs)
            }
            _ => {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "no translation for this short-form vector instruction",
                });
            }
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// GLSL.std.450 instruction number for `FMin` - the smaller of two floats.
const GLSL_FMIN: u32 = 37;
/// GLSL.std.450 instruction number for `FMax` - the larger of two floats.
const GLSL_FMAX: u32 = 40;

/// `v_max_f32` / `v_min_f32`: the per-lane float maximum and minimum, emitted as `GLSL.std.450`
/// extended instructions.
fn float_min_max<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a min/max destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let extended = if name == "v_max_f32_e32" {
        GLSL_FMAX
    } else {
        GLSL_FMIN
    };
    for lane in running_lanes(model) {
        let lhs = model.read_source(instruction, first, lane)?;
        let rhs = model.read_source(instruction, second, lane)?;
        let value = model.f32_ext_binary(extended, lhs, rhs);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// `v_cvt_pkrtz_f16_f32`: two floats, each narrowed to a half, packed low then high.
///
/// The first source lands in bits 0-15 and the second in bits 16-31, the order Mesa's ACO packs
/// `(r, g)` and `(b, a)` for an FP16 colour export (`aco_select_ps_epilog.cpp:170-178`). The
/// instruction rounds toward zero; `OpFConvert` rounds as the device chooses, typically to nearest
/// even. They differ by at most one unit in the last place of a half, so this is not claimed exact.
fn pack_halves<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a half-pack's destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let low = model.read_source(instruction, first, lane)?;
        let high = model.read_source(instruction, second, lane)?;
        let low = model.float_bits_to_half(low);
        let high = model.float_bits_to_half(high);
        let sixteen = model.constant(16);
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, high, sixteen);
        let packed = model.binary(op::BITWISE_OR, low, high);
        model.write_vector_lane(register, lane, packed);
    }
    model.count();
    Ok(())
}

/// GLSL.std.450 instruction number for `Sin`.
const GLSL_SIN: u32 = 13;
/// GLSL.std.450 instruction number for `Cos`.
const GLSL_COS: u32 = 14;
/// GLSL.std.450 instruction number for `Exp2` (2^x).
const GLSL_EXP2: u32 = 29;
/// GLSL.std.450 instruction number for `Log2` (log2(x)).
const GLSL_LOG2: u32 = 30;
/// GLSL.std.450 instruction number for `Sqrt`.
const GLSL_SQRT: u32 = 31;
/// GLSL.std.450 instruction number for `InverseSqrt` (1 / sqrt(x)).
const GLSL_INVERSE_SQRT: u32 = 32;

/// Unary vector float ALU and transcendental operations, emitted as `GLSL.std.450` extended
/// instructions through `OpExtInst`.
fn float_unary<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a unary float destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    // The extended instruction, and whether the source is an angle in revolutions. `v_sin_f32` and
    // `v_cos_f32` compute the sine or cosine of `2*pi * x`; `GLSL.std.450` Sin and Cos take
    // radians, so the argument is scaled by 2*pi first.
    let (extended, revolutions) = match name {
        "v_sqrt_f32_e32" => (GLSL_SQRT, false),
        "v_rsq_f32_e32" => (GLSL_INVERSE_SQRT, false),
        "v_sin_f32_e32" => (GLSL_SIN, true),
        "v_cos_f32_e32" => (GLSL_COS, true),
        "v_exp_f32_e32" => (GLSL_EXP2, false),
        "v_log_f32_e32" => (GLSL_LOG2, false),
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "unknown unary float instruction",
            });
        }
    };
    for lane in running_lanes(model) {
        let operand = model.read_source(instruction, source, lane)?;
        let argument = if revolutions {
            let turn = model.constant(TWO_PI_F32);
            model.f32_binary(op::FMUL, operand, turn)
        } else {
            operand
        };
        let value = model.f32_ext_unary(extended, argument);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The bit pattern of 1.0f.
const ONE_F32: u32 = 0x3F80_0000;

/// The bit pattern of `2*pi` as an f32 (`6.2831855`): the scale from the revolutions
/// `v_sin_f32`/`v_cos_f32` read to the radians `GLSL.std.450` Sin/Cos take.
const TWO_PI_F32: u32 = 0x40C9_0FDB;

/// `v_subrev_f32_e64`, which takes its operands the other way round.
const REVERSE_SUBTRACT: &str = "v_subrev_f32_e64";

/// `v_cndmask_b32`, which picks per lane from a 64-bit mask.
const CNDMASK: &str = "v_cndmask_b32_e64";

/// Where a second, scalar destination sits when an instruction has one: bits 8 to 14 of the first
/// word, where the other sub-encoding keeps per-source absolute-value flags.
const SCALAR_DESTINATION: (u32, u32) = (0, 8);

/// Whether an instruction carries a second, scalar destination.
///
/// The long-form vector ALU has two sub-encodings: one keeps per-source absolute-value flags in
/// bits 8 to 14 of the first word, the other a scalar destination (a carry-out or a pre-scale
/// flag), and nothing in the instruction says which. Misread, `vcc` as a carry destination (code
/// 106) looks like an absolute-value flag and an integer addition loses an operand's sign. The
/// operand solver records where each opcode's operands are, and the sub-encodings differ exactly in
/// whether an operand occupies those bits, so each opcode classifies itself. An instruction whose
/// layout is unknown is refused before translation, so this is only asked about solved opcodes.
fn has_scalar_destination(encodings: &EncodingTable, instruction: &Instruction) -> bool {
    let (word, shift) = SCALAR_DESTINATION;
    instruction
        .encoding
        .and_then(|index| encodings.encodings().get(usize::from(index)))
        .and_then(|family| encodings.operands_for(&family.name, instruction.opcode))
        .is_some_and(|slots| {
            slots
                .iter()
                .any(|slot| slot.word == word && slot.shift == shift)
        })
}

/// Translates the carry-producing vector arithmetic.
///
/// `vdst = src0 op src1`, plus one bit per lane into the scalar destination saying whether that
/// lane carried or borrowed; 64-bit address arithmetic depends on the carry. Needs a model with
/// lanes, because the second destination is a per-lane mask.
fn carry_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    // No absolute-value flags in this sub-encoding; a negate is meaningless on integer arithmetic
    // and is refused.
    let modifiers = Modifiers::read(instruction, true)?;
    for source in 0..3 {
        if modifiers.touches(source) {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "a source modifier on integer carry arithmetic, which is not ",
                    "translated"
                ),
            });
        }
    }

    let operands = &instruction.operands;
    let (Some(Operand::Vector(vector_destination)), Some(scalar_destination)) =
        (operands.first(), operands.get(1))
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "carry arithmetic needs a vector destination and a scalar one",
        });
    };
    // `null` means the carry is not wanted and is dropped, as the guest asked; an ordinary register
    // pair is a place this translator cannot write, handled below.
    let mask_name = match scalar_destination {
        _ if discards(scalar_destination) => None,
        Operand::Named(name) => Some(lane_mask_name(name).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a carry destination this translator does not know",
        })?),
        // An ordinary register pair as the carry destination is legal but needs a general
        // per-register write the lane-mask methods do not offer, so it is refused rather than
        // dropped.
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "carry arithmetic into an ordinary register pair is not ",
                    "translated yet; only the condition mask is"
                ),
            });
        }
    };

    let carry_in = if is_add_with_carry_in(name) {
        Some(operands.get(4).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the carry-in form has no carry-in operand",
        })?)
    } else {
        None
    };
    let (first, second) = (
        operands.get(2).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "carry arithmetic has too few sources",
        })?,
        operands.get(3).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "carry arithmetic has too few sources",
        })?,
    );

    let carry_in = match carry_in {
        Some(operand) => Some(sixty_four_bit_source(model, instruction, operand)?),
        None => None,
    };

    let zero = model.constant(0);
    let mut mask = (zero, zero);
    // Every lane, not only the running ones: the carry is a mask, and its bit for an inactive lane
    // is part of the answer.
    for lane in 0..model.lanes() {
        let left = model.read_source(instruction, first, lane)?;
        let right = model.read_source(instruction, second, lane)?;

        let (value, carried) = match name {
            // Unsigned add carried exactly when the wrapped sum is below an operand, which is exact
            // at the wrap point.
            "v_add_co_u32" => {
                let sum = model.binary(op::IADD, left, right);
                let carried = model.compare(op::ULESS_THAN, sum, left);
                (sum, carried)
            }
            // Unsigned subtract borrows exactly when the left side was smaller.
            "v_sub_co_u32" => {
                let difference = model.binary(op::ISUB, left, right);
                let borrowed = model.compare(op::ULESS_THAN, left, right);
                (difference, borrowed)
            }
            _ if is_add_with_carry_in(name) => {
                let carry_in = carry_in.ok_or(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "the carry-in form lost its carry-in",
                })?;
                add_with_carry(model, carry_in, lane, left, right)
            }
            _ => {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "no translation for this carry instruction",
                });
            }
        };

        model.write_vector_lane(u32::from(*vector_destination), lane, value);
        mask = model.set_lane_bit(mask, lane, carried);
    }

    // The carry, where the instruction asked for one. It falls out of the same arithmetic as the
    // sum, so only the write is conditional.
    if let Some(mask_name) = mask_name {
        model.write_lane_mask(mask_name, mask.0, mask.1)?;
    }
    model.count();
    Ok(())
}

/// One lane of `v_addc_co_u32`: the two sources plus the carry-in bit.
///
/// Two additions and two carry tests: a single test misses the case where the first addition did
/// not carry and adding the carry-in did, which happens when the sources sum to the largest value.
fn add_with_carry<M: Model + ?Sized>(
    model: &mut M,
    carry_in: (Id, Id),
    lane: u32,
    left: Id,
    right: Id,
) -> (Id, Id) {
    let (low, high) = carry_in;
    let bit = model.lane_bit(low, high, lane);
    let one = model.constant(1);
    let zero = model.constant(0);
    let u32_type = model.u32_type();

    let b = model.builder();
    let addend = b.id();
    b.function(op::SELECT, &[u32_type.0, addend.0, bit.0, one.0, zero.0]);

    let partial = model.binary(op::IADD, left, right);
    let first_carry = model.compare(op::ULESS_THAN, partial, left);
    let sum = model.binary(op::IADD, partial, addend);
    let second_carry = model.compare(op::ULESS_THAN, sum, partial);
    (sum, model.either(first_carry, second_carry))
}

/// The add that takes a carry in as well as producing one, in both its forms.
///
/// The long form names its carry registers and the short form leaves them implicit at `vcc`, which
/// the operand table records as implicit operands in the same positions, so one layout serves both.
/// A vertex program forms a 64-bit address with the short form, as in `v_add_co_ci_u32 v19, vcc,
/// s3, v1, vcc`.
fn is_add_with_carry_in(name: &str) -> bool {
    matches!(name, "v_add_co_ci_u32_e64" | "v_add_co_ci_u32_e32")
}

/// Translates the long-form vector ALU instructions.
///
/// One arm for the two-, three- and four-operand shapes: they differ only in how many sources they
/// read, and the modifiers apply the same way.
fn long_form_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read_allowing_clamp(
        instruction,
        has_scalar_destination(model.encodings(), instruction),
    )?;
    let Some(Operand::Vector(register)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a long-form vector destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let sources: Vec<Operand> = instruction.operands[1..].to_vec();

    // The output clamp: applied to a 32-bit float result in a stage whose NaN rule is known, and
    // refused by name otherwise.
    let clamp = if modifiers.clamp {
        if name == CNDMASK || name == "v_div_fmas_f32" || !result_is_f32(name) {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction clamps a result this translation does not clamp - on an ",
                    "integer the flag saturates, and on a select or a scaled multiply-add it is ",
                    "not translated"
                ),
            });
        }
        let Some(nan_to_zero) = model.dx10_clamp() else {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction clamps its result to [0, 1], and what that does to a NaN ",
                    "is the stage's DX10_CLAMP mode, which this translation was not given"
                ),
            });
        };
        Some(nan_to_zero)
    } else {
        None
    };

    if name == CNDMASK {
        return select_per_lane(model, instruction, register, &sources, modifiers);
    }
    if name == "v_div_fmas_f32" {
        return division_fmas(model, instruction, register, &sources, modifiers);
    }

    for lane in running_lanes(model) {
        let mut read = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let raw = model.read_source(instruction, source, lane)?;
            read.push(apply_modifiers(model, raw, modifiers, index));
        }
        let value = combine(model, instruction, name, &read)?;
        let value = match clamp {
            Some(nan_to_zero) => clamp_unit(model, value, nan_to_zero),
            None => value,
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// Whether a long-form vector instruction's result is a 32-bit float, from its name: an `_f32`
/// operation that is not a comparison, and, for a conversion `v_cvt_<to>_<from>`, one converting to
/// `f32`. `v_cvt_i32_f32` produces an integer and `v_cvt_pkrtz_f16_f32` packs halves.
fn result_is_f32(name: &str) -> bool {
    let base = name.strip_suffix("_e64").unwrap_or(name);
    if base.starts_with("v_cmp") || !base.ends_with("_f32") {
        return false;
    }
    base.strip_prefix("v_cvt_")
        .is_none_or(|conversion| conversion.starts_with("f32_"))
}

/// The output clamp on a 32-bit float result: `[0, 1]`, with a NaN either zero (`DX10_CLAMP` set)
/// or passed through (clear), as the stage's `RSRC1` says.
///
/// `FMin`/`FMax` are undefined on a NaN in `GLSL.std.450`, so the NaN takes its own select.
fn clamp_unit<M: Model + ?Sized>(model: &mut M, value: Id, nan_to_zero: bool) -> Id {
    let zero = model.constant(0);
    let one = model.constant(0x3F80_0000);
    let above = model.f32_ext_binary(GLSL_FMAX, value, zero);
    let clamped = model.f32_ext_binary(GLSL_FMIN, above, one);
    let nan = float_is(model, op::IS_NAN, value);
    let for_nan = if nan_to_zero { zero } else { value };
    pick(model, nan, for_nan, clamped)
}

/// Chooses between two 32-bit values on a boolean, without branching.
fn pick<M: Model + ?Sized>(model: &mut M, condition: Id, when_true: Id, when_false: Id) -> Id {
    let u32_type = model.u32_type();
    let b = model.builder();
    let result = b.id();
    b.function(
        op::SELECT,
        &[u32_type.0, result.0, condition.0, when_true.0, when_false.0],
    );
    result
}

/// A unary predicate on a float held as raw bits.
fn float_is<M: Model + ?Sized>(model: &mut M, opcode: u16, value: Id) -> Id {
    let as_float = model.as_float(value);
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(opcode, &[bool_type.0, result.0, as_float.0]);
    result
}

/// Both of two booleans.
fn both<M: Model + ?Sized>(model: &mut M, left: Id, right: Id) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(op::LOGICAL_AND, &[bool_type.0, result.0, left.0, right.0]);
    result
}

/// Translates `v_div_fixup_f32`: the special cases of a division, applied to a quotient.
///
/// The last step of the division sequence: the quotient from reciprocal and Newton-Raphson
/// refinement is replaced wherever zero over zero, infinity over infinity, a signed zero or a NaN
/// applies. The sources are the quotient, the denominator and the numerator, in that order.
///
/// The decision tree follows the instruction-set reference's pseudocode branch for branch. Its
/// `underflow` and `overflow` are IEEE-754 terms under round-to-nearest: below half the smallest
/// subnormal rounds to a signed zero, above the largest finite becomes a signed infinity; the
/// reference's underflow threshold (an exponent difference below -150) is exactly that point.
/// `Quiet(x)` sets the most significant mantissa bit. The tree is built from its default upwards,
/// so the last select applied is the first branch of the pseudocode, which gives the reference's
/// priority.
fn division_fixup<M: Model + ?Sized>(
    model: &mut M,
    quotient: Id,
    denominator: Id,
    numerator: Id,
) -> Id {
    let magnitude = model.constant(0x7FFF_FFFF);
    let sign_bit = model.constant(0x8000_0000);
    let infinity = model.constant(0x7F80_0000);
    // The reference gives this bit pattern for both `0/0` and `inf/inf`: a negative quiet NaN, sign
    // included.
    let indeterminate = model.constant(0xFFC0_0000);
    let quiet_bit = model.constant(0x0040_0000);
    let zero = model.constant(0);

    // sign_out = sign(denominator) ^ sign(numerator)
    let signs = model.binary(op::BITWISE_XOR, denominator, numerator);
    let sign_out = model.binary(op::BITWISE_AND, signs, sign_bit);

    let denominator_magnitude = model.binary(op::BITWISE_AND, denominator, magnitude);
    let numerator_magnitude = model.binary(op::BITWISE_AND, numerator, magnitude);
    let denominator_zero = model.compare(op::IEQUAL, denominator_magnitude, zero);
    let numerator_zero = model.compare(op::IEQUAL, numerator_magnitude, zero);

    let denominator_nan = float_is(model, op::IS_NAN, denominator);
    let numerator_nan = float_is(model, op::IS_NAN, numerator);
    let denominator_infinite = float_is(model, op::IS_INF, denominator);
    let numerator_infinite = float_is(model, op::IS_INF, numerator);

    let signed_zero = sign_out;
    let signed_infinity = model.binary(op::BITWISE_OR, sign_out, infinity);

    // The default: the computed quotient, with the sign the operands imply.
    let magnitude_of_quotient = model.binary(op::BITWISE_AND, quotient, magnitude);
    let mut result = model.binary(op::BITWISE_OR, sign_out, magnitude_of_quotient);

    // exponent(denominator) == 255: an infinity or NaN, already handled above, and translated
    // because the reference states the branch.
    let shift = model.constant(23);
    let exponent_mask = model.constant(0xFF);
    let shifted_denominator = model.binary(op::SHIFT_RIGHT_LOGICAL, denominator, shift);
    let denominator_exponent = model.binary(op::BITWISE_AND, shifted_denominator, exponent_mask);
    let shifted_numerator = model.binary(op::SHIFT_RIGHT_LOGICAL, numerator, shift);
    let numerator_exponent = model.binary(op::BITWISE_AND, shifted_numerator, exponent_mask);
    let all_ones = model.constant(255);
    let denominator_saturated = model.compare(op::IEQUAL, denominator_exponent, all_ones);
    result = pick(model, denominator_saturated, signed_infinity, result);

    // exponent(numerator) - exponent(denominator) < -150: the quotient is below half the smallest
    // subnormal and rounds to a signed zero. Compared as signed on biased exponents; the bias
    // cancels.
    let difference = model.binary(op::ISUB, numerator_exponent, denominator_exponent);
    let threshold = model.constant((-150_i32) as u32);
    let underflows = model.compare(op::SLESS_THAN, difference, threshold);
    result = pick(model, underflows, signed_zero, result);

    // x/inf, or 0/y.
    let vanishes = model.either(denominator_infinite, numerator_zero);
    result = pick(model, vanishes, signed_zero, result);

    // x/0, or inf/y.
    let diverges = model.either(denominator_zero, numerator_infinite);
    result = pick(model, diverges, signed_infinity, result);

    // inf/inf, then 0/0. Both are the indeterminate form.
    let both_infinite = both(model, denominator_infinite, numerator_infinite);
    result = pick(model, both_infinite, indeterminate, result);
    let both_zero = both(model, denominator_zero, numerator_zero);
    result = pick(model, both_zero, indeterminate, result);

    // A NaN operand propagates, quietened. The denominator is tested first so the numerator's
    // select, applied last, wins, in the reference's order.
    let quiet_denominator = model.binary(op::BITWISE_OR, denominator, quiet_bit);
    result = pick(model, denominator_nan, quiet_denominator, result);
    let quiet_numerator = model.binary(op::BITWISE_OR, numerator, quiet_bit);
    pick(model, numerator_nan, quiet_numerator, result)
}

/// Translates `v_div_fmas_f32`: a multiply-add that scales its result when the condition mask says
/// the operands were pre-scaled.
///
/// The middle of the division sequence: `v_div_scale_f32` may scale an operand by a power of two to
/// keep the reciprocal out of the subnormal range and records that in the condition mask; this
/// undoes it per lane by the documented factor of 2^32. The mask is read implicitly, so this needs
/// a model with lanes.
fn division_fmas<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    register: u32,
    sources: &[Operand],
    modifiers: Modifiers,
) -> Result<(), TranslateError> {
    let [first, second, third] = sources else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_div_fmas_f32 does not have three sources",
        });
    };
    let (low, high) = model.read_lane_mask(CONDITION_MASK)?;

    // 2^32, exactly representable, so the scaling is exact.
    let scale = model.constant(0x4F80_0000);

    for lane in running_lanes(model) {
        let a = model.read_source(instruction, first, lane)?;
        let a = apply_modifiers(model, a, modifiers, 0);
        let b = model.read_source(instruction, second, lane)?;
        let b = apply_modifiers(model, b, modifiers, 1);
        let c = model.read_source(instruction, third, lane)?;
        let c = apply_modifiers(model, c, modifiers, 2);

        let product = model.f32_binary(op::FMUL, a, b);
        let sum = model.f32_binary(op::FADD, product, c);
        let scaled = model.f32_binary(op::FMUL, sum, scale);

        let bit = model.lane_bit(low, high, lane);
        let value = pick(model, bit, scaled, sum);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The condition mask, which the division sequence passes its scaling flag through, named by its
/// low half as every 64-bit mask is.
const CONDITION_MASK: &str = VCC_LOW_HALF;

/// The negation of a boolean.
fn negate<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, result.0, value.0]);
    result
}

/// Whether a float's exponent field is all zeroes: a subnormal or a zero.
///
/// The division pre-scale asks whether a computed quotient is subnormal, and a Vulkan device may
/// flush subnormals to zero. Wherever this is used the true result cannot be zero (earlier branches
/// exclude a zero numerator or an infinite operand), so an all-zero exponent means "subnormal" on a
/// flushing device and a preserving one alike. Testing bits rather than comparing against the
/// smallest normal avoids depending on how a comparison treats a flushed operand.
fn exponent_is_zero<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let shift = model.constant(23);
    let mask = model.constant(0xFF);
    let zero = model.constant(0);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, value, shift);
    let exponent = model.binary(op::BITWISE_AND, shifted, mask);
    model.compare(op::IEQUAL, exponent, zero)
}

/// Translates `v_div_scale_f32`: the pre-scale that keeps a division out of the subnormal range.
///
/// The first step of the division sequence. It multiplies one of the numerator and denominator by a
/// power of two so the reciprocal that follows does not land among the subnormals, and records in
/// the condition mask whether it scaled, for `v_div_fmas_f32` to undo. `S0` is the operand to scale
/// and equals either the denominator or the numerator; several branches scale only when `S0` is the
/// operand they concern.
///
/// The decision tree, thresholds and scale factors follow the instruction-set reference's
/// pseudocode; the tree is built from its default upwards so the first branch wins. For the
/// zero-operand case the reference gives no NaN bit pattern, so the canonical quiet NaN is used;
/// `v_div_fixup_f32` replaces any NaN downstream.
fn division_scale<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read(instruction, true)?;
    let operands = &instruction.operands;
    let (register, mask_name) = division_scale_destinations(instruction)?;
    let sources: Vec<Operand> = operands[2..].to_vec();
    let [first, second, third] = sources.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the division pre-scale does not have three sources",
        });
    };

    let constants = ScaleConstants::new(model);

    // Started from zero: the reference opens with `VCC = 0` and then writes every lane.
    let mut halves = (constants.zero, constants.zero);

    // Every lane: this writes a mask as well as values (see `carry_arithmetic`).
    for lane in 0..model.lanes() {
        let (value, set) = division_scale_lane(
            model,
            instruction,
            [first, second, third],
            modifiers,
            &constants,
            lane,
        )?;
        halves = model.set_lane_bit(halves, lane, set);
        model.write_vector_lane(register, lane, value);
    }

    // The flag, where the instruction asked for one. It comes out of the same selects as the value,
    // so only the write is conditional.
    if let Some(mask_name) = mask_name {
        model.write_lane_mask(mask_name, halves.0, halves.1)?;
    }
    model.count();
    Ok(())
}

/// Reads the division pre-scale's vector destination register and the lane mask its flag goes
/// to, `None` when the shader discards the flag.
fn division_scale_destinations(
    instruction: &Instruction,
) -> Result<(u32, Option<&'static str>), TranslateError> {
    let operands = &instruction.operands;
    let (Some(Operand::Vector(destination)), Some(scalar_destination)) =
        (operands.first(), operands.get(1))
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the division pre-scale needs a vector destination and a scalar one",
        });
    };
    let register = u32::from(*destination);
    // `null` means the flag is not wanted: the shader needs only the scaled operand, so the flag is
    // written nowhere.
    let mask_name = match scalar_destination {
        _ if discards(scalar_destination) => None,
        Operand::Named(named) => {
            Some(lane_mask_name(named).ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "the division pre-scale writes a destination this translator does ",
                    "not know as a lane mask"
                ),
            })?)
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "the division pre-scale writes its flag somewhere other than a ",
                    "lane mask, which is not translated"
                ),
            });
        }
    };
    Ok((register, mask_name))
}

/// The constants every lane of the division pre-scale uses, declared once before the lanes.
struct ScaleConstants {
    magnitude: Id,
    zero: Id,
    canonical_nan: Id,
    up: Id,
    down: Id,
    shift: Id,
    exponent_mask: Id,
    ninety_six: Id,
    twenty_three: Id,
    truth: Id,
    falsehood: Id,
}

impl ScaleConstants {
    /// Declares the constants, in field order.
    fn new<M: Model + ?Sized>(model: &mut M) -> Self {
        let magnitude = model.constant(0x7FFF_FFFF);
        let zero = model.constant(0);
        let canonical_nan = model.constant(0x7FC0_0000);
        // Two to the sixty-fourth and its reciprocal, both exact.
        let up = model.constant(0x5F80_0000);
        let down = model.constant(0x1F80_0000);
        let shift = model.constant(23);
        let exponent_mask = model.constant(0xFF);
        let ninety_six = model.constant(96);
        let twenty_three = model.constant(23);
        // The flag is carried as a word through the same selects as the value and turned back into
        // a bit at the end.
        let truth = model.constant(1);
        let falsehood = zero;
        Self {
            magnitude,
            zero,
            canonical_nan,
            up,
            down,
            shift,
            exponent_mask,
            ninety_six,
            twenty_three,
            truth,
            falsehood,
        }
    }
}

/// Computes one lane of the division pre-scale: the scaled value, and whether it scaled.
fn division_scale_lane<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    [first, second, third]: [&Operand; 3],
    modifiers: Modifiers,
    constants: &ScaleConstants,
    lane: u32,
) -> Result<(Id, Id), TranslateError> {
    let ScaleConstants {
        magnitude,
        zero,
        canonical_nan,
        up,
        down,
        shift,
        exponent_mask,
        ninety_six,
        twenty_three,
        truth,
        falsehood,
    } = *constants;
    let scaled_input = model.read_source(instruction, first, lane)?;
    let scaled_input = apply_modifiers(model, scaled_input, modifiers, 0);
    let denominator = model.read_source(instruction, second, lane)?;
    let denominator = apply_modifiers(model, denominator, modifiers, 1);
    let numerator = model.read_source(instruction, third, lane)?;
    let numerator = apply_modifiers(model, numerator, modifiers, 2);

    let denominator_magnitude = model.binary(op::BITWISE_AND, denominator, magnitude);
    let numerator_magnitude = model.binary(op::BITWISE_AND, numerator, magnitude);
    let denominator_zero = model.compare(op::IEQUAL, denominator_magnitude, zero);
    let numerator_zero = model.compare(op::IEQUAL, numerator_magnitude, zero);

    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, denominator, shift);
    let denominator_exponent = model.binary(op::BITWISE_AND, shifted, exponent_mask);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, numerator, shift);
    let numerator_exponent = model.binary(op::BITWISE_AND, shifted, exponent_mask);

    // A subnormal denominator: exponent all zeroes, but not the value zero.
    let denominator_flat = model.compare(op::IEQUAL, denominator_exponent, zero);
    let not_zero = negate(model, denominator_zero);
    let denominator_subnormal = both(model, denominator_flat, not_zero);

    // The quotient is near the top of the range.
    let spread = model.binary(op::ISUB, numerator_exponent, denominator_exponent);
    let very_wide = model.compare(op::SGREATER_THAN_EQUAL, spread, ninety_six);
    let numerator_tiny = model.compare(op::SLESS_THAN_EQUAL, numerator_exponent, twenty_three);

    // The two questions that need an actual division. See `exponent_is_zero`.
    let one = model.constant(0x3F80_0000);
    let reciprocal = model.f32_binary(op::FDIV, one, denominator);
    let reciprocal_subnormal = exponent_is_zero(model, reciprocal);
    let quotient = model.f32_binary(op::FDIV, numerator, denominator);
    let quotient_subnormal = exponent_is_zero(model, quotient);
    let both_subnormal = both(model, reciprocal_subnormal, quotient_subnormal);

    let scaled_is_denominator = model.compare(op::IEQUAL, scaled_input, denominator);
    let scaled_is_numerator = model.compare(op::IEQUAL, scaled_input, numerator);

    let scaled_up = model.f32_binary(op::FMUL, scaled_input, up);
    let scaled_down = model.f32_binary(op::FMUL, scaled_input, down);
    let up_if_denominator = pick(model, scaled_is_denominator, scaled_up, scaled_input);
    let up_if_numerator = pick(model, scaled_is_numerator, scaled_up, scaled_input);

    // Built from the default upwards, so the last applied is the first branch.
    let mut value = scaled_input;
    let mut flag = falsehood;

    value = pick(model, numerator_tiny, scaled_up, value);

    value = pick(model, quotient_subnormal, up_if_numerator, value);
    flag = pick(model, quotient_subnormal, truth, flag);

    value = pick(model, reciprocal_subnormal, scaled_down, value);
    flag = pick(model, reciprocal_subnormal, falsehood, flag);

    value = pick(model, both_subnormal, up_if_denominator, value);
    flag = pick(model, both_subnormal, truth, flag);

    value = pick(model, denominator_subnormal, scaled_up, value);
    flag = pick(model, denominator_subnormal, falsehood, flag);

    value = pick(model, very_wide, up_if_denominator, value);
    flag = pick(model, very_wide, truth, flag);

    let either_zero = model.either(denominator_zero, numerator_zero);
    value = pick(model, either_zero, canonical_nan, value);
    flag = pick(model, either_zero, falsehood, flag);

    let set = model.is_not_zero(flag);
    Ok((value, set))
}

/// Writes the low half of a lane mask, leaving the upper half as it was.
///
/// A 32-lane shader manipulates its mask with 32-bit scalar instructions (`s_mov_b32 exec_lo,
/// ...`). Treated as ordinary scalar writes those would miss the model's mask and leave every lane
/// active. The upper half is read back and rewritten unchanged, because a 64-lane shader may write
/// `exec_lo` alone.
fn write_mask_low<M: Model + ?Sized>(
    model: &mut M,
    mask: &'static str,
    value: Id,
) -> Result<(), TranslateError> {
    let (_, high) = model.read_lane_mask(mask)?;
    model.write_lane_mask(mask, value, high)
}

/// The lane mask an operand names, if it names one.
fn mask_destination(operand: &Operand) -> Option<&'static str> {
    match operand {
        Operand::Named(named) => lane_mask_name(named),
        _ => None,
    }
}

/// Translates `v_cndmask_b32`: per lane, the mask's bit picks which source to copy.
///
/// Its sources are raw bits and are not bitcast, and its third operand is a sixty-four-bit mask, so
/// it needs a model with lanes; the per-lane model refuses it.
fn select_per_lane<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    register: u32,
    sources: &[Operand],
    modifiers: Modifiers,
) -> Result<(), TranslateError> {
    let [when_clear, when_set, mask] = sources else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_cndmask_b32 does not have three sources",
        });
    };
    let (low, high) = sixty_four_bit_source(model, instruction, mask)?;

    for lane in running_lanes(model) {
        let clear = model.read_source(instruction, when_clear, lane)?;
        let set = model.read_source(instruction, when_set, lane)?;
        let clear = apply_modifiers(model, clear, modifiers, 0);
        let set = apply_modifiers(model, set, modifiers, 1);
        let bit = model.lane_bit(low, high, lane);

        let u32_type = model.u32_type();
        let b = model.builder();
        let value = b.id();
        // A set bit picks the second source.
        b.function(op::SELECT, &[u32_type.0, value.0, bit.0, set.0, clear.0]);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// Applies a source's negate and absolute flags: absolute first, then negate, so `-|x|` is
/// expressible, as the encoding means.
///
/// Both act on the bit pattern. Negation has a core opcode; absolute value is clearing the sign
/// bit.
fn apply_modifiers<M: Model + ?Sized>(
    model: &mut M,
    value: Id,
    modifiers: Modifiers,
    source: usize,
) -> Id {
    if !modifiers.touches(source) {
        return value;
    }
    let mut value = value;
    if modifiers.absolute[source] {
        let mask = model.constant(0x7FFF_FFFF);
        value = model.binary(op::BITWISE_AND, value, mask);
    }
    if modifiers.negate[source] {
        let sign = model.constant(0x8000_0000);
        value = model.binary(op::BITWISE_XOR, value, sign);
    }
    value
}

/// Combines the sources a long-form arithmetic instruction read.
fn combine<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
    sources: &[Id],
) -> Result<Id, TranslateError> {
    match (name, sources) {
        (
            "v_add_f32_e64" | "v_sub_f32_e64" | REVERSE_SUBTRACT | "v_mul_f32_e64",
            [first, second],
        ) => {
            // The reverse-subtract takes its operands the other way round, as the short form does.
            let (left, right) = if name == REVERSE_SUBTRACT {
                (*second, *first)
            } else {
                (*first, *second)
            };
            let spirv = match name {
                "v_add_f32_e64" => op::FADD,
                "v_sub_f32_e64" | REVERSE_SUBTRACT => op::FSUB,
                _ => op::FMUL,
            };
            Ok(model.f32_binary(spirv, left, right))
        }
        // Fused multiply-add, a*b+c. SPIR-V's core multiply and add each round, so this is less
        // faithful than a fused operation; `GLSL.std.450` `Fma` is the exact spelling if a
        // framebuffer comparison needs it.
        ("v_fma_f32", [a, b, c]) => {
            let product = model.f32_binary(op::FMUL, *a, *b);
            Ok(model.f32_binary(op::FADD, product, *c))
        }
        ("v_div_fixup_f32", [quotient, denominator, numerator]) => {
            Ok(division_fixup(model, *quotient, *denominator, *numerator))
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a long-form vector instruction has an arity this does not handle",
        }),
    }
}

/// Translates the 32-bit scalar arithmetic and logic.
///
/// Every one writes the condition code: the logical operations to whether the result is non-zero,
/// the arithmetic ones to whether the signed operation overflowed. Dropping it would leave the next
/// branch reading a stale code.
fn scalar_integer<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let mask = mask_destination(destination);
    // A mask destination has no register number, so the register is only resolved when
    // there is one to resolve.
    let register = match mask {
        Some(_) => 0,
        None => scalar_destination(instruction, destination)?,
    };

    let left = model.read_source(instruction, first, 0)?;
    let right = model.read_source(instruction, second, 0)?;

    let (result, condition) = match name {
        "s_add_i32" | "s_sub_i32" => {
            let result = if name == "s_add_i32" {
                model.binary(op::IADD, left, right)
            } else {
                model.binary(op::ISUB, left, right)
            };
            // Signed overflow: the operands agreed in sign and the result does not, tested in bits
            // because no core opcode reports overflow.
            let right = if name == "s_add_i32" {
                right
            } else {
                // Subtraction overflows when the operands *differ* in sign, which is the
                // same test applied to the negated right-hand side.
                let sign = model.constant(0x8000_0000);
                model.binary(op::BITWISE_XOR, right, sign)
            };
            let left_differs = model.binary(op::BITWISE_XOR, left, result);
            let right_differs = model.binary(op::BITWISE_XOR, right, result);
            let both = model.binary(op::BITWISE_AND, left_differs, right_differs);
            let sign = model.constant(0x8000_0000);
            let overflow = model.binary(op::BITWISE_AND, both, sign);
            (result, model.is_not_zero(overflow))
        }
        "s_and_b32" | "s_or_b32" | "s_xor_b32" => {
            let spirv = match name {
                "s_and_b32" => op::BITWISE_AND,
                "s_or_b32" => op::BITWISE_OR,
                _ => op::BITWISE_XOR,
            };
            let result = model.binary(spirv, left, right);
            (result, model.is_not_zero(result))
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "no translation for this scalar integer instruction",
            });
        }
    };

    // A mask destination goes to the mask, not the register file: `s_and_b32 exec_lo, exec_lo, s2`
    // is how a 32-lane shader narrows its execution mask.
    match mask {
        Some(mask) => write_mask_low(model, mask, result)?,
        None => model.write_scalar(register, result),
    }
    model.set_condition_code(condition);
    model.count();
    Ok(())
}

/// Translates the compact scalar form: a destination and a sixteen-bit immediate.
///
/// The immediate is signed; the decoder reports the field as encoded (a disassembler prints -2 as
/// 65534), and sign extension happens here. `s_addk_i32` and `s_mulk_i32` also read their
/// destination, because they accumulate.
fn scalar_immediate<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, immediate) = two_operands(instruction)?;
    let register = scalar_destination(instruction, destination)?;
    let Operand::Immediate(raw) = immediate else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a compact scalar instruction carries no immediate",
        });
    };
    let value = i64::from(sign_extend_16(*raw));
    let constant = model.constant(value as u32);

    match name {
        // s_movk_i32: a move, and the only one here that leaves the code alone.
        "s_movk_i32" => {
            model.write_scalar(register, constant);
        }
        // The compact compares. Signed, like every scalar compare.
        "s_cmpk_eq_i32" | "s_cmpk_lg_i32" => {
            let current = model.read_scalar(register);
            let spirv = if name == "s_cmpk_eq_i32" {
                op::IEQUAL
            } else {
                op::INOT_EQUAL
            };
            let condition = model.compare(spirv, current, constant);
            model.set_condition_code(condition);
        }
        // s_addk_i32 accumulates and sets the code on signed overflow.
        "s_addk_i32" => {
            let current = model.read_scalar(register);
            let result = model.binary(op::IADD, current, constant);
            let left_differs = model.binary(op::BITWISE_XOR, current, result);
            let right_differs = model.binary(op::BITWISE_XOR, constant, result);
            let both = model.binary(op::BITWISE_AND, left_differs, right_differs);
            let sign = model.constant(0x8000_0000);
            let overflow = model.binary(op::BITWISE_AND, both, sign);
            let condition = model.is_not_zero(overflow);
            model.write_scalar(register, result);
            model.set_condition_code(condition);
        }
        // s_mulk_i32 accumulates and leaves the code alone, as the instruction set documents.
        "s_mulk_i32" => {
            let current = model.read_scalar(register);
            let result = model.binary(op::IMUL, current, constant);
            model.write_scalar(register, result);
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "no translation for this compact scalar instruction",
            });
        }
    }
    model.count();
    Ok(())
}

/// Sign-extends a sixteen-bit immediate. The width comes from the instruction's definition, since
/// the operand layout records only the field it observed.
fn sign_extend_16(raw: i64) -> i32 {
    i32::from(i16::try_from(raw & 0xFFFF).unwrap_or(raw as i16))
}

/// The scalar register a destination operand names.
fn scalar_destination(
    instruction: &Instruction,
    destination: &Operand,
) -> Result<u32, TranslateError> {
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scalar destination is not a scalar register",
        });
    };
    Ok(u32::from(*register))
}

/// Translates a scalar comparison into the condition code.
///
/// These compare signed integers; an unsigned comparison agrees on non-negative pairs and reverses
/// the order wherever one operand is negative. The condition code is one bit of hidden state, not
/// an operand.
fn scalar_compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let opcode = op_for_scalar_compare(instruction, name)?;
    let (first, second) = two_operands(instruction)?;

    // Scalar, so once for the wavefront. A scalar instruction has no lanes; a vector source here is
    // refused by the operand check below.
    let left = model.read_source(instruction, first, 0)?;
    let right = model.read_source(instruction, second, 0)?;
    let condition = model.compare(opcode, left, right);

    // Widened to a word, because the code lives in a private variable and a boolean has no defined
    // size in a storage class.
    let one = model.constant(1);
    let zero = model.constant(0);
    let u32_type = model.u32_type();
    let pointer = model.condition_code();
    let b = model.builder();
    let value = b.id();
    b.function(
        op::SELECT,
        &[u32_type.0, value.0, condition.0, one.0, zero.0],
    );
    b.function(op::STORE, &[pointer.0, value.0]);

    model.count();
    Ok(())
}

/// The SPIR-V opcode a scalar comparison maps to. Every one is signed.
fn op_for_scalar_compare(instruction: &Instruction, name: &str) -> Result<u16, TranslateError> {
    match name {
        "s_cmp_eq_i32" => Ok(op::IEQUAL),
        "s_cmp_lg_i32" => Ok(op::INOT_EQUAL),
        "s_cmp_gt_i32" => Ok(op::SGREATER_THAN),
        "s_cmp_ge_i32" => Ok(op::SGREATER_THAN_EQUAL),
        "s_cmp_lt_i32" => Ok(op::SLESS_THAN),
        "s_cmp_le_i32" => Ok(op::SLESS_THAN_EQUAL),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this scalar comparison",
        }),
    }
}

/// Translates a float comparison into a lane mask.
///
/// The 32-bit form's destination is implicit (the condition mask), so it arrives as a named operand
/// carrying no bits. The registers are bitcast to floats first: comparing them as integers orders
/// negative floats backwards.
fn compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (opcode, floats) = op_for_compare(instruction, name)?;
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Named(name) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a comparison's destination is not a named mask",
        });
    };
    let name = lane_mask_name(name).ok_or(TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a comparison's destination is not a lane mask this translator knows",
    })?;

    let zero = model.constant(0);
    let mut halves = (zero, zero);
    // Every lane: the answer is a mask (see `carry_arithmetic`).
    for lane in 0..model.lanes() {
        let left = model.read_source(instruction, first, lane)?;
        let right = model.read_source(instruction, second, lane)?;
        let (left, right) = if floats {
            (model.as_float(left), model.as_float(right))
        } else {
            (left, right)
        };
        let condition = model.compare(opcode, left, right);
        halves = model.set_lane_bit(halves, lane, condition);
    }

    model.write_lane_mask(name, halves.0, halves.1)?;
    model.count();
    Ok(())
}

/// The SPIR-V opcode a comparison maps to, and whether its operands are floats; the two travel
/// together because reading a register as the wrong type is silent.
fn op_for_compare(instruction: &Instruction, name: &str) -> Result<(u16, bool), TranslateError> {
    match name {
        "v_cmp_lt_f32_e32" => Ok((op::FORD_LESS_THAN, true)),
        "v_cmp_eq_f32_e32" => Ok((op::FORD_EQUAL, true)),
        "v_cmp_gt_f32_e32" => Ok((op::FORD_GREATER_THAN, true)),
        "v_cmp_lt_u32_e32" => Ok((op::ULESS_THAN, false)),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this comparison",
        }),
    }
}

/// Translates a sixty-four-bit scalar logical operation.
///
/// Both halves independently, since the operation is bitwise. The destination is often the
/// execution mask: `s_and_b64 exec, exec, s[n:n+1]` enters a conditional region.
fn scalar_logic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let opcode = op_for_logic(instruction, name)?;
    let (destination, first, second) = three_operands(instruction)?;

    let (first_low, first_high) = sixty_four_bit_source(model, instruction, first)?;
    let (second_low, second_high) = sixty_four_bit_source(model, instruction, second)?;

    // `s_andn2_b64` ands with the complement of the second operand; there is no single SPIR-V
    // opcode, so it is a complement then an and.
    let (second_low, second_high) = if name == ANDN2 {
        (model.not(second_low), model.not(second_high))
    } else {
        (second_low, second_high)
    };

    let low = model.binary(opcode, first_low, second_low);
    let high = model.binary(opcode, first_high, second_high);

    // These set the condition code to whether the result is non-zero, as the instruction set
    // documents: `s_and_b64 exec, exec, vcc` then a branch on the code skips a block once no lane
    // survives.
    let either = model.binary(op::BITWISE_OR, low, high);
    let non_zero = model.is_not_zero(either);
    model.set_condition_code(non_zero);

    if let Operand::Named(name) = destination
        && let Some(mask) = lane_mask_name(name)
    {
        model.write_lane_mask(mask, low, high)?;
        model.count();
        return Ok(());
    }

    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a 64-bit scalar logical destination is neither a register pair nor ",
                "the execution mask"
            ),
        });
    };
    let register = u32::from(*register);
    if register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a 64-bit scalar logical result runs past the end of the register file",
        });
    }
    model.write_scalar(register, low);
    model.write_scalar(register + 1, high);
    model.count();
    Ok(())
}

/// `s_andn2_b64`, whose second operand is complemented.
const ANDN2: &str = "s_andn2_b64";

/// The SPIR-V opcode a 64-bit scalar logical instruction maps to.
fn op_for_logic(instruction: &Instruction, name: &str) -> Result<u16, TranslateError> {
    match name {
        "s_and_b64" | ANDN2 => Ok(op::BITWISE_AND),
        "s_or_b64" => Ok(op::BITWISE_OR),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this scalar logical instruction",
        }),
    }
}

/// Translates a scalar move, both widths together because `s_mov_b64` is not two `s_mov_b32`s.
fn scalar_move<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // s_mov_b64: two consecutive registers from two consecutive sources.
        //
        // A constant source is extended to sixty-four bits, not repeated: `s_mov_b64 s[0:1], -1`
        // sets both halves to all ones and `s_mov_b64 s[0:1], 1` sets s0 to one and s1 to zero.
        // This is how the execution mask is set.
        "s_mov_b64" => {
            let (destination, source) = two_operands(instruction)?;
            let (low, high) = sixty_four_bit_source(model, instruction, source)?;

            // `s_mov_b64 exec, ...` turns lanes off; the destination decodes as the mask's low
            // half, as a sixty-four-bit operand names its pair.
            if let Operand::Named(register) = destination
                && let Some(mask) = lane_mask_name(register)
            {
                model.write_lane_mask(mask, low, high)?;
                model.count();
                return Ok(());
            }

            let Operand::Scalar(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "s_mov_b64 destination is neither a scalar register nor the ",
                        "execution mask"
                    ),
                });
            };
            let register = u32::from(*register);
            if register + 2 > SCALAR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "s_mov_b64 runs past the end of the register file",
                });
            }

            model.write_scalar(register, low);
            model.write_scalar(register + 1, high);
            model.count();
            Ok(())
        }

        "s_mov_b32" => {
            let (destination, source) = two_operands(instruction)?;
            let value = model.read_source(instruction, source, 0)?;

            // `s_mov_b32 exec_lo, ...` sets a 32-lane shader's execution mask, as `s_mov_b64 exec,
            // ...` does for a 64-lane one.
            if let Some(mask) = mask_destination(destination) {
                write_mask_low(model, mask, value)?;
                model.count();
                return Ok(());
            }

            // `s_mov_b32 m0, sN` is how a pixel shader hands the interpolator its primitive mask
            // (orbistoun-gpu `tests/oracle_gl_cube.rs`).
            if let Operand::Named(name) = destination
                && name == M0
            {
                model.write_m0(value);
                model.count();
                return Ok(());
            }

            let Operand::Scalar(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "s_mov_b32 destination is neither a scalar register nor a ",
                        "lane mask"
                    ),
                });
            };
            model.write_scalar(u32::from(*register), value);
            model.count();
            Ok(())
        }

        // v_mov_b32: per lane, masked.
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this scalar move",
        }),
    }
}

/// Translates a local-data-share access.
///
/// The address is a byte address in a vector register plus a byte offset in the instruction, which
/// the reference omits when it is zero. Reads are unmasked and writes masked, as for guest memory:
/// an inactive lane must not write what an active one will read.
fn local_share<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (first, second, offset) = three_operands(instruction)?;
    let Operand::Immediate(offset) = offset else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a local-data-share access carries no byte offset - the operand ",
                "layout for this opcode is missing one"
            ),
        });
    };
    let offset = u32::try_from(*offset).map_err(|_| TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a negative local-data-share offset",
    })?;

    let reading = name == DS_READ;
    // A read names its destination then its address; a write names its address then its data. The
    // layouts differ.
    let (destination, address) = if reading {
        (first, second)
    } else {
        (second, first)
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a local-data-share operand is not a vector register",
        });
    };
    let register = u32::from(*register);

    for lane in running_lanes(model) {
        let base = model.read_source(instruction, address, lane)?;
        let byte_offset = model.constant(offset);
        let byte_address = model.add(base, byte_offset);
        let index = model.word_index(byte_address);

        if reading {
            let value = model.read_local(index)?;
            model.write_vector_lane(register, lane, value);
        } else {
            let value = model.read_source(instruction, destination, lane)?;
            model.write_local(index, value, lane)?;
        }
    }
    model.count();
    Ok(())
}

/// `ds_read_b32`.
const DS_READ: &str = "ds_read_b32";

/// How many consecutive words a multi-word access carries, from its name.
///
/// `s_load_dword` is one, `s_load_dwordx2` two, and so on; the flat accesses spell it the same way.
/// The suffix is the instruction's own statement of its width, where opcode adjacency is not. An
/// unparseable suffix answers one, which under-reads visibly rather than writing registers the
/// instruction never named.
fn access_words(name: &str) -> u32 {
    match name.rsplit_once("dwordx") {
        Some((_, count)) => count.parse().unwrap_or(1),
        None => 1,
    }
}

/// A buffer resource constant, read out of four consecutive scalar registers.
///
/// Holds the fields addressing needs; channel selects and data format describe a conversion the
/// untyped accesses do not do.
struct BufferResource {
    /// Byte address of the buffer. The reference gives 48 bits; see [`buffer_address`].
    base: Id,
    /// Bytes per record, 0 to 16383. Zero means a raw buffer.
    stride: Id,
    /// In units of stride when there is one, otherwise in bytes.
    records: Id,
    /// Set when the descriptor asks for addressing this translator does not do.
    unsupported: Id,
}

/// Reads a buffer resource constant from the register file.
///
/// From the instruction-set reference's descriptor table: base address in bits 47:0, stride in
/// 61:48, record count in 95:64, swizzle enable at 63, add-thread-id at 119 and the out-of-bounds
/// mode in 125:124, landing across four registers as the shifts below. The descriptor lives in
/// registers, so addressing is emitted as run-time arithmetic.
fn read_buffer_resource<M: Model + ?Sized>(model: &mut M, first: u32) -> BufferResource {
    let base = model.read_scalar(first);
    let second = model.read_scalar(first + 1);
    let records = model.read_scalar(first + 2);
    let flags = model.read_scalar(first + 3);

    let sixteen = model.constant(16);
    let stride_mask = model.constant(0x3FFF);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, second, sixteen);
    let stride = model.binary(op::BITWISE_AND, shifted, stride_mask);

    // Swizzled addressing and add-thread-id change where an access lands, and neither is modelled.
    // A translated shader cannot refuse at run time, so such an access is forced out of bounds,
    // which reads zero and drops writes (D147).
    let swizzle_bit = model.constant(1 << 31);
    let swizzled = model.binary(op::BITWISE_AND, second, swizzle_bit);
    let add_tid_bit = model.constant(1 << 23);
    let add_tid = model.binary(op::BITWISE_AND, flags, add_tid_bit);
    let unsupported = model.binary(op::BITWISE_OR, swizzled, add_tid);

    BufferResource {
        base,
        stride,
        records,
        unsupported,
    }
}

/// The byte address a buffer access reads or writes.
///
/// The reference gives it as
///
/// ```text ADDR = Base + baseOffset + Inst_offset + Voffset + Stride * (Vindex + TID) ```
///
/// where `baseOffset` is the scalar offset operand, `Inst_offset` the literal in the instruction,
/// `Voffset` a vector register present when the instruction sets `offen`, and `Vindex` one present
/// when it sets `idxen`. The thread-id term is excluded, with the descriptors that ask for it.
///
/// Only the low thirty-two bits are computed: guest memory here is a window indexed from its base,
/// as for the flat accesses.
fn buffer_address<M: Model + ?Sized>(
    model: &mut M,
    resource: &BufferResource,
    scalar_offset: Id,
    instruction_offset: Id,
    voffset: Option<Id>,
    vindex: Option<Id>,
) -> Id {
    let mut address = model.add(resource.base, scalar_offset);
    address = model.add(address, instruction_offset);
    if let Some(voffset) = voffset {
        address = model.add(address, voffset);
    }
    if let Some(vindex) = vindex {
        let scaled = model.binary(op::IMUL, resource.stride, vindex);
        address = model.add(address, scaled);
    }
    address
}

/// Whether a buffer access falls outside the buffer.
///
/// The reference defines four modes, selected by two descriptor bits known only at run time, so all
/// four are evaluated: mode 0 checks index >= records or offset >= stride (structured buffers);
/// mode 1 checks index >= records (raw buffers); mode 2 checks records == 0; mode 3 checks offset +
/// payload > records (raw, unswizzled). Mode 3's payload is read as bytes: the reference calls it
/// dwords, but every other term is a byte count, and bytes give the ordinary range check.
fn buffer_out_of_bounds<M: Model + ?Sized>(
    model: &mut M,
    resource: &BufferResource,
    flags: Id,
    offset: Id,
    index: Id,
    payload_bytes: u32,
) -> Id {
    let zero = model.constant(0);
    let two = model.constant(2);
    // The selector is two bits wide.
    let three = model.constant(3);
    let twenty_four = model.constant(24);
    let payload = model.constant(payload_bytes);

    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, flags, twenty_four);
    let mode = model.binary(op::BITWISE_AND, shifted, three);

    let index_past = model.compare(op::UGREATER_THAN_EQUAL, index, resource.records);
    let offset_past_stride = model.compare(op::UGREATER_THAN_EQUAL, offset, resource.stride);
    let no_records = model.compare(op::IEQUAL, resource.records, zero);
    let reach = model.add(offset, payload);
    let reach_past = model.compare(op::UGREATER_THAN, reach, resource.records);

    let structured = model.either(index_past, offset_past_stride);
    let one = model.constant(1);

    // Mode three is the default and the others override it, so the checks apply in descending order
    // and the last select wins.
    let mut out = reach_past;
    let is_two = model.compare(op::IEQUAL, mode, two);
    out = select_bool(model, is_two, no_records, out);
    let is_one = model.compare(op::IEQUAL, mode, one);
    out = select_bool(model, is_one, index_past, out);
    let is_zero = model.compare(op::IEQUAL, mode, zero);
    out = select_bool(model, is_zero, structured, out);

    // A descriptor asking for addressing this does not do is treated as out of bounds.
    let refused = model.is_not_zero(resource.unsupported);
    model.either(out, refused)
}

/// Chooses between two booleans.
fn select_bool<M: Model + ?Sized>(
    model: &mut M,
    condition: Id,
    when_true: Id,
    when_false: Id,
) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(
        op::SELECT,
        &[
            bool_type.0,
            result.0,
            condition.0,
            when_true.0,
            when_false.0,
        ],
    );
    result
}

/// The typed-buffer format table, parsed once.
///
/// A table for the reasons in [`orbistoun_shader::formats`]; behind a lock because parsing it per
/// instruction would dominate translation.
static BUFFER_FORMATS: std::sync::OnceLock<orbistoun_shader::FormatTable> =
    std::sync::OnceLock::new();

/// How many components a typed access moves, from its name.
///
/// The channel letters (`_x` is one, `_xyzw` four) count registers, which may differ from the
/// format's component count; this reads only the letters.
fn typed_channels(name: &str) -> Option<u32> {
    let suffix = name.rsplit_once("_format_")?.1;
    let count = suffix.len();
    // Contiguous from `x`, so `xz` is not an access this understands.
    ("xyzw".starts_with(suffix) && count >= 1).then_some(count as u32)
}

/// A typed buffer access: the descriptor and addressing of an untyped one, with a format saying how
/// to read what was fetched.
///
/// Formats whose components are all thirty-two bits move whole words through [`buffer_access`].
/// Narrower components are extracted and converted by [`packed_buffer_memory`]. Anything neither
/// handles is refused by name, never approximated. The component count must match the channel
/// count: the hardware's rule when they differ is unmeasured.
fn typed_buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let loading = name.starts_with("tbuffer_load");
    let Some(channels) = typed_channels(name) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer access does not name its channels",
        });
    };

    let formats =
        BUFFER_FORMATS.get_or_init(|| orbistoun_shader::FormatTable::builtin().unwrap_or_default());
    let code = orbistoun_shader::FormatTable::field(instruction.word);
    let Some(format) = formats.get(code) else {
        // The explicitly invalid code or a reserved one; both mean the shader is wrong.
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer access names a format code with no meaning",
        });
    };
    if format.components() != channels as usize {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer access whose format and channel count disagree",
        });
    }

    // A plain-word format moves whole dwords through the untyped path; a narrow format packs
    // components inside words and goes to `packed_buffer_memory`.
    if format.is_plain_words() {
        buffer_access(model, instruction, loading, channels)
    } else {
        packed_buffer_memory(model, instruction, format, loading)
    }
}

/// One narrow integer component pulled out of a packed word, sign-extended when `signed`.
///
/// The component occupies bits `bit..bit + width` of `packed`, with `width < 32`. Unsigned: shift
/// down and mask. Signed: shift its high bit up to bit 31, then shift arithmetically right. Both
/// are exact.
fn packed_integer_component<M: Model + ?Sized>(
    model: &mut M,
    packed: Id,
    bit: u32,
    width: u32,
    signed: bool,
) -> Id {
    if signed {
        let left = 32 - bit - width;
        let up = if left == 0 {
            packed
        } else {
            let amount = model.constant(left);
            model.binary(op::SHIFT_LEFT_LOGICAL, packed, amount)
        };
        let right = model.constant(32 - width);
        model.binary(op::SHIFT_RIGHT_ARITHMETIC, up, right)
    } else {
        let shifted = if bit == 0 {
            packed
        } else {
            let amount = model.constant(bit);
            model.binary(op::SHIFT_RIGHT_LOGICAL, packed, amount)
        };
        let mask = model.constant((1u32 << width) - 1);
        model.binary(op::BITWISE_AND, shifted, mask)
    }
}

/// Extracts one component from the packed word and produces the value the register holds: the
/// integer for the integer kinds, or a float's bits for the converting ones.
///
/// The kinds handled are exactly those [`packed_buffer_memory`] admits, so the `unreachable!`
/// guards a case that cannot arise.
fn packed_component<M: Model + ?Sized>(
    model: &mut M,
    packed: Id,
    bit: u32,
    width: u32,
    kind: orbistoun_shader::ComponentKind,
) -> Id {
    use orbistoun_shader::ComponentKind;
    match kind {
        ComponentKind::Uint => packed_integer_component(model, packed, bit, width, false),
        ComponentKind::Sint => packed_integer_component(model, packed, bit, width, true),
        // A UNORM component is its unsigned field over the field's maximum, in 0.0..=1.0. The
        // maximum is converted from the integer domain the same way the field is.
        ComponentKind::Unorm => {
            let field = packed_integer_component(model, packed, bit, width, false);
            let field_f = model.unsigned_to_float_bits(field);
            let maximum = model.constant((1u32 << width) - 1);
            let maximum_f = model.unsigned_to_float_bits(maximum);
            model.f32_binary(op::FDIV, field_f, maximum_f)
        }
        // A SNORM component is its signed field over 2^(width-1) - 1, in -1.0..=1.0. The
        // most-negative code lands just past -1.0 (-128/127 for a byte) and the reference pins it
        // at -1.0, so the low end is clamped by selecting between the two bit patterns.
        ComponentKind::Snorm => {
            let field = packed_integer_component(model, packed, bit, width, true);
            let field_f = model.signed_to_float_bits(field);
            let maximum = model.constant((1u32 << (width - 1)) - 1);
            let maximum_f = model.unsigned_to_float_bits(maximum);
            let normalized = model.f32_binary(op::FDIV, field_f, maximum_f);
            let minus_one = model.constant((-1.0f32).to_bits());
            let value_f = model.as_float(normalized);
            let low_f = model.as_float(minus_one);
            let below = model.compare(op::FORD_LESS_THAN, value_f, low_f);
            pick(model, below, minus_one, normalized)
        }
        // USCALED and SSCALED are UNORM and SNORM without the division: the field's value as a
        // float, so an eight-bit 255 is 255.0. No range, so no clamp.
        ComponentKind::Uscaled => {
            let field = packed_integer_component(model, packed, bit, width, false);
            model.unsigned_to_float_bits(field)
        }
        ComponentKind::Sscaled => {
            let field = packed_integer_component(model, packed, bit, width, true);
            model.signed_to_float_bits(field)
        }
        // A float, by one of two routes: at width 16 an IEEE half widened by `FConvert`; at 11 and
        // 10 bits (formats like `10_11_11`) a sign-less packed float built by hand.
        ComponentKind::Float => {
            let field = packed_integer_component(model, packed, bit, width, false);
            if width == 16 {
                model.half_to_float_bits(field)
            } else {
                model.narrow_float_to_float_bits(field, width)
            }
        }
        // sRGB is a transfer curve rather than a scale; `packed_buffer_memory` refuses it before a
        // lane is emitted.
        ComponentKind::Srgb => {
            unreachable!(
                "packed_buffer_memory admits only the integer, normalised, scaled and half kinds"
            )
        }
    }
}

/// Whether a packed format is one [`packed_buffer_memory`] can translate, and its width in bits.
///
/// Translated kinds, all within whole words: `UINT`/`SINT` (exact bit fields), `UNORM`/`SNORM`,
/// `USCALED`/`SSCALED`, and `FLOAT` at widths 16, 11 and 10. `SRGB`, other float widths and
/// components spanning words are refused with a detail naming which. Every admitted kind has
/// arithmetic in [`packed_component`], whose `unreachable!` catches a kind that bypassed this.
fn packed_format_admitted(
    instruction: &Instruction,
    format: &orbistoun_shader::BufferFormat,
    loading: bool,
) -> Result<u32, TranslateError> {
    use orbistoun_shader::ComponentKind;

    let total_bits: u32 = format.widths.iter().sum();
    // Floats are admitted at 16 bits (IEEE halves) and at 11 and 10 bits (the sign-less packed
    // floats `narrow_float_to_float_bits` widens, pinned by `MEASURED_10_11_11` in
    // `tests/execute.rs`). Other float widths are refused: their bias and subnormal rules are
    // unmeasured.
    let known_float_widths = format.kind == ComponentKind::Float
        && format
            .widths
            .iter()
            .all(|&width| matches!(width, 16 | 11 | 10));
    let convertible = matches!(
        format.kind,
        ComponentKind::Uint
            | ComponentKind::Sint
            | ComponentKind::Unorm
            | ComponentKind::Snorm
            | ComponentKind::Uscaled
            | ComponentKind::Sscaled
    ) || known_float_widths;

    // An element is a little-endian byte sequence with component zero first, so the component at
    // bit `b` is in word `b / 32` at bit `b % 32`. A component spanning a word boundary would need
    // a rule nothing has measured; no format in the table has one, and it is refused.
    let mut bit = 0;
    let straddles = format.widths.iter().any(|&width| {
        let crosses = bit / 32 != (bit + width - 1) / 32;
        bit += width;
        crosses
    });

    // A store is admitted only for the packed 10/11-bit floats measured for it
    // (`MEASURED_10_11_11_STORE` in `tests/execute.rs`), which fill exactly one word and pack by
    // clamp-and-truncate (`Model::float_to_narrow_float_bits`). Other store conversions have
    // unmeasured rounding and saturation rules.
    let packed_float_store = format.kind == ComponentKind::Float
        && total_bits == 32
        && format.widths.iter().all(|&width| matches!(width, 11 | 10));
    let direction_admitted = loading || packed_float_store;

    if !direction_admitted || total_bits > ELEMENT_BITS || straddles || !convertible {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a packed typed buffer format needing component conversion ",
                "(integer, normalised, scaled and half-float loads, and the packed ",
                "10/11-bit float stores, whose components do not span a word are translated so far)"
            ),
        });
    }
    Ok(total_bits)
}

/// The widest element a packed typed access can move: four whole-word components, the four
/// registers a `_xyzw` access names.
const ELEMENT_BITS: u32 = 128;

fn packed_buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    format: &orbistoun_shader::BufferFormat,
    loading: bool,
) -> Result<(), TranslateError> {
    let total_bits = packed_format_admitted(instruction, format, loading)?;

    // Operands and addressing modifiers are read as the untyped path reads them; only the
    // per-component work differs.
    let [data, vaddr, resource_operand, soffset] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer access does not have four operands",
        });
    };
    let Operand::Vector(register) = data else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer access names something other than a vector register for its data",
        });
    };
    let register = u32::from(*register);
    let Operand::Scalar(resource_base) = resource_operand else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant is not in scalar registers",
        });
    };
    let resource_base = u32::from(*resource_base);
    if resource_base + 4 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant runs past the end of the register file",
        });
    }

    let word = instruction.word;
    let literal_offset = word & 0xFFF;
    let offen = word & (1 << 12) != 0;
    let idxen = word & (1 << 13) != 0;

    let resource = read_buffer_resource(model, resource_base);
    let flags = model.read_scalar(resource_base + 3);
    let instruction_offset = model.constant(literal_offset);

    // The element is bounds-checked once, whole, then read a word at a time; a per-word check would
    // let half an element be read at the end of a buffer.
    let element_bytes = total_bits.div_ceil(8);
    let element_words = total_bits.div_ceil(32);

    for lane in running_lanes(model) {
        let scalar_offset = model.read_source(instruction, soffset, lane)?;
        let (vindex, voffset) = match (idxen, offen) {
            (false, false) => (None, None),
            (true, false) => (Some(model.read_source(instruction, vaddr, lane)?), None),
            (false, true) => (None, Some(model.read_source(instruction, vaddr, lane)?)),
            (true, true) => {
                let Operand::Vector(first) = vaddr else {
                    return Err(TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: concat!(
                            "a buffer access with both address modifiers does not ",
                            "name a vector register pair"
                        ),
                    });
                };
                let base = *first;
                let index = model.read_source(instruction, &Operand::Vector(base), lane)?;
                let offset = model.read_source(instruction, &Operand::Vector(base + 1), lane)?;
                (Some(index), Some(offset))
            }
        };

        let zero = model.constant(0);
        let offset_term = voffset.unwrap_or(zero);
        let offset = model.add(instruction_offset, offset_term);
        let index = vindex.unwrap_or(zero);

        let base = buffer_address(
            model,
            &resource,
            scalar_offset,
            instruction_offset,
            voffset,
            vindex,
        );

        let outside = buffer_out_of_bounds(model, &resource, flags, offset, index, element_bytes);

        // `widths` lists components as the format's name does, highest bits first, so the last
        // entry is the component at bit 0 (`x`) and both directions walk them in reverse.
        // `MEASURED_10_11_11` in `tests/execute.rs` pins this: in `10_11_11`, `x` is an eleven-bit
        // channel at bit 0.
        if loading {
            let mut packed = Vec::with_capacity(element_words as usize);
            for word in 0..element_words {
                let step = model.constant(word * 4);
                let address = model.add(base, step);
                packed.push(read_guarded(model, address));
            }

            let mut bit = 0u32;
            for (component, &width) in format.widths.iter().rev().enumerate() {
                let register_component = register
                    + u32::try_from(component).expect("a format has at most four components");
                // `packed_format_admitted` has refused anything spanning a word, so the component
                // is inside this one.
                let value = packed_component(
                    model,
                    packed[(bit / 32) as usize],
                    bit % 32,
                    width,
                    format.kind,
                );
                // Out of range reads zero, exactly as the untyped path answers it.
                let kept = pick(model, outside, zero, value);
                model.write_vector_lane(register_component, lane, kept);
                bit += width;
            }
        } else {
            // A store is admitted only for a packed float that is exactly one word; `packed_store`
            // assembles and writes it.
            packed_store(model, instruction, data, format, base, outside, lane)?;
        }
    }
    model.count();
    Ok(())
}

/// Packs a lane's channels and writes the one word a packed float store produces.
///
/// The store half of [`packed_buffer_memory`]'s per-lane body. Each channel is read from its
/// register, packed by clamp-and-truncate ([`Model::float_to_narrow_float_bits`]) and shifted into
/// place; out of bounds the previous contents are kept, as the untyped store does.
fn packed_store<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    data: &Operand,
    format: &orbistoun_shader::BufferFormat,
    base: Id,
    outside: Id,
    lane: u32,
) -> Result<(), TranslateError> {
    let mut assembled = model.constant(0);
    let mut bit = 0u32;
    for (component, &width) in format.widths.iter().rev().enumerate() {
        let source = step_operand(
            data,
            u32::try_from(component).expect("at most four components"),
        )?;
        let value_bits = model.read_source(instruction, &source, lane)?;
        let field = model.float_to_narrow_float_bits(value_bits, width);
        let shift = model.constant(bit);
        let placed = model.binary(op::SHIFT_LEFT_LOGICAL, field, shift);
        assembled = model.binary(op::BITWISE_OR, assembled, placed);
        bit += width;
    }
    let word = model.word_index(base);
    let previous = model.read_memory(word);
    let kept = pick(model, outside, previous, assembled);
    write_guarded(model, base, kept, lane);
    Ok(())
}

/// Translates the untyped buffer accesses.
///
/// `buffer_load_dword`-family and `buffer_store_dword`-family accesses through a resource constant
/// in four scalar registers. Swizzled buffers and thread-id addressing are forced out of bounds at
/// run time (D147). The typed accesses share the body through [`buffer_access`]; see
/// [`typed_buffer_memory`].
fn buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    // Untyped: no format and no conversion, so the name adds only a width. `dwordx2`/`x3`/`x4` move
    // that many consecutive dwords into consecutive registers, as `buffer_access` does per
    // component.
    let loading = name.starts_with("buffer_load");
    let components = match name.rsplit_once("dword") {
        Some((_, "")) => 1,
        Some((_, "x2")) => 2,
        Some((_, "x3")) => 3,
        Some((_, "x4")) => 4,
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "an untyped buffer access names a width this does not translate",
            });
        }
    };
    buffer_access(model, instruction, loading, components)
}

/// The body shared by the typed and untyped buffer accesses.
///
/// They differ only in whether a format was checked and how many components move, and share the
/// descriptor, the addressing equation and both bounds checks. `components` are consecutive dwords
/// at consecutive addresses, written to consecutive registers.
fn buffer_access<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    loading: bool,
    components: u32,
) -> Result<(), TranslateError> {
    let [data, vaddr, resource_operand, soffset] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer access does not have four operands",
        });
    };
    let Operand::Vector(register) = data else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a buffer access names something other than a vector register for ",
                "its data"
            ),
        });
    };
    let register = u32::from(*register);
    let Operand::Scalar(resource_base) = resource_operand else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant is not in scalar registers",
        });
    };
    let resource_base = u32::from(*resource_base);
    if resource_base + 4 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant runs past the end of the register file",
        });
    }

    // The addressing modifiers are bits of the instruction rather than operands: the
    // literal offset in bits 11:0, `offen` at 12 and `idxen` at 13.
    let word = instruction.word;
    let literal_offset = word & 0xFFF;
    let offen = word & (1 << 12) != 0;
    let idxen = word & (1 << 13) != 0;

    let resource = read_buffer_resource(model, resource_base);
    let flags = model.read_scalar(resource_base + 3);
    let instruction_offset = model.constant(literal_offset);

    for lane in running_lanes(model) {
        let scalar_offset = model.read_source(instruction, soffset, lane)?;

        // With both modifiers the address register is a pair, index first then offset; with one it
        // is a single register holding that one.
        let (vindex, voffset) = match (idxen, offen) {
            (false, false) => (None, None),
            (true, false) => (Some(model.read_source(instruction, vaddr, lane)?), None),
            (false, true) => (None, Some(model.read_source(instruction, vaddr, lane)?)),
            (true, true) => {
                let Operand::Vector(first) = vaddr else {
                    return Err(TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: concat!(
                            "a buffer access with both address modifiers does not ",
                            "name a vector register pair"
                        ),
                    });
                };
                let base = *first;
                let index = model.read_source(instruction, &Operand::Vector(base), lane)?;
                let offset = model.read_source(instruction, &Operand::Vector(base + 1), lane)?;
                (Some(index), Some(offset))
            }
        };

        let zero = model.constant(0);
        let offset_term = voffset.unwrap_or(zero);
        let offset = model.add(instruction_offset, offset_term);
        let index = vindex.unwrap_or(zero);

        let base = buffer_address(
            model,
            &resource,
            scalar_offset,
            instruction_offset,
            voffset,
            vindex,
        );

        for component in 0..components {
            // Consecutive dwords. The step is added to both the address and the offset the bounds
            // check sees, so a multi-channel access at the end of a buffer is checked per word.
            let step = model.constant(component * 4);
            let address = model.add(base, step);
            let offset = model.add(offset, step);

            // Two bounds: the buffer's record count and the module's guest-memory window. An access
            // can satisfy one and not the other.
            let outside = buffer_out_of_bounds(model, &resource, flags, offset, index, 4);
            let register = register + component;

            if loading {
                // Out of range reads zero, as the reference states.
                let value = read_guarded(model, address);
                let kept = pick(model, outside, zero, value);
                model.write_vector_lane(register, lane, kept);
            } else {
                let source = step_operand(data, component)?;
                let value = model.read_source(instruction, &source, lane)?;
                let word = model.word_index(address);
                let previous = model.read_memory(word);
                let kept = pick(model, outside, previous, value);
                write_guarded(model, address, kept, lane);
            }
        }
    }
    model.count();
    Ok(())
}

/// The `component`th register of a multi-register operand.
///
/// Only a vector register can be stepped; stepping a scalar or an inline constant would read a
/// neighbouring value as data.
fn step_operand(operand: &Operand, component: u32) -> Result<Operand, TranslateError> {
    match operand {
        Operand::Vector(base) => Ok(Operand::Vector(
            base.checked_add(u16::try_from(component).unwrap_or(u16::MAX))
                .ok_or(TranslateError::Unsupported {
                    offset: 0,
                    detail: "a multi-channel store runs past the register file",
                })?,
        )),
        _ if component == 0 => Ok(operand.clone()),
        _ => Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a multi-channel store does not name a vector register",
        }),
    }
}

/// Reads guest memory, answering zero for an address outside the window. With [`write_guarded`],
/// this stops an out-of-window access aliasing a real one; see [`Model::address_within_window`].
fn read_guarded<M: Model + ?Sized>(model: &mut M, address: Id) -> Id {
    let inside = model.address_within_window(address);
    let index = model.word_index(address);
    let value = model.read_memory(index);
    let zero = model.constant(0);
    pick(model, inside, value, zero)
}

/// Writes guest memory, dropping a write to an address outside the window, by writing back what was
/// there rather than branching around the store.
fn write_guarded<M: Model + ?Sized>(model: &mut M, address: Id, value: Id, lane: u32) {
    let inside = model.address_within_window(address);
    let index = model.word_index(address);
    let previous = model.read_memory(index);
    let kept = pick(model, inside, value, previous);
    model.write_memory(index, kept, lane);
}

/// What a texture access's operands say, once they have been checked.
///
/// Register numbers rather than [`Operand`]s, because each has been bounds-checked by the time this
/// exists.
struct ImageAccess {
    /// First vector register the components land in.
    destination: u32,
    /// First vector register of the coordinate.
    address: u32,
    /// First scalar register of the image descriptor.
    descriptor: u32,
    /// First scalar register of the sampler descriptor, when the instruction names one.
    sampler: Option<u32>,
    /// Which components come back, one bit each.
    mask: u32,
    /// Whether this reads a texel by its index rather than sampling a position.
    fetches: bool,
    /// Whether the level of detail comes from an address register rather than being named.
    levelled: bool,
}

/// Address registers a two-dimensional access uses, by instruction.
///
/// Two for a coordinate and three where the level is one of them: the assembler refuses an address
/// operand whose register count does not match the instruction's dimensionality field.
const fn address_registers(levelled: bool) -> u32 {
    if levelled { 3 } else { 2 }
}

/// Reads a texture access's operands, refusing anything it cannot use.
///
/// The operand at index three differs: a sample names a sampler there and a fetch its mask. The
/// mnemonic says which, and the solved layouts agree (five operands for a sample, four for a
/// fetch).
fn image_access(instruction: &Instruction, name: &str) -> Result<ImageAccess, TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };

    // Two dimensions, checked: every image translation here reads two coordinate registers, and an
    // image of another dimensionality would sample somewhere else.
    let (shift, mask) = IMAGE_DIMENSION;
    let dimension = (instruction.word >> shift) & mask;
    if dimension != IMAGE_DIMENSION_2D {
        return Err(refuse(concat!(
            "only a two-dimensional image is translated - this instruction's coordinate has a ",
            "different number of components, and reading two of them would sample a place the ",
            "guest did not name"
        )));
    }

    // A load reads a texel by integer coordinate and names no sampler; a sample takes a position
    // across the image and names one.
    let fetches = name == "image_load";
    // The level from a register rather than named: the last address element, after the coordinates,
    // as compiled levelled samples place it.
    let levelled = name == "image_sample_l";

    let (Some(Operand::Vector(destination)), Some(Operand::Vector(address))) =
        (instruction.operands.first(), instruction.operands.get(1))
    else {
        return Err(refuse(
            "a texture access's destination and coordinate are not vector registers",
        ));
    };
    let Some(Operand::Scalar(descriptor)) = instruction.operands.get(2) else {
        return Err(refuse(concat!(
            "a texture access's image descriptor is not a scalar register - it names the first ",
            "register of a group, and a group has to start somewhere"
        )));
    };
    let sampler = match instruction.operands.get(3) {
        Some(Operand::Scalar(sampler)) if !fetches => Some(u32::from(*sampler)),
        _ => None,
    };
    let mask_at = if sampler.is_some() { 4 } else { 3 };
    let Some(Operand::Immediate(mask)) = instruction.operands.get(mask_at) else {
        return Err(refuse(
            "a texture access carries no component mask, so what it returns is unknown",
        ));
    };
    let Ok(mask) = u32::try_from(*mask) else {
        return Err(refuse("a texture access's component mask is negative"));
    };
    if mask == 0 || mask >= 1 << IMAGE_COMPONENTS {
        return Err(refuse(concat!(
            "a texture access's component mask selects nothing, or selects a component the ",
            "instruction does not have"
        )));
    }

    // The address registers and the components coming back both have to fit.
    let address = u32::from(*address);
    let destination = u32::from(*destination);
    if address + address_registers(levelled) > VECTOR_REGISTERS
        || destination + mask.count_ones() > VECTOR_REGISTERS
    {
        return Err(refuse(
            "a texture access runs past the end of the vector register file",
        ));
    }

    Ok(ImageAccess {
        destination,
        address,
        descriptor: u32::from(*descriptor),
        sampler,
        mask,
        fetches,
        levelled,
    })
}

fn image_sample<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };

    let ImageAccess {
        destination,
        address,
        descriptor,
        sampler,
        mask,
        fetches,
        levelled,
    } = image_access(instruction, name)?;

    let texture = model.sampled_image(descriptor, sampler).map_err(refuse)?;

    // Which sampling instruction this is: `lz` is level zero named, `_l` takes the level from a
    // register, and the plain form lets the implementation choose from derivatives, a different
    // SPIR-V instruction with different operands.
    let named_level = name.ends_with("_lz") || levelled;

    let f32_type = model.f32_type();
    let u32_type = model.u32_type();
    let zero = model.constant(0);
    let constant_level = model.as_float(zero);

    for lane in running_lanes(model) {
        // Two dimensions from consecutive registers; the dimensionality field was checked above.
        let u = model.read_source(instruction, &Operand::Vector(address_of(address)), lane)?;
        let v = model.read_source(instruction, &Operand::Vector(address_of(address + 1)), lane)?;
        // The level, where the instruction takes one from a register: the last address element.
        let level = if levelled {
            let bits =
                model.read_source(instruction, &Operand::Vector(address_of(address + 2)), lane)?;
            model.as_float(bits)
        } else {
            constant_level
        };
        // A fetch's coordinate is a texel index, an integer, so the words go straight in; a
        // sample's is a position, so the words are reinterpreted as float bits.
        let (u, v, kind) = if fetches {
            (u, v, texture.texel)
        } else {
            (model.as_float(u), model.as_float(v), texture.coordinate)
        };

        let builder = model.builder();
        let coordinate = builder.id();
        builder.function(op::COMPOSITE_CONSTRUCT, &[kind.0, coordinate.0, u.0, v.0]);
        // Loaded per lane rather than hoisted, so the translation does not reorder the guest's
        // program on an unchecked aliasing assumption.
        let bound = builder.id();
        builder.function(op::LOAD, &[texture.sampled.0, bound.0, texture.variable.0]);
        let texel = builder.id();
        if fetches {
            // A fetch takes an image, so the bound pair is unwrapped first. Vulkan requires a level
            // for a non-multisampled image, and zero is the only level bound.
            let image = builder.id();
            builder.function(op::IMAGE, &[texture.image.0, image.0, bound.0]);
            builder.function(
                op::IMAGE_FETCH,
                &[
                    texture.result.0,
                    texel.0,
                    image.0,
                    coordinate.0,
                    image_operands::LOD,
                    zero.0,
                ],
            );
        } else if named_level {
            builder.function(
                op::IMAGE_SAMPLE_EXPLICIT_LOD,
                &[
                    texture.result.0,
                    texel.0,
                    bound.0,
                    coordinate.0,
                    image_operands::LOD,
                    level.0,
                ],
            );
        } else {
            builder.function(
                op::IMAGE_SAMPLE_IMPLICIT_LOD,
                &[texture.result.0, texel.0, bound.0, coordinate.0],
            );
        }

        // The mask says which components come back, and they land in consecutive registers, so a
        // hole in the mask leaves no hole in the destination.
        let mut written = 0;
        for component in 0..IMAGE_COMPONENTS {
            if mask & (1 << component) == 0 {
                continue;
            }
            let builder = model.builder();
            let extracted = builder.id();
            builder.function(
                op::COMPOSITE_EXTRACT,
                &[f32_type.0, extracted.0, texel.0, component],
            );
            let bits = builder.id();
            builder.function(op::BITCAST, &[u32_type.0, bits.0, extracted.0]);
            model.write_vector_lane(destination + written, lane, bits);
            written += 1;
        }
    }

    model.count();
    Ok(())
}

/// Translates a texel store.
///
/// Four operands, shaped like a fetch: the data registers, the coordinate registers, the image
/// descriptor and the component mask, which decides how many consecutive registers hold the texel.
/// Components the mask does not select are written as zero: the format that would define them is in
/// a descriptor that is not decoded (D692).
fn image_store<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };

    let ImageAccess {
        destination: data,
        address,
        descriptor,
        mask,
        ..
    } = image_access(instruction, name)?;

    let stored = model.storage_image(descriptor).map_err(refuse)?;
    let zero = model.constant(0);

    for lane in running_lanes(model) {
        let u = model.read_source(instruction, &Operand::Vector(address_of(address)), lane)?;
        let v = model.read_source(instruction, &Operand::Vector(address_of(address + 1)), lane)?;

        // The texel's components, taken in mask order from consecutive registers, so a hole in the
        // mask reads no register for that component.
        let mut components = [model.as_float(zero); IMAGE_COMPONENTS as usize];
        let mut read = 0;
        for (component, slot) in components.iter_mut().enumerate() {
            let selected = u32::try_from(component).unwrap_or(IMAGE_COMPONENTS);
            if mask & (1 << selected) == 0 {
                continue;
            }
            let source = Operand::Vector(address_of(data + read));
            let bits = model.read_source(instruction, &source, lane)?;
            *slot = model.as_float(bits);
            read += 1;
        }

        let builder = model.builder();
        let coordinate = builder.id();
        builder.function(
            op::COMPOSITE_CONSTRUCT,
            &[stored.texel.0, coordinate.0, u.0, v.0],
        );
        let value = builder.id();
        let mut construct = vec![stored.value.0, value.0];
        construct.extend(components.iter().map(|id| id.0));
        builder.function(op::COMPOSITE_CONSTRUCT, &construct);
        let bound = builder.id();
        builder.function(op::LOAD, &[stored.image.0, bound.0, stored.variable.0]);
        builder.function(op::IMAGE_WRITE, &[bound.0, coordinate.0, value.0]);
    }

    model.count();
    Ok(())
}

/// One vector register number as the decoder spells it.
///
/// Saturating, and unreachable either way: the caller has checked the pair fits the register file.
fn address_of(register: u32) -> u16 {
    u16::try_from(register).unwrap_or(u16::MAX)
}

/// Translates the flat memory instructions.
///
/// Loads and stores of one, two or four consecutive words. A load's destination and a store's data
/// sit in different fields, so each opcode has its own operand layout.
fn flat_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // The flat loads, which differ only in how many consecutive registers they fill; each
        // layout is probed rather than assumed shared.
        "global_load_dword" | "global_load_dwordx2" | "global_load_dwordx4" => {
            let words = access_words(name);
            let (destination, vaddr, base) = first_three_operands(instruction)?;
            let Operand::Vector(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat load destination is not a vector register",
                });
            };
            let register = u32::from(*register);
            if register + words > VECTOR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat load runs past the end of the vector register file",
                });
            }
            for lane in running_lanes(model) {
                let address = model.flat_address(instruction, vaddr, base, lane)?;
                for word in 0..words {
                    // Consecutive words, stepped by address so each word is bounds-checked: a
                    // multi-word access can start inside the window and end outside it.
                    let stepped = step_address(model, address, word);
                    let value = read_guarded(model, stepped);
                    model.write_vector_lane(register + word, lane, value);
                }
            }
            model.count();
            Ok(())
        }

        // The flat stores: per lane and masked, because another lane may read what an inactive lane
        // would have written.
        "global_store_dword" | "global_store_dwordx2" | "global_store_dwordx4" => {
            let words = access_words(name);
            let (vaddr, data, base) = first_three_operands(instruction)?;
            let Operand::Vector(first_register) = data else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat store's data is not a vector register",
                });
            };
            let first_register = u32::from(*first_register);
            if first_register + words > VECTOR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat store reads past the end of the vector register file",
                });
            }
            for lane in running_lanes(model) {
                let address = model.flat_address(instruction, vaddr, base, lane)?;
                for word in 0..words {
                    let stepped = step_address(model, address, word);
                    let source =
                        Operand::Vector(u16::try_from(first_register + word).unwrap_or(u16::MAX));
                    let value = model.read_source(instruction, &source, lane)?;
                    write_guarded(model, stepped, value, lane);
                }
            }
            model.count();
            Ok(())
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this flat memory instruction",
        }),
    }
}

/// Vector registers the guest has.
///
/// The wide flat forms reach the end: `global_load_dwordx4` into v253 would write past the last
/// register.
pub const VECTOR_REGISTERS: u32 = 256;

/// The byte address `offset` words past `address`.
///
/// Stepped by address rather than index so each word is checked against the window; stepping a
/// masked index would wrap the tail onto the front of the buffer.
fn step_address<M: Model + ?Sized>(model: &mut M, address: Id, offset: u32) -> Id {
    if offset == 0 {
        return address;
    }
    let step = model.constant(offset * 4);
    model.add(address, step)
}

/// Translates `s_wqm_b64`: whole quad mode.
///
/// Each group of four result bits is set if any of the source's four is: fold the group with two
/// shifts and two ors, then spread the answer across all four positions.
fn whole_quad_mode<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let (low, high) = sixty_four_bit_source(model, instruction, source)?;

    let low = quad_expand(model, low);
    let high = quad_expand(model, high);

    if let Operand::Named(name) = destination
        && let Some(mask) = lane_mask_name(name)
    {
        model.write_lane_mask(mask, low, high)?;
        model.count();
        return Ok(());
    }
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_wqm_b64 destination is neither a register pair nor a lane mask",
        });
    };
    let register = u32::from(*register);
    if register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_wqm_b64 runs past the end of the register file",
        });
    }
    model.write_scalar(register, low);
    model.write_scalar(register + 1, high);
    model.count();
    Ok(())
}

/// Translates `s_wqm_b32`: whole quad mode on a 32-lane mask.
///
/// One register, so one [`quad_expand`], with `s_mov_b32`'s destination rules: `exec_lo` or
/// `vcc_lo` sets that mask's low half, anything else must be a scalar register. The GL context's
/// textured pixel shader runs it on `exec_lo` so every covered quad's helper pixels execute the
/// sample (oops-sdk `tools/shader/tex-prolog.s`).
fn whole_quad_mode_32<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let value = model.read_source(instruction, source, 0)?;
    let expanded = quad_expand(model, value);
    if let Some(mask) = mask_destination(destination) {
        write_mask_low(model, mask, expanded)?;
        model.count();
        return Ok(());
    }
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_wqm_b32 destination is neither a scalar register nor a lane mask",
        });
    };
    model.write_scalar(u32::from(*register), expanded);
    model.count();
    Ok(())
}

/// Sets every bit of each four-bit group whose group contained any set bit.
///
/// Fold the group down to its lowest bit with two shift-and-or steps, mask to the group
/// starts, then multiply by 0b1111 to spread it back. The multiply cannot carry between
/// groups because only the low bit of each group survives the mask.
fn quad_expand<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let two = model.constant(2);
    let one = model.constant(1);
    let starts = model.constant(0x1111_1111);
    let spread = model.constant(0b1111);

    let shifted_two = model.binary(op::SHIFT_RIGHT_LOGICAL, value, two);
    let folded = model.binary(op::BITWISE_OR, value, shifted_two);
    let shifted_one = model.binary(op::SHIFT_RIGHT_LOGICAL, folded, one);
    let folded = model.binary(op::BITWISE_OR, folded, shifted_one);
    let lowest = model.binary(op::BITWISE_AND, folded, starts);
    model.binary(op::IMUL, lowest, spread)
}

/// Translates an instruction that reaches guest memory.
///
/// Reached only for admitted families, so the fallthrough is a translator bug; it still returns an
/// error rather than panicking.
fn memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // The scalar loads, which differ only in how many consecutive registers they fill.
        "s_load_dword" | "s_load_dwordx2" | "s_load_dwordx4" | "s_load_dwordx8" => {
            // The suffix says how many consecutive registers are filled; opcodes are consecutive
            // only on the generation they were numbered for.
            let words = access_words(name);
            let (destination, base, offset) = three_operands(instruction)?;
            let (Operand::Scalar(register), Operand::Scalar(base_register)) = (destination, base)
            else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a scalar load needs a scalar destination and a scalar base",
                });
            };
            let Operand::Immediate(bytes) = offset else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a scalar load offset is not an immediate",
                });
            };
            let bytes = u32::try_from(*bytes).map_err(|_| TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "negative load offset",
            })?;

            // A destination running off the end of the register file is refused rather than
            // truncated: the registers stop at 101 and the wide forms take up to eight.
            let register = u32::from(*register);
            if register + words > SCALAR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a scalar load runs past the end of the register file",
                });
            }

            let base_value = model.read_scalar(u32::from(*base_register));
            let offset_value = model.constant(bytes);
            let address = model.add(base_value, offset_value);
            let first = model.word_index(address);
            for word in 0..words {
                // Consecutive words, so consecutive indices: the address arithmetic is done once
                // and stepped.
                let index = if word == 0 {
                    first
                } else {
                    let step = model.constant(word);
                    model.add(first, step)
                };
                let loaded = model.read_memory(index);
                model.write_scalar(register + word, loaded);
            }
            model.count();
            Ok(())
        }

        // Flat memory: a per-lane address rather than a uniform one.
        "global_load_dword"
        | "global_load_dwordx2"
        | "global_load_dwordx4"
        | "global_store_dword"
        | "global_store_dwordx2"
        | "global_store_dwordx4" => flat_memory(model, instruction, name),

        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this instruction",
        }),
    }
}
