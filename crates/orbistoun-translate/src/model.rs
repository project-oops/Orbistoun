//! What every wavefront model has to provide, and the instruction dispatch they share.
//!
//! # Why this exists now and not before
//!
//! Two backends were written with the dispatch duplicated between them, deliberately:
//! factoring a seam before two implementations exist is guessing where it goes. With
//! both present the seam is observable, and it turns out to be narrow - four methods.
//!
//! The payoff is that an instruction is translated **once**. Adding floating-point
//! arithmetic or a memory access is now one match arm rather than two that must be
//! kept saying the same thing, and two implementations that can disagree about what an
//! instruction means is exactly the bug a differential test would then be unable to
//! find, because both sides would be wrong in the same place.
//!
//! # What the models actually differ in
//!
//! Less than it appeared. `v_mov_b32` is "for each lane, read the source and write the
//! destination" in both; what changes is how many lanes there are, where a register
//! lives, and whether a write is masked. Everything else was duplication.

use orbistoun_shader::{EncodingTable, Instruction, Operand};
use orbistoun_spirv::{Builder, Id, image_operands, op};

use crate::TranslateError;
use crate::modifiers::Modifiers;

/// Every instruction the predicated strategy translates, as (family, opcode).
///
/// Public because a coverage report needs to know what is done in order to rank what is
/// not. Shared by every fidelity level: which instructions are understood is a property
/// of the translator, not of how the wavefront is modelled.
/// Every instruction the translator understands, by **name**.
///
/// # Why names and not opcode numbers
///
/// This list used to be `(family, opcode)` pairs. Opcode numbers are a property of one
/// architecture generation and most of them move between generations - the same
/// arithmetic can sit at a different number, in a family whose identifying bits also
/// changed. A list of numbers retargeted to another generation does not fail; it binds
/// silently to whichever instructions happen to occupy those numbers.
///
/// Names mostly do not move. A handful do - one generation's `v_add_u32` is another's
/// `v_add_nc_u32` - and that is the point: a name this target does not have is
/// **reported**, by [`unresolved`], rather than translated into something else.
///
/// The names are the ones the reference assembler prints, recorded by the probe solver
/// alongside each opcode's operand layout. Both come from the same observation, so they
/// cannot disagree about what an opcode is called.
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
/// Empty on a target the tables were built for. Non-empty after a retarget, naming
/// exactly what needs attention - which is the failure mode this list exists to produce
/// instead of a silent mis-binding.
pub fn unresolved(encodings: &EncodingTable) -> Vec<&'static str> {
    SUPPORTED
        .iter()
        .copied()
        .filter(|name| encodings.find_by_name(name).is_none())
        .collect()
}

/// Whether the translator understands the instruction at a family and opcode.
///
/// Resolved through the loaded table's names, so the answer follows the generation the
/// tables were generated for rather than a number compiled in here. An opcode the table
/// cannot name is not supported - which is the right answer: an unnamed opcode is one
/// nothing has observed, and translating it would be acting on a number alone.
pub fn supports_named(encodings: &EncodingTable, family: &str, opcode: u32) -> bool {
    encodings
        .mnemonic_for(family, opcode)
        .is_some_and(|name| SUPPORTED.contains(&name))
}

/// The operand code a flat access uses to say it has no scalar base register.
///
/// **Measured, after being wrong.** This was the top of the seven-bit field, 0x7f, which the
/// shared operand numbering names `exec_hi` - a plausible reading of "nothing here" and not
/// what the hardware uses. The reference assembler encodes `global_store_dword v[8:9], v10,
/// off offset:4` as `0xdc708004 0x007d0a08`, so the marker is **0x7d**, which the operand
/// table names `null`; the same bytes are in `primitive.gcn`, and the same bytes again are
/// what the console ran in both GL cube shaders (worklogs 545 and 552).
///
/// Nothing caught it for two phases because the constant and every test that exercised it
/// were written together: the tests encoded 0x7f, the translator accepted 0x7f, and the pair
/// agreed with each other and with no shader that exists. The one real stream that reached
/// this code was refused, and the refusal was read as an open question about the hardware.
///
/// It is matched by name rather than fixed in the operand table because the same code means
/// different things in different fields, which is a fact about the encoding.
pub const FLAT_NO_BASE: &str = NO_DESTINATION;

/// The operand that names no register at all.
///
/// One spelling because it is one thing: a field saying it holds nothing.
/// [`FLAT_NO_BASE`] is this name in an address field, where it means the access has no base
/// register; in a **destination** field it means the result is not wanted and must not be
/// written anywhere.
///
/// A shader uses it where a value is computed for its other effects: the division pre-scale
/// produces a scaled operand and a flag, and a shader that only needs the operand writes the
/// flag here. Translating that as a write to some register would invent a destination the
/// guest deliberately declined to name.
pub const NO_DESTINATION: &str = "null";

/// Whether a destination operand says the result is not wanted.
fn discards(operand: &Operand) -> bool {
    matches!(operand, Operand::Named(name) if name == NO_DESTINATION)
}

/// Scalar registers the guest has.
///
/// The shared operand numbering runs past this into specials and inline constants, so a
/// scalar destination at or above it is not a register at all. The wide loads make this
/// reachable: `s_load_dwordx8` at s100 would write four registers and then four
/// specials, and nothing downstream would notice.
pub const SCALAR_REGISTERS: u32 = 102;

/// Scalar registers an image descriptor occupies.
///
/// Eight, which is what the instruction's resource field names: a base address, an extent, a
/// format and a tiling mode. This translation reads none of them - see
/// [`Model::sampled_image`] - and the width still matters, because a write anywhere inside the
/// group means the descriptor is no longer the one the last sample used.
pub const IMAGE_DESCRIPTOR_REGISTERS: u32 = 8;

/// Scalar registers a sampler descriptor occupies.
pub const SAMPLER_DESCRIPTOR_REGISTERS: u32 = 4;

/// Components a sampling instruction can return, one per bit of its mask.
const IMAGE_COMPONENTS: u32 = 4;

/// Where an image instruction says how many dimensions its coordinate has.
///
/// **Measured, and it had been assumed.** Every image translation here reads two coordinate
/// registers, which is right for a two-dimensional image and wrong for every other kind - and
/// nothing checked, because the field is printed as a symbolic name (`dim:SQ_RSRC_IMG_2D`) and
/// the operand solver skips symbolic modifiers by design. So the dimensionality never reached
/// the operand table and the translation could not have looked at it (worklog 576).
///
/// It is in the **instruction**, not in the descriptor, which is the part that was written down
/// wrong. Assembling one instruction at each of the eight dimensionalities and differencing the
/// encodings puts it at these bits and nowhere else: every other byte of the eight is identical.
const IMAGE_DIMENSION: (u32, u32) = (3, 0b111);

/// The dimensionality code for a two-dimensional image.
///
/// Zero is one-dimensional and the codes run upward in the order a disassembler prints them:
/// 1D, 2D, 3D, cube, 1D array, 2D array, 2D multi-sampled, 2D multi-sampled array. Measured the
/// same way, from the same eight encodings.
const IMAGE_DIMENSION_2D: u32 = 1;

/// What a translated texture sample needs from the model.
///
/// Four identifiers that only exist together: the variable, what loading it produces, the type
/// of the coordinate handed to it, and the type it answers with. They are declared in one place
/// on first use, so they travel in one value rather than as four accessors that could each be
/// called without the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texture {
    /// The variable holding the image and its sampler together.
    pub variable: Id,
    /// The type a load of [`variable`](Self::variable) produces.
    pub sampled: Id,
    /// The image type inside that, which a fetch takes and a sample does not.
    ///
    /// A guest's `image_load` names an image descriptor and no sampler, because it reads a
    /// texel by its integer coordinate and there is nothing to filter. On the host that is a
    /// fetch, and a fetch takes an image - so this is what the bound sampled image is unwrapped
    /// to, and it is why reading a texel needs no binding of its own (worklog 573).
    pub image: Id,
    /// The two-component float vector a two-dimensional sampling coordinate is.
    pub coordinate: Id,
    /// The two-component unsigned vector a two-dimensional **texel** coordinate is.
    ///
    /// Separate from [`coordinate`](Self::coordinate) because the two are different types and
    /// the guest means different things by them: a sample takes a normalised position across
    /// the image, and a fetch takes the texel's own index.
    pub texel: Id,
    /// The four-component float vector a sample or a fetch answers with.
    pub result: Id,
}

/// What a translated texture store needs from the model.
///
/// Separate from [`Texture`] because a storage image is a separate binding and a separate
/// declaration - a module that only samples must not declare one, because declaring it means
/// declaring a capability the device may not have (D692).
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
/// A sixty-four-bit operand names its pair by the low register, and the operand table
/// gives that code the name it has as a thirty-two-bit register. So `exec` decodes as
/// `exec_lo` and the width has to come from the opcode - the same situation as
/// [`FLAT_NO_BASE`], and a fact about the encoding rather than a decoding fault.
pub const EXEC_LOW_HALF: &str = "exec_lo";

/// How the condition mask's low half arrives from the decoder.
///
/// Where a comparison puts its answer: one bit per lane, and the operand a shader then
/// ands into the execution mask to enter a conditional region.
pub const VCC_LOW_HALF: &str = "vcc_lo";

/// The `m0` register, as the operand table names it.
///
/// One thirty-two-bit word of scalar state outside the register file: a pixel shader
/// hands the interpolator its primitive mask through it, and a primitive shader tells the
/// geometry engine how much it will emit through it. Held apart from the scalar file
/// because its operand code (124) sits past the last scalar register, in the range the
/// wide loads are refused from writing into.
pub const M0: &str = "m0";

/// A lane mask, by whichever spelling reached the translator.
///
/// The same register arrives under two names. A source field holding code 106 decodes
/// through the operand table, which names codes as 32-bit registers, so it reads
/// `vcc_lo`. A comparison's destination is not encoded at all and comes from the
/// operand layout as the text the reference printed, which is `vcc` - the 64-bit
/// spelling, because that is what the instruction writes.
///
/// Neither name is wrong and neither carries the width; the width comes from the opcode.
/// Normalising here rather than picking one and rewriting the other keeps both tables
/// saying what they observed.
pub fn lane_mask_name(name: &str) -> Option<&'static str> {
    match name {
        "exec" | EXEC_LOW_HALF => Some(EXEC_LOW_HALF),
        "vcc" | VCC_LOW_HALF => Some(VCC_LOW_HALF),
        _ => None,
    }
}

/// Whether an instruction reads or writes a lane mask.
///
/// Mostly a property of the operands rather than of the opcode: `s_mov_b64` needs a mask
/// when its destination is `exec` and does not when it is an ordinary register pair. A
/// table keyed on the opcode alone would have to say yes to both, and would push every
/// shader containing any 64-bit move onto the slow model.
///
/// The exception is a branch. `s_cbranch_execz` names no mask in its operands - its only
/// operand is a target - so for those the opcode has to say. Missing them would send a
/// looping shader to the lane model, which refuses it, and [`Fidelity::Auto`](crate::Fidelity::Auto) would
/// report a shader it could have translated as untranslatable.
///
/// The family is required rather than convenient. An opcode number means nothing on its
/// own, and matching `6..=9` across every family would put any shader containing a
/// `v_mul_f32` onto a model sixty-four times slower for no reason at all.
pub fn touches_mask(instruction: &Instruction, name: &str) -> bool {
    let branches_on_a_mask = matches!(
        name,
        "s_cbranch_vccz" | "s_cbranch_vccnz" | "s_cbranch_execz" | "s_cbranch_execnz"
    );

    // `v_cndmask_b32` reads one bit of a mask *per lane*, so it needs a model that knows
    // which lane it is. The per-lane model does not - its single invocation is not
    // lane zero, it is an unspecified lane - so it must not be handed one.
    // Whole quad mode operates on a sixty-four-bit lane mask, so it needs a model with
    // one. Its operands may be ordinary register pairs, so the operand check below cannot
    // see it.
    let whole_quad = matches!(name, "s_wqm_b64" | "s_wqm_b32");

    // The local data share is storage the lanes of a wavefront share. A model with one
    // lane per invocation would give each its own, so a shader using it to exchange
    // values between lanes would read back whatever it wrote itself - plausible, and
    // wrong.
    let shares_between_lanes = name.starts_with("ds_");

    // `v_div_fmas_f32` names no mask in its operands and reads one anyway - the
    // reference is explicit that the condition mask is implicit - so it has to be listed
    // rather than detected.
    // A second scalar destination used to be listed here too. It did not need to be: one
    // that *is* a mask arrives as a named operand and the check below catches it, and one
    // that is not is refused for a different reason entirely. The list was doing the
    // operand check's job with a copy of its answer.
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
/// A register pair reads both registers. A constant is **sign-extended** rather than
/// repeated: -1 fills both halves and 1 sets the low half to one and the high half to
/// zero. Repeating the low word is right for -1 and wrong for everything else, which
/// matters most here of all - `s_mov_b64 exec, -1` enables every lane and is the common
/// case, so a translation that only ever gets that one right looks correct until a
/// shader enables some other set.
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
/// # Why this is a list, when the sub-encoding one was deleted for being a list
///
/// A fair question, and the answer is where the fact comes from. That one duplicated
/// something the probe solver already recorded, so it could be derived and the copy could
/// only ever drift. This cannot be derived from anything here: a side effect on hidden
/// state is invisible in the encoding, in the operand layout, and in any test that checks
/// destinations. It is read out of the published instruction set, and a list is the honest
/// shape for a fact with one source.
///
/// What *is* new is that it can now be **checked**. A compiler emitting a shader will
/// place instructions between one that sets the condition code and one that branches on
/// it, and every instruction it puts there is one it believes does not write it. That is
/// an observation about real compiled output, and
/// `the_corpus_agrees_about_hidden_side_effects` mines the fixtures for it.
///
/// Thin evidence today - a corpus of ten fixtures yields a handful of such windows - and
/// it grows with the corpus rather than needing anyone to remember. D129 said this class
/// of fault needs a different habit to catch; this is that habit, mechanised.
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
            // The logic sets it to whether the result was non-zero, at either width.
            | "s_and_b32"
            | "s_or_b32"
            | "s_xor_b32"
            | "s_and_b64"
            | "s_or_b64"
            | "s_andn2_b64"
    )
}

/// Instructions that read the condition code.
///
/// Only the branches do, which is what makes the window between a write and a read
/// findable at all.
pub fn reads_condition_code(name: &str) -> bool {
    matches!(name, "s_cbranch_scc0" | "s_cbranch_scc1")
}

/// The refusal an instruction gets for not being in [`SUPPORTED`].
///
/// A named constant because a test asserts on it. An instruction that *is* supported can
/// still be refused - for operands it cannot use, or a fidelity with no lane mask - and
/// telling those two apart by matching on prose would break the moment the prose
/// improved.
pub const NO_TRANSLATION: &str = "no translation for this instruction";

/// Whether the translator understands an instruction, by name.
///
/// The numeric form this replaced took a family and an opcode, which are properties of
/// one architecture generation. Callers that have an instruction rather than a name want
/// [`supports_named`], which resolves through the loaded table.
pub fn supports(mnemonic: &str) -> bool {
    SUPPORTED.contains(&mnemonic)
}

/// Instructions understood well enough to say what they are waiting on.
///
/// **Empty, for the second time.** It emptied once at D553, when the export - its only entry
/// then - became translatable at the fragment stage, and it was kept rather than deleted
/// precisely so the next instruction waiting on a decision had somewhere to be recorded. Two
/// went in after that and both have now come out, which is what the list is for.
///
/// The texture sample was the last, and its entry said the obstacle was "a descriptor this
/// translator has no model for and a host image view nothing here declares". Half of that was
/// solved by building the host half first (worklog 566) and the other half was settled by
/// deciding it did not need solving: D690 maps every sample in a module onto the one bound
/// texture and refuses a module that names a second, so the descriptor still has no model and
/// the instruction translates anyway.
///
/// The parameter move left for a different reason, worth keeping beside that one. It was listed
/// here as waiting on a source for which operand value names which parameter - and that had
/// been *measured* all along, by the same solver that measured the export targets. What
/// actually blocked it was the interpolation mode of the host input, which is a decoration
/// decided when the variable is declared, and naming the wrong obstacle kept it here for two
/// phases (worklog 551).
///
/// The distinction it draws, between "nobody has looked at this" and "this is waiting on
/// something that is not encoding work", is what stops a worklist sending effort at
/// whichever refusal is most frequent. Empty is the state to want, and keeping the list is
/// cheaper than rediscovering why it existed.
pub const BLOCKED: &[(&str, &str)] = &[];

/// Why an instruction is blocked, if it is one this translator recognises.
///
/// Keyed by **name**. It was keyed by family and opcode number, with the numbers from a
/// different architecture generation - so after a retarget every entry pointed at
/// whatever instruction happened to occupy that slot, and an instruction with a
/// carefully written explanation of why it is blocked would have offered that
/// explanation for something else entirely (D139).
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

    /// The shader's `DX10_CLAMP` mode (`SPI_SHADER_PGM_RSRC1` bit 21): whether an output clamp turns
    /// a NaN into zero. `None` - the default, and every model given no `RSRC1` - refuses a clamped
    /// instruction rather than choosing an answer for it (worklog 834).
    fn dx10_clamp(&self) -> Option<bool> {
        None
    }

    /// How many lanes this model emits code for.
    ///
    /// One where an invocation *is* a lane; the full wavefront where one invocation
    /// simulates all of them.
    fn lanes(&self) -> u32;

    /// Whether `lane` may be active here. `false` only where the execution mask is known at
    /// translation time and has the lane's bit clear, so an instruction whose only effect is a
    /// masked write can emit nothing for that lane - exactly what the hardware does with it.
    ///
    /// The default knows nothing, so every lane may run.
    fn lane_may_run(&self, _lane: u32) -> bool {
        true
    }

    /// Control has reached the start of a guest block, which more than one place may branch to,
    /// so anything this model inferred from the instructions before it no longer holds.
    fn enter_block(&mut self) {}

    /// A constant of the given value, declared once however often it is used.
    fn constant(&mut self, value: u32) -> Id;

    /// The colour output this module exports to, and its vector type, if it has one.
    ///
    /// [`None`] for every module that is not a fragment shader, which is the default and was
    /// every module until D553. An export into a compute dispatch has nowhere to go, and
    /// answering `None` is what makes that a refusal rather than a store somewhere arbitrary.
    fn colour_output(&self) -> Option<(Id, Id)> {
        None
    }

    /// Declares how many vertices and primitives this workgroup will emit.
    ///
    /// [`None`] at any stage but the mesh one, where there is nothing to declare it to - the
    /// same shape [`colour_output`](Self::colour_output) has, and the caller turns it into a
    /// refusal that names the instruction's offset.
    ///
    /// Both counts are values rather than literals, because the guest's are: it writes them
    /// into `m0` and the shader may compute them.
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

    /// Writes one primitive's vertex indices, for the lane that is that primitive. As many as
    /// [`Self::mesh_primitive`] carries: one for a point, two for a line, three for a triangle.
    fn write_mesh_indices(&mut self, _lane: u32, _indices: &[Id]) -> Option<()> {
        None
    }

    /// The primitive this module assembles - only meaningful for a mesh module, and the default
    /// ([`crate::wavefront::MeshPrimitive::Triangles`]) for every other, where it is never read.
    fn mesh_primitive(&self) -> crate::wavefront::MeshPrimitive {
        crate::wavefront::MeshPrimitive::default()
    }

    /// The fragment input carrying an attribute, and its vector type, if this module has one.
    ///
    /// [`None`] for a compute module, and for a fragment module that was built without being
    /// told the shader interpolates that attribute - the inputs are declared in the header, so
    /// which ones exist is decided before any instruction is seen (D555).
    fn attribute_input(&self, _attribute: u32) -> Option<(Id, Id)> {
        None
    }

    /// The texture this module samples, declaring it on first use.
    ///
    /// `descriptor` and `sampler` are the **first scalar register** of each of the two groups a
    /// guest's sampling instruction names: eight registers holding an image descriptor and four
    /// holding a sampler. What those describe is a surface at a guest address in a tiled layout,
    /// and nothing here has a host image for it - so D690 maps every sample in a module onto the
    /// one texture the pipeline bound, and refuses a module where that cannot be the whole
    /// story.
    ///
    /// The sampler is [`None`] for a **fetch**, which names an image descriptor and nothing
    /// else: `image_load` reads a texel by its integer coordinate, and there is nothing for a
    /// sampler to do. A module that fetches and samples the same image descriptor is one
    /// texture and is allowed; only the image half of the recording is compared when a caller
    /// names no sampler.
    ///
    /// The refusal is the point rather than a limitation. A translation that sampled whatever
    /// happened to be bound would render a frame that looks right and is not, which is the
    /// wording D104 already used to refuse inventing a colour attachment.
    ///
    /// The error is a reason rather than a [`TranslateError`], because only the caller knows the
    /// offset to attach to it.
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
    /// A **different binding** from the one [`sampled_image`](Self::sampled_image) answers with,
    /// because they are different host objects: one is read through a sampler and cannot be
    /// written, the other is written and has no sampler. A guest names an image descriptor
    /// identically for both, and that the two are not the same image on the host is a gap D692
    /// records rather than closes.
    ///
    /// The module declares **no format** for it. The guest's format is in a descriptor this
    /// project does not decode, so naming one would be inventing it - and a format invented
    /// wrong reinterprets every texel silently.
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
    /// Whether that masking costs anything is the model's business: where an invocation
    /// is a lane there is nothing to mask, and where one invocation holds the whole
    /// wavefront every write is a select against the mask.
    fn write_vector_lane(&mut self, register: u32, lane: u32, value: Id);

    /// Writes a scalar register.
    ///
    /// Never masked. The scalar unit runs regardless of which lanes are active, and
    /// predicating it would silently change what the guest asked for.
    fn write_scalar(&mut self, register: u32, value: Id);

    /// Counts an instruction as translated.
    fn count(&mut self);

    /// The module under construction.
    fn builder(&mut self) -> &mut Builder;

    /// The unsigned 32-bit type, which every register is.
    fn u32_type(&self) -> Id;

    /// The 32-bit float type, for arithmetic.
    fn f32_type(&self) -> Id;

    /// The id of the imported `GLSL.std.450` extended instruction set, importing it on first
    /// use and caching it thereafter.
    ///
    /// Lazy rather than imported at module setup so a shader that uses no extended instruction
    /// still emits no import - keeping every such module's word stream exactly as it was before
    /// the set existed. Importing twice would declare two sets as far as the validator is
    /// concerned, which is why the id is cached.
    fn glsl_set(&mut self) -> Id;

    /// The 16-bit float type, the intermediate a packed half is read as before it is widened
    /// to a [`f32_type`](Self::f32_type) by the driver's own conversion.
    ///
    /// **Declared on first use, with its capability**, the way the extended instruction set is
    /// imported. Every module used to declare `Float16` and `Int16` whether or not anything
    /// sixteen-bit appeared in it, which asks a device for two features most modules do not
    /// need - and which the validation layer names as an error on a device that was not created
    /// with them, for every module, including the compute dispatches that had been running for
    /// months (worklog 556).
    ///
    /// Only one path reaches these: a typed buffer load whose format has a half channel.
    fn f16_type(&mut self) -> Id;

    /// The 16-bit unsigned type, which a packed half's field is narrowed to before it is read
    /// as a half - a bitcast needs the two sides the same width.
    ///
    /// Declared on first use, like [`f16_type`](Self::f16_type).
    fn u16_type(&mut self) -> Id;

    /// Reads one word of the local data share.
    ///
    /// Storage shared between the lanes of a wavefront, which is what a shader uses to
    /// exchange values between them. A model with one lane per invocation cannot
    /// represent that - each invocation would get its own - so this and its write are
    /// fallible for the same reason the lane masks are.
    fn read_local(&mut self, word_index: Id) -> Result<Id, TranslateError>;

    /// Writes one word of the local data share, honouring the execution mask.
    fn write_local(&mut self, word_index: Id, value: Id, lane: u32) -> Result<(), TranslateError>;

    /// The guest-memory buffer.
    fn memory_buffer(&self) -> Id;

    /// Pointer type for one word of guest memory.
    fn memory_element_ptr(&self) -> Id;

    /// Reads a scalar register, for an address held in one.
    fn read_scalar(&mut self, register: u32) -> Id;

    /// Reads a sixty-four-bit lane mask, by the name of its low half.
    ///
    /// [`EXEC_LOW_HALF`] or [`VCC_LOW_HALF`]. One pair of methods rather than two,
    /// because the two masks differ only in which registers they occupy - the execution
    /// mask decides who runs and the condition mask is where a comparison puts its
    /// answer, and a shader moves values between them constantly.
    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError>;

    /// Writes a sixty-four-bit lane mask.
    ///
    /// Returns an error rather than doing nothing when the model has no such mask. That
    /// is the whole point: a shader that disables lanes and a model that cannot
    /// represent disabled lanes produce a plausible, wrong answer, with nothing in the
    /// output to indicate it. The per-lane model refuses; the wavefront model writes it.
    ///
    /// This is what makes [`Fidelity::Lane`](crate::Fidelity::Lane) safe by decision
    /// rather than by accident - it was previously safe only because no instruction
    /// touching a mask could be translated at all.
    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError>;

    /// Sets bit `lane` of a pair of half-masks from a boolean.
    ///
    /// The shape every comparison needs: a per-lane predicate assembled into one
    /// sixty-four-bit value, low half for lanes 0-31 and high half for the rest.
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
    /// Required rather than provided, because whether an inactive lane's store is
    /// suppressed is exactly what distinguishes the models - and a store that lands
    /// when it should not have corrupts memory another lane will read.
    fn write_memory(&mut self, word_index: Id, value: Id, lane: u32);

    /// Turns a byte address into a word index, inside the window.
    ///
    /// The index is **masked**, so it is always a legal index into the buffer. That is
    /// not a bounds check - it is what makes the access defined. Reading a storage buffer
    /// out of range is undefined behaviour in SPIR-V, so an unclamped index would be a
    /// worse fault than the one it reports.
    ///
    /// Whether the address was *in* range is a separate question, and
    /// [`Model::address_within_window`] answers it. The two are separate because the
    /// masking has to happen regardless and the check is what callers act on.
    fn word_index(&mut self, address: Id) -> Id {
        let two = self.constant(2);
        // Relative to where the window starts. Subtracting first is what makes the index mean
        // the same thing as the check beside it: an address below the base wraps to an enormous
        // number, which the check then refuses.
        let base = self.constant(self.memory_base());
        // The window is a power of two words, so masking is the whole of keeping the
        // index legal.
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
    /// # Why this exists
    ///
    /// [`Model::word_index`] masks, so an address past the end of the window does not
    /// clamp - it **wraps**. A store to the word after the last one lands on the first,
    /// and everything about it looks fine: the shader runs, the buffer changes, and the
    /// change is somewhere the guest never asked for. That is the exact shape of fault
    /// this project spends most of its effort avoiding, sitting inside the one function
    /// every memory access goes through.
    ///
    /// So the callers ask. An access outside the window reads zero and writes nothing -
    /// the same answer the hardware gives for an out-of-range buffer access (D147), and
    /// visibly wrong rather than quietly aliased.
    fn address_within_window(&mut self, address: Id) -> Id {
        let two = self.constant(2);
        let base = self.constant(self.memory_base());
        let words = self.constant(self.memory_words());
        let u32_type = self.u32_type();
        let bool_type = self.bool_type();
        let b = self.builder();
        // Unsigned arithmetic does the work of two comparisons: an address below the base
        // wraps to something enormous, which fails the one test below as surely as an address
        // past the end does.
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
    /// Zero for a module whose addresses are the test's own, which is every module this project
    /// emitted until real shaders arrived. A guest's are not: the GL cube's vertex buffer sat at
    /// `0x200900000`, and a window anchored at zero refuses every access to it - reads answer
    /// zero, writes go nowhere, and the shader draws three identical vertices (worklog 561).
    ///
    /// Thirty-two bits, because that is what an address is by the time it reaches here: a flat
    /// access names its address in a register pair and this reads the low half of it. A guest
    /// whose buffers straddle four gigabytes needs the high half too, and that is a separate
    /// piece of work from this one.
    fn memory_base(&self) -> u32 {
        0
    }

    /// How many words of guest memory this module addresses.
    ///
    /// A property of the module rather than a constant of the crate, so a test can widen
    /// it to reach an address the default window cannot hold. The default is small
    /// because nothing yet knows how large it should be (D101).
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
    /// One bit of hidden state that the scalar compares write and the `scc` branches
    /// read. Held in a private variable rather than a SPIR-V value for the same reason
    /// the registers are: it is written in one arm of the dispatch switch and read in
    /// another, and nothing can cross those.
    ///
    /// Both models have one. Unlike a lane mask it is not per-lane - it is a property of
    /// the wavefront as a whole - so the per-lane model can represent it perfectly well.
    fn condition_code(&mut self) -> Id;

    /// The `m0` register: a private word, like the condition code.
    ///
    /// Both shaders the GL cube ran on the console write it before their first
    /// interpolation, and a shader that copies it back into a register must read what it
    /// wrote. What the hardware goes on to *do* with the value - address the parameter
    /// cache, size an allocation request - has no host counterpart to translate into: an
    /// interpolated input already is the interpolated attribute. So both models hold it
    /// and neither acts on it.
    fn read_m0(&mut self) -> Id;

    /// Writes the `m0` register. Never masked, like every scalar write.
    fn write_m0(&mut self, value: Id);

    /// The program counter: a private variable holding the index of the block to run.
    ///
    /// Private rather than a value, for the same reason the register file is: a SPIR-V
    /// result belongs to the block that produced it, and the counter is written in one
    /// arm of the dispatch switch and read in the header. Nothing can cross those.
    fn program_counter(&mut self) -> Id;

    /// How many guest instructions have been translated so far.
    fn instructions(&self) -> usize;

    /// Stores a boolean into the scalar condition code.
    ///
    /// Widened to a word on the way in, because the code lives in an ordinary private
    /// variable and a boolean has no defined size in a storage class.
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

    /// Whether bit `lane` of a sixty-four-bit mask, held as two halves, is set.
    ///
    /// The general form of what the wavefront model already does for the execution mask,
    /// needed because `v_cndmask_b32` takes an arbitrary register pair rather than a
    /// named one.
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
    /// A base of [`FLAT_NO_BASE`] means there is none and the vector address stands
    /// alone. Any other named operand is refused: it would be a base this translator
    /// does not understand, and ignoring it would put the access at the wrong address.
    fn flat_address(
        &mut self,
        instruction: &Instruction,
        vaddr: &Operand,
        base: &Operand,
        lane: u32,
    ) -> Result<Id, TranslateError> {
        let mut address = self.read_source(instruction, vaddr, lane)?;
        // The byte offset the instruction carries, when it carries one. The reference prints it
        // only when it is not zero, so the operand is absent from an access at offset zero -
        // which is why the layout had no field for it at all until probes varied it, and why
        // every offset access read the wrong word in silence (worklog 565).
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
    /// Provided rather than per-model, because the sequence is identical everywhere and
    /// the one thing it must not do differently is **bitcast rather than convert**.
    ///
    /// A register holds thirty-two bits with no type attached; the instruction decides
    /// how to read them. `OpBitcast` reinterprets those bits, which is what the hardware
    /// does. `OpConvertUToF` would take the *number* 1065353216 and produce the float
    /// 1065353216.0 - same input, an answer wrong by nine orders of magnitude, and a
    /// shader that runs perfectly happily while rendering nonsense.
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
    /// The bitcast bracketing is [`f32_binary`](Self::f32_binary)'s exactly, and for the same
    /// reason: a register holds thirty-two bits with no type, so it is reinterpreted as a float
    /// (`OpBitcast`) rather than converted. What differs is the operation - an `OpExtInst` into the
    /// imported set - which is how the min, max and transcendentals the core opcode set has none of
    /// are spelled.
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
    /// value, returning the result's bits.
    ///
    /// The bitcast bracketing is [`f32_ext_binary`](Self::f32_ext_binary)'s exactly: a register
    /// holds thirty-two bits with no type, so it is reinterpreted as a float (`OpBitcast`),
    /// transformed via `OpExtInst` in `GLSL.std.450`, and reinterpreted back.
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
    /// The exact counterpart to [`f32_binary`](Self::f32_binary)'s bitcast, and the reason that
    /// one's doc warns against confusing the two: there the register already holds a float's bits;
    /// here it holds an integer whose *value* becomes the float - 255 becomes 255.0, not the
    /// float whose bits happen to be 255. Used to lift a packed normalised component and its range
    /// into the float domain before the division that normalises it.
    fn unsigned_to_float_bits(&mut self, value: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();
        let as_float = b.id();
        b.function(op::CONVERT_U_TO_F, &[f32_type.0, as_float.0, value.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, as_float.0]);
        bits
    }

    /// As [`unsigned_to_float_bits`](Self::unsigned_to_float_bits), but reading the register's
    /// value as a signed integer: `-128` becomes `-128.0`, so a sign-extended packed field
    /// carries its sign into the float. The pair exists because the two conversions differ in
    /// exactly the opcode and nothing else, and choosing the wrong one is silent.
    fn signed_to_float_bits(&mut self, value: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();
        let as_float = b.id();
        b.function(op::CONVERT_S_TO_F, &[f32_type.0, as_float.0, value.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, as_float.0]);
        bits
    }

    /// Widens a packed float **narrower than a half** - the 11- and 10-bit channels of formats
    /// like `10_11_11` - to a single-precision float, and returns that float's bits.
    ///
    /// # Why this is arithmetic where the half is a conversion
    ///
    /// [`Self::half_to_float_bits`] hands a half to `FConvert` and lets the hardware's own IEEE
    /// path do it. There is no such instruction for these: they are not IEEE types. Each channel
    /// is an **unsigned** float - no sign bit at all - with a five-bit exponent biased by 15 and
    /// `width - 5` bits of mantissa. So the three cases are built by hand.
    ///
    /// Measured, not derived from the format's name (obSCEne `REQ-...b3d4`, sweep
    /// `20260916-223136`, `166-agc/typed-buffer-formats`, `rc-submit 0x0`, `fence-hit 0x1`):
    /// six known words per format, loaded on the device through `tbuffer_load_format_xyz`, with
    /// all three output floats recorded as raw bit patterns. Thirty-six channel values, and this
    /// reproduces every one of them.
    ///
    /// The two edges are what the measurement bought, and neither could have been guessed safely:
    ///
    /// - **Exponent 31 is Inf/NaN**, and the mantissa lands at the *top* of the single's
    ///   mantissa field rather than the bottom - the all-ones word gives `0x7ffe0000` for an
    ///   11-bit channel and `0x7ffc0000` for a 10-bit one, which is the payload shifted up, not a
    ///   canonical NaN and not a zero-extension.
    /// - **Exponent 0 is subnormal**, and multiplying the mantissa as an integer by
    ///   `2^-(14 + mantissa)` produces the measured bits exactly. Doing it by normalising the
    ///   mantissa with a leading-zero count would be the usual trick and needs an instruction
    ///   this does not have; the multiply needs none.
    ///
    /// The caller must have masked the field to its own bits already.
    fn narrow_float_to_float_bits(&mut self, field: Id, width: u32) -> Id {
        let mantissa_bits = width - 5;

        let exponent_shift = self.constant(mantissa_bits);
        let exponent = self.binary(op::SHIFT_RIGHT_LOGICAL, field, exponent_shift);
        let mantissa_mask = self.constant((1u32 << mantissa_bits) - 1);
        let mantissa = self.binary(op::BITWISE_AND, field, mantissa_mask);

        // The mantissa sits at the top of the single's 23-bit field in every case, so it is
        // placed once and shared by all three.
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

        // Chosen in this order so that exponent 0 wins over the normal arithmetic, which would
        // otherwise produce 2^-15 times the mantissa rather than 2^-14.
        let all_ones = self.constant(31);
        let is_inf_or_nan = self.compare(op::IEQUAL, exponent, all_ones);
        let finite = self.select(is_inf_or_nan, inf_or_nan, normal);
        let zero = self.constant(0);
        let is_subnormal = self.compare(op::IEQUAL, exponent, zero);
        self.select(is_subnormal, subnormal, finite)
    }

    /// Packs a single-precision float into an unsigned N-bit packed float - the inverse of
    /// [`Self::narrow_float_to_float_bits`], for storing a channel of a format like `10_11_11`.
    ///
    /// Measured, not derived (obSCEne `REQ-...2f7a` and `REQ-...9f1c`, sweep `20260917-043235`,
    /// `166-agc/typed-buffer-formats`): the rule is **clamp to `[0, max finite]`, then truncate**.
    /// `1.009375` - 0.6 of a mantissa step above 1.0 - stores as mantissa 0 where round-to-nearest
    /// would give 1, so the rounding is toward zero; and `100000` stores as the largest finite value
    /// (exponent 30, mantissa all ones), not infinity, so an over-range value saturates. Everything
    /// between is truncated. Two edges are the same choice a clamp makes and are not separately
    /// measured: a negative or an underflowing value packs to zero, and infinity or NaN to the largest
    /// finite rather than to the packed infinity.
    ///
    /// `value_bits` is the register's bits - a register holds bits, and which of them are a float is
    /// the instruction's to say.
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
        // largest finite (`exp >= 143`, where Inf and NaN sit) to the largest finite; else the normal.
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

    /// Widens a packed half - a sixteen-bit IEEE float in the low bits of the register - to a
    /// single-precision float, and returns that float's bits.
    ///
    /// The conversion is the driver's own, not a hand-rolled one: the field is narrowed to
    /// sixteen bits (`UConvert`), read as a half (`Bitcast` - which needs the two sides the
    /// same width, hence the narrow first), and widened by `FConvert`. So subnormals,
    /// infinities and NaNs go through the hardware's IEEE path rather than a bit-twiddle whose
    /// edge cases would be wrong silently - the one failure this project has no cheap way to
    /// catch. The caller must have masked the field to its sixteen bits already.
    fn half_to_float_bits(&mut self, field: Id) -> Id {
        // Fetched one at a time because each may declare a type and its capability, so each
        // needs the builder to itself.
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
    /// The inverse of [`Self::half_to_float_bits`], through the same IEEE conversion
    /// (`OpFConvert`) rather than a bit-twiddle, so overflow, denormals and NaNs are the
    /// device's. See `pack_halves` for the rounding this does not promise.
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
/// A flat access decodes three operands at offset zero and four when the reference printed an
/// offset, because the reference prints one only when it is not zero. Demanding *exactly* three
/// refuses the second kind, which is every access with an offset (worklog 565) - so the flat
/// paths ask for the first three and read the offset themselves.
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

/// Translates one instruction into whichever model it is handed.
///
/// Refusing is the default. An instruction with no arm here is an error, never a no-op:
/// a shader missing one instruction computes the wrong thing while appearing to work,
/// What this target calls an instruction, or a refusal saying why it cannot be named.
///
/// Split out from the dispatch below because it answers a different question - *what is
/// this?* rather than *what does it do?* - and because the two together were long enough
/// that neither was easy to read.
///
/// Everything downstream dispatches on the name, because an opcode number is a property
/// of one architecture generation and this translator serves more than one (D139).
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
        // Nothing has observed this opcode on this target, so there is no name to
        // dispatch on. Refused rather than guessed: acting on a bare number is exactly
        // what dispatching by name exists to stop.
        .ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "this target has no recorded name for that opcode, so there is ",
                "nothing to translate it as"
            ),
        })
}
/// `v_interp_p1_f32` / `v_interp_p2_f32` - an interpolated fragment attribute.
///
/// # Two instructions, one host operation
///
/// On the guest these are a pair: `p1` computes `P10 * I + P0` and `p2` adds `P20 * J` to it,
/// with the parameters coming from a cache the hardware fills before the shader runs. SPIR-V has
/// no such pair. A fragment `Input` variable **is** the interpolated attribute - the hardware
/// does the weighting - so the two-step computation collapses into one read.
///
/// **Both halves therefore yield the whole value.** The alternative, treating `p2` as a no-op on
/// the grounds that its destination already holds `p1`'s result, is wrong the moment the two do
/// not share a destination - which `unreached.s` does on purpose, and which the guest permits
/// because `p2` reads its destination as an accumulator. Answering the same value from either
/// half depends on no register history at all (D555).
///
/// # What this assumes, and what would break it
///
/// **`assumed`, not measured.** A shader that used `p1`'s intermediate for anything other than
/// feeding `p2` would get a different number here - the intermediate is a partial sum on the
/// guest and the finished value here. The only documented use is the pair.
///
/// It also ignores both `vsrc` operands, which carry the barycentric I and J. The host
/// interpolates with its own, and a shader that computed its own barycentrics and expected them
/// honoured would be silently wrong rather than refused. That is the sharpest edge of this
/// translation and it is why it is recorded rather than asserted.
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
    // Registers hold bits, not floats: the same convention the export reads them back under.
    let bits = b.id();
    b.function(op::BITCAST, &[u32_type.0, bits.0, component.0]);

    // **Lane zero only.** At the fragment stage one invocation is one pixel, not a wavefront -
    // the host rasteriser decides coverage - so the other lanes of this model are not meaningful
    // here, and the export reads lane zero for the same reason.
    model.write_vector_lane(u32::from(*destination), 0, bits);
    Ok(())
}

/// The parameter a `v_interp_mov_f32` reads, as the operand field encodes it.
///
/// **Measured, not transcribed.** `orbistoun-gen`'s symbolic-code solver assembles the same
/// instruction with each spelling in turn and reads the bits that moved, which is where the
/// differential test's mapping comes from too: `p10` is 0, `p20` is 1, `p0` is 2.
///
/// What the three *are* follows from the interpolation this translator already implements:
/// `v_interp_p1_f32` computes `P10 * I + P0` and `v_interp_p2_f32` adds `P20 * J`, so `P0` is
/// the value where both barycentrics are zero - the first vertex of the primitive - and the
/// other two are deltas from it. A shader wanting the attribute unchanged across the primitive
/// asks for `P0`, which is flat shading with a first-vertex provoking convention, and that is
/// the convention the host uses by default.
const PARAMETER_CONSTANT_TERM: i64 = 2;

/// `v_interp_mov_f32` - a fragment attribute read without interpolating.
///
/// # Why this is the input variable and not an instruction
///
/// The move takes a parameter straight out of the cache the hardware filled. SPIR-V has no
/// parameter cache: an input variable *is* the attribute, and whether reading it interpolates
/// is a property of the variable rather than of the read. So the whole translation of this
/// instruction is the `Flat` decoration the declaration pass already put on that input, and
/// what remains here is the same component read the interpolating pair does (D555).
///
/// # The two that are refused
///
/// `P10` and `P20` are deltas between vertices. Nothing on the host can read one: the host
/// interpolates for you and offers the result, never the gradient it used. A shader moving a
/// delta into a register is doing arithmetic with the primitive's shape, and translating it as
/// the attribute's value would be a plausible number that is wrong everywhere.
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
/// **Measured.** `orbistoun-gen`'s symbolic-code solver assembles the same export with each
/// spelling and reads the bits that moved: colour attachments from 0, `pos0` at 12, `param0` at
/// 32, and the primitive export at 20. The same mapping the differential test carries.
const EXPORT_POSITION: i64 = 12;
/// The first parameter target; `param<n>` is this plus n.
const EXPORT_PARAMETER: i64 = 32;
/// How many parameter targets there are, so a code past them is not read as one.
const EXPORT_PARAMETERS: i64 = 32;
/// The primitive export: the triangle's vertex indices, packed into one register.
const EXPORT_PRIMITIVE: i64 = 20;

/// Which parameter location an export target names, if it names one.
///
/// Public because the declaration pass needs it before any instruction is translated: an
/// output variable is declared in the module header, so which locations exist has to be known
/// first.
#[must_use]
pub fn export_parameter_location(target: i64) -> Option<u32> {
    let location = target.checked_sub(EXPORT_PARAMETER)?;
    (0..EXPORT_PARAMETERS)
        .contains(&location)
        .then(|| u32::try_from(location).unwrap_or(0))
}

/// The message that asks the geometry engine for room to export.
///
/// Measured: the reference assembler encodes `s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)` as
/// `0xbf900009`, and the console-run vertex program carries that exact word (worklog 548).
const MSG_GS_ALLOC_REQ: i64 = 9;

/// Where the vertex and primitive counts sit in `m0`, for the allocation request.
///
/// **Measured: the low field is the vertex count, the high field is the primitive count**
/// (`REQ-20260915T1652Z-6fad`, sweep `20260915-192617`). The primitive-draw triangle, whose
/// body emits one primitive of three vertices, was submitted with three `m0` literals: `0x1003`
/// drew, `0x1004` drew (a surplus of vertices), and `0x3001` never ran and left the fence unhit
/// (the low field held one vertex, starving a body that writes three). So the vertices are low
/// and the primitives are high, not the reverse.
///
/// The **exact boundary is not uniquely pinned** by those three - any split between bits 3 and 12
/// fits them - but twelve is the value that reads oops-sdk's `0x1003` as exactly one primitive of
/// three vertices, which is how that shader encoded it, and it is what real NGG counts (always far
/// below 2^12) are packed at. Note that obSCEne's own resolution stated a sixteen-bit split
/// (`prims = m0 >> 16`); that is refuted by its variant A, which would then declare zero
/// primitives yet drew a triangle (worklog 603).
const MESH_COUNT_BITS: u32 = 12;

/// `s_sendmsg` - a message to a fixed-function unit outside the shader core.
///
/// Only the allocation request is translated, and only at the mesh stage, where the host has
/// the same idea: declare how much this workgroup will emit before emitting any of it. Every
/// other message is refused by name rather than ignored - a message this translator does not
/// understand is a thing the guest asked the hardware to do, and doing nothing is an answer
/// only if somebody has checked.
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

    // The counts the guest wrote into `m0`, read back as values - the host's declaration takes
    // ids, so a shader that computed its counts translates as readily as one that wrote a
    // literal.
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

/// `exp pos0` and `exp param<n>` - one vertex's position or one of its parameters.
///
/// # One lane, one vertex
///
/// The guest narrows its execution mask to the lanes that are vertices and exports once; each
/// active lane is a vertex of the primitive. A mesh shader writes an array indexed the same
/// way, so the translation is the same loop the rest of this model uses, and the store keeps
/// what was there for an inactive lane exactly as a register write does.
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

/// `exp prim` - the triangle's three vertex indices, packed into one register.
///
/// # The packing, which is our own shader's
///
/// oops-sdk's vertex program writes `0x20280600` for "vertices 0, 1, 2 with edge flags", and
/// the console drew the frame that word produced (oracle record A). That value is exactly
/// `0 | 1 << 10 | 2 << 20` with bits 9, 19 and 29 set, which is where the three nine-bit
/// indices and their edge flags sit. The edge flags are not translated: they select which
/// edges a wireframe draws, the host decides that from its own state, and reading them into
/// an index would be a number rather than a flag.
fn mesh_primitive_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    packed: &Operand,
) -> Result<(), TranslateError> {
    /// Bits per index, and the gap between them - the tenth bit of each is an edge flag.
    const INDEX_BITS: u32 = 9;
    /// Where each index starts.
    const INDEX_SHIFTS: [u32; 3] = [0, 10, 20];

    // How many indices this primitive uses: three for a triangle, two for a line, one for a
    // point (`-0c58`). **Assumed** for a point and a line, and marked so: the measured packing
    // (oracle record A) is a triangle, and this reads the low `count` nine-bit fields on the
    // assumption a point or line packs its indices in the same low-to-high order. A point or
    // line record would settle it; until one is measured this is a written-down assumption, not
    // a fact (principle 1).
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

/// The only export target this translates, and what it is.
///
/// `mrt0` - colour attachment zero. Which attachment any *other* target index selects is
/// register state a guest writes, and D104 refuses to invent it; a capture would settle it. So
/// one target is translated and the rest are refused by name, rather than all of them being
/// translated onto the same attachment and appearing to work (D553).
const MRT0: i64 = 0;

/// An export's `COMPR` bit, in its first word: the sources carry packed halves
/// (`aco_assembler.cpp:1001` in the collection's Mesa tree).
const EXPORT_COMPRESSED: u32 = 1 << 10;

/// An export's `EN` field, bits 0-3: which of the four channels it writes
/// (`aco_assembler.cpp:1005`).
const EXPORT_ENABLE_MASK: u32 = 0xf;

/// `exp` - hands four registers to a render target.
///
/// # What is translated
///
/// The four sources are read for lane zero, reinterpreted as floats, assembled into a `vec4`
/// and stored to the module's colour output. **The bits are not converted**: a vector register
/// holds the colour's bit pattern already, so this is a bitcast rather than an arithmetic
/// conversion, and treating it as one would scale every channel.
///
/// # What is refused, and why each refusal is separate
///
/// - **A module with no colour output.** A compute dispatch has nowhere to export to. That is
///   the whole reason [`Model::colour_output`] exists and defaults to `None`.
/// - **Any target but `mrt0`.** See [`MRT0`].
/// - **A write mask narrower than all four channels.** The mask and the compressed bit live in
///   the instruction's first word rather than among the operands the decoder solved; both are
///   read from there (worklog 820). A compressed export is unpacked from its two half-packed
///   sources; a partial mask is refused, because storing a whole `vec4` would overwrite channels
///   the guest meant to keep. `done` and `vm` change nothing a single-export translation does.
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

    // A geometry stage's exports: the vertices' positions and parameters, and the primitive
    // that joins them. One lane is one vertex, which is how the guest arranges the wave (D688).
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

    // Every channel, no channel, or this refuses (`EN`, bits 0-3, `aco_assembler.cpp:1005`). No
    // channel is an export that writes nothing - a shader that raises one only to end the wave,
    // or one whose colour went to a storage image instead - so nothing is stored; before worklog
    // 820 this stored a whole vec4 there too. A partial mask leaves channels the target keeps,
    // and storing a whole vec4 would overwrite them.
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
        // **Compressed**: two registers of two halves each - (r, g) in the first source, (b, a)
        // in the second, low half first, as `v_cvt_pkrtz_f16_f32` packed them; the third and
        // fourth sources are unused (`aco_select_ps_epilog.cpp:170-185`). Read as four whole
        // floats, which is what this did until worklog 820, a compressed export writes two
        // packed bit patterns as red and green and whatever sat in two other registers as blue
        // and alpha.
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
            // Lane zero: one fragment is one pixel, and the wavefront model's lanes are pixels a
            // rasteriser assigns rather than anything this translation chooses.
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

/// The lanes an instruction whose only effect is a masked write emits code for: every lane but
/// those the model knows are inactive (worklog 854). An instruction that writes a **mask** loops
/// over every lane instead, because its answer for an inactive lane is a bit someone reads.
fn running_lanes<M: Model + ?Sized>(model: &M) -> Vec<u32> {
    (0..model.lanes())
        .filter(|&lane| model.lane_may_run(lane))
        .collect()
}

/// which is far harder to find than a translator that stops and names what it hit.
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
        // Two instructions that emit nothing, for different reasons, kept in one arm
        // because the compiler objects to identical bodies and splitting them to
        // satisfy it would be pretence:
        //
        //   s_endpgm  - the shader ends; the epilogue is emitted when the module is
        // finished.
        //   s_waitcnt - waits for outstanding memory operations to land. SPIR-V
        // expresses ordering as semantics on the memory operations
        // themselves rather than as a separate instruction, so there is
        // nothing to emit. A translation rather than a shortcut, provided
        // memory operations carry the right semantics when they arrive.
        //
        //   s_nop     - wait states. The hardware needs them between an SALU write and a
        //               read of the same hidden register; a host with no such hazard needs
        //               nothing emitted, and emitting anything would be inventing work.
        //
        //   s_inst_prefetch - a hint to fetch the next instructions into the instruction
        //               cache. It has no architectural effect at all: a shader with it and
        //               the same shader without compute the same thing, which is what makes
        //               dropping it a translation rather than a shortcut.
        //
        //   s_clause  - "define a clause of instructions which are executed together"
        // (the instruction-set reference for this generation). It groups
        // the instructions that follow so they are issued without
        // interruption; the instructions themselves are unchanged and it
        // computes nothing. A scheduling directive, and scheduling is the
        // host driver's business once this is SPIR-V.
        //
        // Matched explicitly rather than falling through, because "emits nothing" and
        // "nobody handled it" must never look the same.
        "s_endpgm" | "s_waitcnt" | "s_clause" | "s_nop" | "s_inst_prefetch" => Ok(()),

        // The export, which is why a fragment stage exists at all (D553).
        "exp" => export(model, instruction),

        // Both halves of the interpolation pair answer the whole value (D555).
        "v_interp_p1_f32_e32" | "v_interp_p2_f32_e32" => interpolate(model, instruction),

        // The same read, from an input the declaration pass decorated `Flat`.
        "v_interp_mov_f32_e32" => parameter_move(model, instruction),

        // The message a primitive shader opens with, which is a mesh module's first act too.
        "s_sendmsg" => send_message(model, instruction),

        // s_mov_b32: once for the whole wavefront, since scalar registers are uniform.
        // The scalar moves. Split out for the same reason the memory instructions
        // were: the combined match outgrew a screen, and these two ask a question the
        // others do not - how wide the operand is.
        "s_mov_b32" | "s_mov_b64" => scalar_move(model, instruction, name),

        // s_wqm_b64: whole quad mode. Sets each group of four bits of the result if any
        // of the corresponding four in the source is set - so a derivative computed
        // across a quad has all four pixels live even where only one is covered.
        "s_wqm_b64" => whole_quad_mode(model, instruction),
        // s_wqm_b32: the same for a 32-lane wavefront, whose mask is one register - how the
        // GL context's textured pixel shader enters whole-quad mode for its sample
        // (`s_wqm_b32 exec_lo, exec_lo`, worklog 819).
        "s_wqm_b32" => whole_quad_mode_32(model, instruction),

        // The 64-bit scalar logic, which is how a guest computes a mask: narrow it by
        // anding with a comparison result, widen it by oring, and take the lanes an
        // if-branch did not with `s_andn2_b64`. The mask is an ordinary operand to all
        // three, which is exactly why the wavefront model keeps it as a value.
        "s_and_b64" | "s_or_b64" | "s_andn2_b64" => scalar_logic(model, instruction, name),

        // The 32-bit scalar arithmetic and logic. Every one of these writes the
        // condition code as well as its destination.
        "s_add_i32" | "s_sub_i32" | "s_and_b32" | "s_or_b32" | "s_xor_b32" => {
            scalar_integer(model, instruction, name)
        }

        // The compact scalar form: a destination and a sixteen-bit immediate.
        "s_movk_i32" | "s_cmpk_eq_i32" | "s_cmpk_lg_i32" | "s_addk_i32" | "s_mulk_i32" => {
            scalar_immediate(model, instruction, name)
        }

        // The scalar compares, which write the condition code the `scc` branches read.
        // No destination operand at all - the result is hidden state.
        "s_cmp_eq_i32" | "s_cmp_lg_i32" | "s_cmp_gt_i32" | "s_cmp_ge_i32" | "s_cmp_lt_i32"
        | "s_cmp_le_i32" => scalar_compare(model, instruction, name),

        // Comparisons, which is where a mask comes from. Every lane compares, and the
        // sixty-four answers become one value the shader can then and into `exec`.
        "v_cmp_lt_f32_e32" | "v_cmp_eq_f32_e32" | "v_cmp_gt_f32_e32" | "v_cmp_lt_u32_e32" => {
            compare(model, instruction, name)
        }

        // Where a lane learns its own index. There is no "lane id" instruction and the
        // value is not handed to the shader; a shader that needs to know which lane it
        // is counts the mask bits below itself.
        "v_mbcnt_lo_u32_b32" | "v_mbcnt_hi_u32_b32" => mask_bit_count(model, instruction, name),

        // The long-form vector ALU. Same arithmetic as the short forms, plus per-source
        // negate and absolute flags living in bits neither the operand layout nor the
        // encoding table describes - read separately, and refused where not implemented
        // (D127).
        "v_cndmask_b32_e64" | "v_add_f32_e64" | "v_sub_f32_e64" | "v_subrev_f32_e64"
        | "v_mul_f32_e64" | "v_fma_f32" | "v_div_fixup_f32" | "v_div_fmas_f32" => {
            long_form_arithmetic(model, instruction, name)
        }

        // The carry-producing arithmetic, which writes a second destination: one bit per
        // lane saying whether that lane carried. Sixty-four-bit address arithmetic is
        // built out of these, so they are ordinary rather than exotic.
        "v_div_scale_f32" => division_scale(model, instruction),
        "v_add_co_u32" | "v_sub_co_u32" | "v_add_co_ci_u32_e64" | "v_add_co_ci_u32_e32" => {
            carry_arithmetic(model, instruction, name)
        }

        // v_rcp_f32: a reciprocal, per lane.
        //
        // The guest's is an *approximation* with a documented accuracy of roughly one
        // part in a million; this emits an exact division. The difference is real and it
        // is the right way round - being more accurate than the hardware cannot turn a
        // correct frame into a wrong one, where being less accurate can. Worth knowing
        // before a bit-exact framebuffer comparison is trusted to the last bit.
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
        // Split out for the same reason the memory and scalar instructions were - the
        // combined match outgrew a screen.
        "v_add_f32_e32" | "v_sub_f32_e32" | "v_subrev_f32_e32" | "v_mul_f32_e32"
        | "v_lshlrev_b32_e32" | "v_lshrrev_b32_e32" | "v_add_nc_u32_e32" | "v_fmac_f32_e32" => {
            short_form_arithmetic(model, instruction, name)
        }

        // Float minimum and maximum, which the core opcode set has no instruction for: emitted
        // as GLSL.std.450 FMax/FMin extended instructions (worklog 523). They appear in nearly
        // every real shader - clamp and saturate are a min of a max - so they are the first
        // extended pair wired up now that the emitter can import the set.
        "v_max_f32_e32" | "v_min_f32_e32" => float_min_max(model, instruction, name),

        // Two floats packed into one register as halves - how a pixel shader prepares the
        // compressed export an 8_8_8_8 target takes on this part (worklog 820).
        "v_cvt_pkrtz_f16_f32_e32" => pack_halves(model, instruction),

        // Unary vector float ALU and transcendentals: square root, reciprocal square root,
        // sin, cos, base-2 exp, and base-2 log (worklog 525).
        "v_sqrt_f32_e32" | "v_rsq_f32_e32" | "v_sin_f32_e32" | "v_cos_f32_e32"
        | "v_exp_f32_e32" | "v_log_f32_e32" => float_unary(model, instruction, name),

        // Anything that reaches guest memory. Split out because the two halves ask
        // different questions - one is about registers and arithmetic, the other about
        // addresses - and because the combined match outgrew what fits on a screen.
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

        // The local data share: storage the lanes of a wavefront share.
        // Untyped buffer access at any width; `buffer_memory` reads the width from the name and
        // refuses one it does not know, the same shape as the `tbuffer_` guard below. `tbuffer_`
        // does not start with `buffer_`, so the two families stay distinct.
        name if name.starts_with("buffer_") => buffer_memory(model, instruction, name),
        name if name.starts_with("tbuffer_") => typed_buffer_memory(model, instruction, name),
        "ds_write_b32" | "ds_read_b32" => local_share(model, instruction, name),

        // A texture sample. The `lz` form was the last thing between the GL cube's textured
        // pixel shader and a translation; the plain form is the same instruction letting the
        // implementation pick a level. Split out because everything hard about either is on the
        // other side of the instruction rather than in it (D690).
        "image_sample_lz" | "image_sample" | "image_sample_l" | "image_load" => {
            image_sample(model, instruction, name)
        }
        // A store, which is the one image instruction that needs a binding of its own: nothing
        // writes to a sampled image (D692).
        "image_store" => image_store(model, instruction, name),

        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this instruction",
        }),
    }
}

/// Translates `v_mbcnt_lo_u32_b32` / `v_mbcnt_hi_u32_b32`.
///
/// Counts the set bits of the mask *strictly below* this lane and adds the second
/// source. `lo` looks at bits 0-31 and `hi` at 32-63, so the idiom that yields a lane
/// index is the pair run in sequence: `lo` with an all-ones mask, then `hi` feeding the
/// first result in as its addend.
///
/// # Why this matters more than one instruction should
///
/// It is the only way a shader learns which lane it is. Without it every lane reads the
/// same registers, so every comparison answers the same in all sixty-four and every mask
/// is all-ones or all-zero - which makes the entire masking apparatus untestable against
/// anything but its own extremes.
///
/// # The boundary is the part to get right
///
/// *Strictly* below. Including this lane's own bit shifts every index by one wherever
/// the lane is active, and leaves it correct wherever the lane is not - so a test run
/// with the mask all ones catches it and a test run with the mask empty does not.
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

        // Which bits of *this* half sit below this lane. A lane in the other half sees
        // the whole of this one; a lane in this half sees only what precedes it.
        let below = match (high_half, lane < 32) {
            // `lo` for a lane in the high half, or `hi` for a lane in the low half:
            // the halves do not meet, so nothing or everything.
            (false, false) => u32::MAX,
            (true, true) => 0,
            _ => {
                let within = lane % 32;
                // Shifting a 32-bit value by 32 is undefined, and lane 0 of the low half
                // has nothing below it - so the identity is spelled out rather than
                // computed.
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
///
/// The VOP3 opcode field is ten bits wide, so these sit far above the three-digit
/// numbers the shorter families use - written out rather than guessed at from the
/// mnemonic table's ordering.
const MBCNT_HI: &str = "v_mbcnt_hi_u32_b32";

/// Translates the short-form vector ALU instructions.
///
/// No source modifiers here - the short encoding has nowhere to put them, which is one
/// of the reasons the long form exists.
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
            // Integer: address arithmetic, and the shift whose amount comes *first*.
            // Reading a shift in written order computes `2 << index` where `index << 2`
            // was meant, and those agree for lane two and no other.
            "v_add_nc_u32_e32" => model.binary(op::IADD, lhs, rhs),
            "v_lshlrev_b32_e32" => model.binary(op::SHIFT_LEFT_LOGICAL, rhs, lhs),
            // Logical, not arithmetic: the guest has a separate `v_ashrrev_i32` for the
            // sign-propagating one, so reading this as arithmetic would be right for every
            // address and wrong for every negative value.
            "v_lshrrev_b32_e32" => model.binary(op::SHIFT_RIGHT_LOGICAL, rhs, lhs),
            // Float. `v_subrev_f32` reverses its operands - the name says so and the
            // encoding does not, so the two agree only when the operands are equal.
            // Accumulates *into* its destination, so the destination is a source as
            // well. Read through the ordinary source path rather than a new accessor -
            // the destination operand already names a vector register, which is exactly
            // what that path takes.
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

/// `v_max_f32` / `v_min_f32`: the per-lane float maximum/minimum, emitted as the GLSL.std.450
/// extended instructions the core opcode set has no equivalent for.
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
/// The first source lands in bits 0-15 and the second in bits 16-31 - the order ACO relies on
/// when it packs `(r, g)` and `(b, a)` for an FP16 colour export
/// (`aco_select_ps_epilog.cpp:170-178` in the collection's Mesa tree).
///
/// **The rounding is assumed, and named.** The instruction rounds toward zero; the narrowing here
/// is `OpFConvert`, whose rounding the device chooses and which is round-to-nearest-even on every
/// driver this has run on. The two differ by at most one unit in the last place of a half, which is
/// far below an eight-bit colour channel's resolution but is not nothing, so it is not claimed to
/// be exact.
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

/// Unary vector float ALU and transcendental operations: emitted as GLSL.std.450 extended
/// instructions reached through `OpExtInst`.
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
    // The extended instruction, and whether the source is an angle in *revolutions*.
    // v_sin_f32/v_cos_f32 are the hardware's turn-based trig: the instruction computes the
    // sine or cosine of `2*pi * x`, which is why a shader divides a radian angle by 2*pi
    // before calling one. GLSL.std.450 Sin/Cos take radians, so the argument has to be scaled
    // back up by 2*pi to reproduce what the instruction means - a plain Sin(x) is wrong by
    // that factor, and wrong in a way sin(0)/cos(0) cannot see.
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

/// The bit pattern of `2*pi` as an f32 (`6.2831855`).
///
/// The scale from a turn-based angle to radians: v_sin_f32/v_cos_f32 read revolutions and
/// compute the trig of `2*pi * x`, so translating them onto the radian-based GLSL.std.450
/// Sin/Cos means multiplying the argument by this first.
const TWO_PI_F32: u32 = 0x40C9_0FDB;

/// `v_subrev_f32_e64`, which takes its operands the other way round.
const REVERSE_SUBTRACT: &str = "v_subrev_f32_e64";

/// `v_cndmask_b32`, which picks per lane from a 64-bit mask.
const CNDMASK: &str = "v_cndmask_b32_e64";

/// Where a second, scalar destination sits when an instruction has one.
///
/// Bits 8 to 14 of the first word. The *other* sub-encoding puts per-source
/// absolute-value flags in the same place, which is the whole difficulty.
const SCALAR_DESTINATION: (u32, u32) = (0, 8);

/// Whether an instruction carries a second, scalar destination.
///
/// # The problem this answers
///
/// The long-form vector ALU has two sub-encodings. One puts per-source absolute-value
/// flags in bits 8 to 14 of the first word; the other puts a **scalar destination** there,
/// a carry-out or a flag saying an operand was pre-scaled. Nothing in the instruction says
/// which. Read the wrong way, `vcc` as a carry destination - code 106, or 1101010 -
/// presents as "the second source is an absolute value", and an integer addition silently
/// loses the sign of an operand.
///
/// # Derived, where it used to be listed
///
/// This was a hand-written list of names, and its own decision entry said what was wrong
/// with that: nothing enforced the pairing, so an opcode added to [`SUPPORTED`] without
/// also being added there would read its modifiers from the wrong bits, quietly, and only
/// for the operands that happened to have the sign bit set.
///
/// It never needed listing. The operand solver already probes each opcode and records
/// where its operands are, and the two sub-encodings differ in exactly that: one has an
/// operand in those bits and the other does not. The answer was in the data all along,
/// one table over.
///
/// So an opcode classifies itself, from evidence about that opcode, and a newly supported
/// instruction cannot be missed - there is nothing left to remember to update.
///
/// # Why an unsolved opcode can safely answer "no"
///
/// It never gets here. An instruction whose operand layout is unknown is refused before
/// translation begins, so this is only ever asked about opcodes the solver has data for.
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
/// `vdst = src0 op src1`, and one bit per lane into the scalar destination saying whether
/// that lane carried or borrowed. The carry is what makes sixty-four-bit address
/// arithmetic work, so dropping it would produce addresses that are right below four
/// gigabytes and wrong above.
///
/// Needs a model with lanes, because the second destination is a per-lane mask.
fn carry_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    // No absolute-value flags in this sub-encoding; a negate would be meaningless on
    // integer arithmetic and is refused rather than ignored.
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
    // `null` means the carry is not wanted, which is a different thing from an ordinary
    // register pair below: one declines the result and the other names a place this translator
    // cannot write. Dropping the first is what the guest asked for; dropping the second would
    // be a carry silently going nowhere.
    let mask_name = match scalar_destination {
        _ if discards(scalar_destination) => None,
        Operand::Named(name) => Some(lane_mask_name(name).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a carry destination this translator does not know",
        })?),
        // An ordinary register pair as the carry destination is legal and needs the
        // general per-register write the lane-mask methods do not offer. Refused rather
        // than dropped: a carry silently going nowhere is an address that is wrong only
        // sometimes.
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
    // Every lane, not only the running ones: the carry is a mask, and what it holds for an
    // inactive lane is this translation's existing answer, which skipping would change.
    for lane in 0..model.lanes() {
        let left = model.read_source(instruction, first, lane)?;
        let right = model.read_source(instruction, second, lane)?;

        let (value, carried) = match name {
            // Unsigned add: it carried exactly when the wrapped sum came out below one
            // of the operands. Comparing against an operand rather than recomputing is
            // what makes this exact at the wrap point.
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

    // The carry, where the instruction asked for one. Computed either way - it falls out of the
    // same arithmetic as the sum - so only the write is conditional.
    if let Some(mask_name) = mask_name {
        model.write_lane_mask(mask_name, mask.0, mask.1)?;
    }
    model.count();
    Ok(())
}

/// One lane of `v_addc_co_u32`: the two sources plus the carry-in bit.
///
/// **Two additions and two carry tests.** A single test would miss the case where the
/// first addition did not carry and adding the carry-in did - which happens whenever the
/// sources sum to exactly the largest representable value, and is the one input a
/// simpler implementation gets wrong.
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
/// Two spellings of one instruction: the long form names its carry registers and the short
/// form leaves them implicit at `vcc`, which the operand table records as implicit operands
/// in the same positions. So the layout below indexes the same way for both, and the only
/// thing that had to change to accept the short form was this question - which is the answer
/// to "is the long form's translation right for the short one", and it is.
///
/// The short form is what a real shader contains: it is how the console-run vertex program
/// forms a 64-bit address, `v_add_co_ci_u32 v19, vcc, s3, v1, vcc` (orbistoun worklog 545).
fn is_add_with_carry_in(name: &str) -> bool {
    matches!(name, "v_add_co_ci_u32_e64" | "v_add_co_ci_u32_e32")
}

/// Translates the long-form vector ALU instructions.
///
/// One arm for the two-, three- and four-operand shapes, because what separates them is
/// only how many sources they read - and the modifiers apply the same way to each.
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
    // refused by name everywhere else (worklog 834).
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
/// operation that is not a comparison, and - for a conversion `v_cvt_<to>_<from>` - one converting
/// *to* `f32`. `v_cvt_i32_f32` ends in `_f32` and produces an integer; `v_cvt_pkrtz_f16_f32` packs
/// halves (worklog 834).
fn result_is_f32(name: &str) -> bool {
    let base = name.strip_suffix("_e64").unwrap_or(name);
    if base.starts_with("v_cmp") || !base.ends_with("_f32") {
        return false;
    }
    base.strip_prefix("v_cvt_")
        .is_none_or(|conversion| conversion.starts_with("f32_"))
}

/// The output clamp on a 32-bit float result: `[0, 1]`, and a NaN either zero (`DX10_CLAMP` set) or
/// passed through (clear) - the rule the stage's `RSRC1` names (worklog 834).
///
/// `FMin`/`FMax` alone are undefined on a NaN in GLSL.std.450, so the NaN takes its own select and
/// the min/max only ever sees an ordered value.
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
/// # What this is
///
/// The last step of the division sequence. Something else has already computed a
/// quotient by reciprocal and Newton-Raphson refinement, which is right for ordinary
/// values and says nothing useful about zero over zero, infinity over infinity, a signed
/// zero, or a NaN. This replaces the quotient wherever one of those applies.
///
/// The sources are, in order, the **quotient**, the **denominator** and the
/// **numerator** - not the order a reader expects, and the reference states it
/// explicitly because guessing it produces a division that is correct except for its
/// sign.
///
/// # Where the numbers come from
///
/// The decision tree is the pseudocode in the instruction-set reference for this
/// generation, followed branch for branch and in the same order - the order is load
/// bearing, because several conditions overlap and only the first match applies.
///
/// Two terms in that pseudocode are named rather than given as bit patterns:
/// `underflow` and `overflow`. Those are **IEEE-754** terms and are read as IEEE-754
/// defines them under round-to-nearest: a magnitude below half the smallest subnormal
/// rounds to a signed zero, and one above the largest finite value becomes a signed
/// infinity. The threshold the reference uses for the underflow branch - an exponent
/// difference below -150 - is exactly the point where that rounding applies, which is
/// the check that it has been read correctly rather than assumed.
///
/// `Quiet(x)` is likewise IEEE-754: set the most significant mantissa bit, which turns a
/// signalling NaN into a quiet one and leaves a quiet one alone.
///
/// # Built backwards
///
/// The tree is built from its default case upwards, each condition overriding the ones
/// below it, so the *last* select applied is the *first* branch of the pseudocode. That
/// is the ordering the reference specifies, and writing it in reading order would invert
/// every priority.
fn division_fixup<M: Model + ?Sized>(
    model: &mut M,
    quotient: Id,
    denominator: Id,
    numerator: Id,
) -> Id {
    let magnitude = model.constant(0x7FFF_FFFF);
    let sign_bit = model.constant(0x8000_0000);
    let infinity = model.constant(0x7F80_0000);
    // The reference gives this bit pattern literally for both `0/0` and `inf/inf`. It is
    // a negative quiet NaN; the sign is part of what it specifies.
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

    // The default: the computed quotient, with the sign the operands imply rather than
    // whichever sign the reciprocal sequence happened to produce.
    let magnitude_of_quotient = model.binary(op::BITWISE_AND, quotient, magnitude);
    let mut result = model.binary(op::BITWISE_OR, sign_out, magnitude_of_quotient);

    // exponent(denominator) == 255. Unreachable in practice - that is an infinity or a
    // NaN and both are handled above it - and translated anyway, because a branch the
    // reference states is not this translator's to decide is dead.
    let shift = model.constant(23);
    let exponent_mask = model.constant(0xFF);
    let shifted_denominator = model.binary(op::SHIFT_RIGHT_LOGICAL, denominator, shift);
    let denominator_exponent = model.binary(op::BITWISE_AND, shifted_denominator, exponent_mask);
    let shifted_numerator = model.binary(op::SHIFT_RIGHT_LOGICAL, numerator, shift);
    let numerator_exponent = model.binary(op::BITWISE_AND, shifted_numerator, exponent_mask);
    let all_ones = model.constant(255);
    let denominator_saturated = model.compare(op::IEQUAL, denominator_exponent, all_ones);
    result = pick(model, denominator_saturated, signed_infinity, result);

    // exponent(numerator) - exponent(denominator) < -150: the quotient is below half the
    // smallest subnormal, so it rounds to a signed zero. Compared as signed, on biased
    // exponents - the bias cancels in a difference.
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

    // A NaN operand propagates, quietened. The denominator is tested first so that the
    // numerator's select, applied last, wins - which is the order the reference gives.
    let quiet_denominator = model.binary(op::BITWISE_OR, denominator, quiet_bit);
    result = pick(model, denominator_nan, quiet_denominator, result);
    let quiet_numerator = model.binary(op::BITWISE_OR, numerator, quiet_bit);
    pick(model, numerator_nan, quiet_numerator, result)
}

/// Translates `v_div_fmas_f32`: a multiply-add that scales its result when the condition
/// mask says the operands were pre-scaled.
///
/// The middle of the division sequence. `v_div_scale_f32` may multiply an operand by a
/// power of two to keep the reciprocal that follows out of the subnormal range, and
/// records in the condition mask that it did; this undoes that, per lane, by the
/// documented factor of two to the thirty-second.
///
/// Reads the condition mask **implicitly**: it is not one of the instruction's operands,
/// which is why this needs a model with lanes rather than being another arm of the
/// arithmetic that surrounds it.
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

    // Two to the thirty-second, exactly representable, so the scaling is exact rather
    // than nearly so.
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

/// The condition mask, which the division sequence passes its scaling flag through.
///
/// Spelled as the low half, because that is how a 64-bit mask is named throughout - the
/// pair is identified by the register its low word lives in.
const CONDITION_MASK: &str = VCC_LOW_HALF;

/// The negation of a boolean.
fn negate<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, result.0, value.0]);
    result
}

/// Whether a float's exponent field is all zeroes - so it is a subnormal or a zero.
///
/// # Why this one test answers a question about subnormals on any device
///
/// The instruction below has to know whether a computed quotient came out *subnormal*.
/// The obvious way to ask - divide, then check the result is subnormal - looks like it
/// depends on the host preserving subnormals, and Vulkan lets an implementation flush
/// them to zero. The device this was written on does exactly that: it reports no support
/// for preserving 32-bit subnormals at all, so asking for `SPV_KHR_float_controls` would
/// not have helped, it would have made the module unloadable.
///
/// It does not matter, because a flushed subnormal and a preserved one are the same
/// answer here. Both a zero and a subnormal have an all-zero exponent field, and in
/// every place this is used the true result **cannot be zero**: it is a reciprocal of a
/// finite non-zero value, or a quotient with a non-zero numerator, and those cases are
/// excluded by branches that run before it. So an all-zero exponent means "the true
/// value was subnormal" on a preserving device and on a flushing one alike.
///
/// Testing the bits rather than the arithmetic is what makes that work: a comparison
/// against the smallest normal would be at the mercy of how the comparison itself
/// handles a flushed operand.
fn exponent_is_zero<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let shift = model.constant(23);
    let mask = model.constant(0xFF);
    let zero = model.constant(0);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, value, shift);
    let exponent = model.binary(op::BITWISE_AND, shifted, mask);
    model.compare(op::IEQUAL, exponent, zero)
}

/// Translates `v_div_scale_f32`: the pre-scale that keeps a division out of the
/// subnormal range.
///
/// # What it does
///
/// The first step of the division sequence. Given a numerator and a denominator, it
/// multiplies **one** of them by a power of two so that the reciprocal the hardware is
/// about to take does not land among the subnormals, where it would lose precision. It
/// records in the condition mask whether it scaled, so `v_div_fmas_f32` can undo it.
///
/// Which operand gets scaled is the caller's choice: `S0` is the one to scale and must
/// be the same value as either the denominator or the numerator. Several branches only
/// scale when `S0` is the operand *that branch* cares about, and pass it through
/// untouched otherwise - which is how one instruction serves both halves of the
/// sequence.
///
/// # Where the numbers come from
///
/// The decision tree, the exponent thresholds and both scaling factors are the
/// pseudocode in the instruction-set reference for this generation, branch for branch
/// and in the same order. The order is load bearing: several conditions overlap and only
/// the first match applies, so the tree is built from its default upwards and the last
/// select applied is the first branch of the pseudocode.
///
/// The reference writes `D.f = NAN` for the zero-operand case without giving a bit
/// pattern - unlike the fixup, where it gives one - so the canonical quiet NaN is used.
/// Nothing downstream can observe the difference: this result feeds the reciprocal and
/// then `v_div_fixup_f32`, which replaces any NaN with a quietened operand of its own.
#[allow(
    clippy::too_many_lines,
    reason = "one algorithm - the division pre-scale's subnormal cases are a single decision table, and splitting it to satisfy a line count would put half a rule in each half"
)]
fn division_scale<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read(instruction, true)?;
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
    // **`null` means the flag is not wanted**, and a shader that only needs the scaled operand
    // says so there. Writing it somewhere anyway would invent a destination the guest declined
    // to name; refusing would refuse a shader that is asking for less, not more.
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
    let sources: Vec<Operand> = operands[2..].to_vec();
    let [first, second, third] = sources.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the division pre-scale does not have three sources",
        });
    };

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
    // The flag is carried as a word so it can go through the same selects as the value,
    // and is turned back into a bit at the end. Selecting between booleans would need a
    // second select shape for no gain.
    let truth = model.constant(1);
    let falsehood = zero;

    // Started from zero rather than from the mask's current contents: the reference
    // opens with `VCC = 0` and every lane is then written, so anything already there is
    // overwritten in full.
    let mut halves = (zero, zero);

    // Every lane: this writes a mask as well as values (see `carry_arithmetic`).
    for lane in 0..model.lanes() {
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
        halves = model.set_lane_bit(halves, lane, set);
        model.write_vector_lane(register, lane, value);
    }

    // The flag, where the instruction asked for one. It is still computed either way: the value
    // and the flag come out of the same selects, so there is nothing to skip and nothing gained
    // by skipping it - only the write is conditional.
    if let Some(mask_name) = mask_name {
        model.write_lane_mask(mask_name, halves.0, halves.1)?;
    }
    model.count();
    Ok(())
}

/// Writes the low half of a lane mask, leaving the upper half as it was.
///
/// # Why a 32-bit write to a mask is not a scalar write
///
/// A shader compiled for 32 lanes manipulates its masks with the **32-bit** scalar
/// instructions - `s_mov_b32 exec_lo, ...`, `s_and_b32 exec_lo, ...` - because its mask
/// is thirty-two bits and fits in one register. The 64-bit forms a 64-lane shader uses
/// would be meaningless there.
///
/// Translated as an ordinary scalar write, those land in the register *file* rather than
/// in the model's mask, and every lane stays active for the whole shader: no branch
/// narrows, no comparison excludes anything, and the result is a shader that runs and is
/// not the one the guest wrote. Which is why this exists rather than the destination
/// simply being allowed through.
///
/// The upper half is read back and rewritten unchanged. A 64-lane shader is allowed to
/// touch `exec_lo` alone, and dropping the other thirty-two lanes on the floor when it
/// does would be a much stranger bug than the one this fixes.
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
/// The one instruction here that is not float arithmetic. Its sources are raw bits and
/// must not be bitcast, and its third operand is a sixty-four-bit mask rather than a
/// value - so it needs a model with lanes, and the per-lane model refuses it.
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
        // Set picks the *second* source. The other way round is a shader that takes the
        // wrong branch of every ternary the compiler wrote.
        b.function(op::SELECT, &[u32_type.0, value.0, bit.0, set.0, clear.0]);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// Applies a source's negate and absolute flags, in that order.
///
/// Absolute first, then negate - so `-|x|` is expressible and `|-x|` is not, which is
/// what the encoding means. The other order makes `-|x|` come out as `|x|` for every
/// negative input and agree for every positive one.
///
/// Both act on the bit pattern rather than as floating-point operations. Negation has a
/// core opcode; absolute value is only in an extended instruction set this crate does
/// not import, and clearing the sign bit is exactly what it would do.
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
            // The reverse-subtract takes its operands the other way round, as the short
            // form does. Named rather than numbered: the two sat one apart in the
            // supported list for a while because the numbers were assumed from the short
            // form's ordering, and every long-form reverse-subtract was wrong.
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
        // Fused multiply-add, a*b+c. The previous generation also had an unfused
        // `v_mad_f32`; this one does not, so the distinction the translator used to
        // have to make no longer arises here (D139).
        //
        // Old comment, kept because the reasoning still applies to the one that remains:
        // both were a*b+c and the guest distinguished them
        // by whether the multiply rounds before the add. SPIR-V's core multiply and add
        // always round, so both translate the same way and the fused one is the less
        // faithful of the two translations. There is an extended-instruction spelling
        // for a genuine fused multiply-add; reaching for it is the fix if a framebuffer
        // comparison ever cares.
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
/// # The condition code is half of what these do
///
/// Every one writes it, and what it means differs: the logical operations set it to
/// whether the result is non-zero, and the arithmetic ones to whether the *signed*
/// addition overflowed. Translating the destination and dropping the code produces a
/// shader whose next branch reads whatever the previous compare left - which runs, and
/// takes the wrong path.
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
            // Signed overflow: the two operands agreed in sign and the result does not.
            // Expressed in bits rather than compared, because there is no core opcode
            // for "did that overflow" and the bit form is exact.
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

    // A mask destination goes to the mask, not to the register file. `s_and_b32 exec_lo,
    // exec_lo, s2` is how a 32-lane shader narrows its execution mask, and writing it to
    // a scalar register instead would leave every lane active for the whole shader.
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
/// The immediate is **signed**, and the decoder reports the field as encoded so it keeps
/// agreeing with the reference - a disassembler prints -2 as 65534. Sign extension
/// happens here, where the instruction's meaning is known, exactly as it does for a
/// branch offset.
///
/// Two of these read their destination as well as writing it: `s_addk_i32` and
/// `s_mulk_i32` accumulate. Treating them as plain moves would leave a shader computing
/// from whatever happened to be there.
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
        // s_mulk_i32 accumulates and leaves the code alone, which is the one asymmetry
        // in this family and is documented rather than deduced.
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

/// Sign-extends a sixteen-bit immediate.
///
/// The width comes from the instruction's definition rather than the operand layout: the
/// layout records the field it observed, and how to read it is not something the bits can
/// say.
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
/// # Signed, and that is the whole difficulty
///
/// These compare **signed** integers. Comparing the same bits as unsigned agrees on
/// every pair where both are non-negative and reverses the order wherever one is not -
/// so a shader that only ever compares small positive numbers works either way, and a
/// shader that compares against -1 takes the wrong branch every time.
///
/// # No destination
///
/// The condition code is not an operand. It is one bit of state, and the fact that it is
/// invisible in the instruction is exactly why a branch on it could not be translated
/// until something set it: the code would have read zero in every shader and every `scc`
/// branch would have taken the same path, silently.
fn scalar_compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let opcode = op_for_scalar_compare(instruction, name)?;
    let (first, second) = two_operands(instruction)?;

    // Scalar, so once for the wavefront rather than once per lane. Lane zero is not a
    // choice here - a scalar instruction has no lanes, and reading a *vector* source
    // through this path would be a bug the operand check below refuses.
    let left = model.read_source(instruction, first, 0)?;
    let right = model.read_source(instruction, second, 0)?;
    let condition = model.compare(opcode, left, right);

    // Widened to a word, because the code lives in an ordinary private variable and a
    // boolean has no defined size in a storage class.
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

/// The SPIR-V opcode a scalar comparison maps to.
///
/// Every one signed. `SLESS_THAN` rather than `ULESS_THAN` is the entire content of this
/// function, and getting it wrong is invisible until a shader compares against a
/// negative number.
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
/// The destination is implicit - the 32-bit form writes the condition mask and nothing
/// else - so it arrives as a named operand carrying no bits, and the layout says so
/// rather than omitting it (D108).
///
/// The comparison itself is done on floats, which means bitcasting the registers first:
/// a register holds thirty-two bits and this instruction is the thing that decides they
/// are a float. Comparing the integers instead would order negative floats backwards and
/// agree with the float comparison on every non-negative pair - so a test using positive
/// values only would pass.
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

/// The SPIR-V opcode a comparison maps to, and whether its operands are floats.
///
/// The pair travels together because reading a register as the wrong type is silent:
/// comparing two floats as unsigned integers agrees on every non-negative pair and
/// orders negatives backwards.
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
/// Both halves independently, because the operation is bitwise and there is no carry to
/// carry. The destination may be the execution mask, which is the common case and the
/// reason these are here at all - `s_and_b64 exec, exec, s[n:n+1]` is how a shader
/// enters a conditional region.
fn scalar_logic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let opcode = op_for_logic(instruction, name)?;
    let (destination, first, second) = three_operands(instruction)?;

    let (first_low, first_high) = sixty_four_bit_source(model, instruction, first)?;
    let (second_low, second_high) = sixty_four_bit_source(model, instruction, second)?;

    // `s_andn2_b64` is "and with the complement of the second operand". Expressed as a
    // complement then an and rather than looked for as a single SPIR-V opcode, because
    // there is not one - and translating it as a plain and would silently invert the
    // sense of every else-branch.
    let (second_low, second_high) = if name == ANDN2 {
        (model.not(second_low), model.not(second_high))
    } else {
        (second_low, second_high)
    };

    let low = model.binary(opcode, first_low, second_low);
    let high = model.binary(opcode, first_high, second_high);

    // These set the condition code to whether the result is non-zero, and that was
    // missing when they were first translated. `s_and_b64 exec, exec, vcc` followed by a
    // branch on the code is how a compiler skips a block once no lane survives - so a
    // shader would have branched on whatever the *previous* compare had left there.
    // Documented behaviour in the published instruction set; nothing here can observe it
    // directly, which is exactly why it was easy to miss.
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

/// Translates a scalar move.
///
/// The two widths are here together because the difference between them is the whole
/// content: `s_mov_b64` is not two `s_mov_b32`s, and keeping them adjacent is what makes
/// that visible rather than a comment somebody has to find.
fn scalar_move<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // s_mov_b64: two consecutive registers from two consecutive sources.
        //
        // Not two `s_mov_b32`s. A constant source is *extended* to sixty-four bits
        // rather than repeated - `s_mov_b64 s[0:1], -1` sets both halves to all ones and
        // `s_mov_b64 s[0:1], 1` sets s0 to one and s1 to zero. Copying the low word into
        // both would be right for -1 and wrong for every other constant, which is the
        // sort of thing that passes the first test written for it.
        //
        // Worth the care because this is how the execution mask is set. A wrong high
        // half means the top thirty-two lanes are active when they should not be.
        "s_mov_b64" => {
            let (destination, source) = two_operands(instruction)?;
            let (low, high) = sixty_four_bit_source(model, instruction, source)?;

            // `s_mov_b64 exec, ...` is how a shader turns lanes off, and it is by far
            // the most common thing this instruction is used for. The destination
            // decodes as the mask's low half, because a sixty-four-bit operand names its
            // pair that way.
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

            // `s_mov_b32 exec_lo, ...` is how a 32-lane shader sets its execution mask,
            // exactly as `s_mov_b64 exec, ...` is for a 64-lane one.
            if let Some(mask) = mask_destination(destination) {
                write_mask_low(model, mask, value)?;
                model.count();
                return Ok(());
            }

            // `s_mov_b32 m0, sN` is how a pixel shader hands the interpolator its
            // primitive mask: the first instruction after the wait in both shaders the
            // GL cube ran on the console (orbistoun-gpu `tests/oracle_gl_cube.rs`).
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
/// The address is a **byte** address in a vector register plus a byte offset held in the
/// instruction - and that offset was invisible until it was probed for, because the
/// reference omits it when it is zero and every earlier probe used the zero form. A
/// translator built on the layout that produced would have ignored every offset a
/// compiler emitted and read the wrong word.
///
/// Reads are unmasked and writes are not, the same asymmetry guest memory has: an
/// inactive lane must not write, because an active one will read what it would otherwise
/// have left behind.
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
    // A read names its destination first and its address second; a write names its
    // address first and its data second. They do not share a layout, and assuming they
    // did is a mistake this crate has now made twice.
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
/// `s_load_dword` is one, `s_load_dwordx2` is two, and so on; the flat accesses spell it
/// the same way. The opcodes happen to run consecutively on one generation and there is
/// no reason for that to hold on another, so the suffix is read instead - it is the
/// instruction's own statement of its width.
///
/// An unparseable suffix answers one rather than erroring. One word is the narrowest
/// access, so a name this does not understand under-reads: it returns less data than the
/// instruction asked for, which a test sees. Guessing wide would write registers the
/// instruction never named.
fn access_words(name: &str) -> u32 {
    match name.rsplit_once("dwordx") {
        Some((_, count)) => count.parse().unwrap_or(1),
        None => 1,
    }
}

/// A buffer resource constant, read out of four consecutive scalar registers.
///
/// The fields are those the addressing needs; the rest of the descriptor - channel
/// selects, data format - describes a *conversion* that the untyped accesses do not do.
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
/// # Layout
///
/// From the descriptor table in the instruction-set reference for this generation:
/// base address in bits 47:0, stride in 61:48, record count in 95:64, swizzle enable at
/// 63, add-thread-id at 119 and the out-of-bounds mode in 125:124. Those land across four
/// registers as the shifts below.
///
/// # Why the descriptor is *read*, not folded away
///
/// It lives in registers, so every field is a value the shader computed or loaded rather
/// than a constant this translator can see. The addressing is therefore emitted as
/// arithmetic, not evaluated here.
fn read_buffer_resource<M: Model + ?Sized>(model: &mut M, first: u32) -> BufferResource {
    let base = model.read_scalar(first);
    let second = model.read_scalar(first + 1);
    let records = model.read_scalar(first + 2);
    let flags = model.read_scalar(first + 3);

    let sixteen = model.constant(16);
    let stride_mask = model.constant(0x3FFF);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, second, sixteen);
    let stride = model.binary(op::BITWISE_AND, shifted, stride_mask);

    // Swizzled addressing interleaves records by an element size this does not model, and
    // add-thread-id folds the lane number into the index. Both change *where* an access
    // lands, so producing the unswizzled address for them would read real-looking data
    // from the wrong place.
    //
    // A translated shader cannot refuse at run time, so the refusal is expressed the only
    // way it can be: the access is forced out of bounds, which the hardware defines as
    // reading zero and dropping writes. A buffer that reads zero is visibly, consistently
    // wrong; a buffer read from the wrong offset looks like data.
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
/// ```text
/// ADDR = Base + baseOffset + Inst_offset + Voffset + Stride * (Vindex + TID)
/// ```
///
/// where `baseOffset` is the scalar offset operand, `Inst_offset` the literal in the
/// instruction, `Voffset` a vector register present when the instruction sets `offen`,
/// and `Vindex` one present when it sets `idxen`. The thread-id term is excluded above,
/// with the descriptors that ask for it.
///
/// **Only the low thirty-two bits are computed.** The base is a 48-bit address and guest
/// memory here is a small window indexed directly (D101), so the upper bits have nowhere
/// to go - the same simplification the flat accesses already make. It is stated rather
/// than hidden because it is the thing that has to change when the address space is real.
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
/// The reference defines four modes, selected by two bits of the descriptor, and they
/// are all evaluated because the selector is a *runtime* value - the descriptor lives in
/// registers, so which mode applies is not known here.
///
/// | mode | check | for |
/// |---|---|---|
/// | 0 | index >= records, or offset >= stride | structured buffers |
/// | 1 | index >= records | raw buffers |
/// | 2 | records == 0 | unchecked |
/// | 3 | offset + payload > records | raw, unswizzled |
///
/// **Mode 3's payload is read as bytes.** The reference calls it "the number of dwords
/// the instruction transfers" while every other term in that comparison is a byte count,
/// and a raw buffer of N bytes accepts a four-byte read at offset `off` exactly when
/// `off + 4 <= N`. Read as dwords the comparison mixes units and is wrong by a factor of
/// four at the boundary; read as bytes it is the ordinary range check. Noted because it
/// is the one place here where the reference is loose.
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

    // Mode three is the default and the others override it, so the checks are applied in
    // descending order and the last select wins. `three` is not compared against at all:
    // it is what remains when none of the others matched.
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
/// A table rather than a match arm for the reasons in
/// [`orbistoun_shader::formats`]; read through a lock here because parsing it per
/// instruction would dominate the cost of translating a shader that uses one.
static BUFFER_FORMATS: std::sync::OnceLock<orbistoun_shader::FormatTable> =
    std::sync::OnceLock::new();

/// How many components a typed access moves, from its name.
///
/// The channel letters are the whole suffix - `_x` is one, `_xyzw` is four - and they
/// count *registers*, which is a different question from how many components the format
/// describes. The two are allowed to disagree and this reads only the first.
fn typed_channels(name: &str) -> Option<u32> {
    let suffix = name.rsplit_once("_format_")?.1;
    let count = suffix.len();
    // Contiguous from `x`, so `xz` is not a thing and a name that looks like one is not
    // an access this understands.
    ("xyzw".starts_with(suffix) && count >= 1).then_some(count as u32)
}

/// A typed buffer access: the same descriptor and addressing as an untyped one, with a
/// format saying how to read what was fetched.
///
/// # What this translates and what it refuses
///
/// Formats whose components are all thirty-two bits wide move whole words unchanged, so the
/// work is an untyped access repeated per component and the format contributes nothing but a
/// component count. Those go through [`buffer_access`] unaltered.
///
/// A narrower component has to be extracted from within a word and converted - a normalised
/// eight-bit value becomes a float by dividing by 255, a half-precision one needs a real
/// conversion - which is [`packed_buffer_memory`]'s job, and its own comment says which kinds
/// it has so far.
///
/// **What neither can do is refused by name**, never approximated. Translating a narrow
/// component as if it were a word would produce a shader that runs, draws, and is wrong in a
/// way only a rendered frame would show, which is the failure this project is least equipped
/// to catch.
///
/// The component count must also match the channel count. The hardware permits them to
/// differ, and what it does then - padding the missing channels with zero and one, or
/// discarding the extra - is a rule this has not measured. Refusing an unmeasured rule
/// costs a shader; guessing it costs the ability to trust every shader that used one.
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
        // Either the explicitly invalid code or a reserved one. Both mean the shader is
        // wrong, and saying so beats picking a neighbouring format that would render.
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

    // A plain-word format moves whole dwords and goes straight through the untyped path. A
    // narrow format packs its components inside words and has to unpack them, which is a
    // different shape of access - `packed_buffer_memory` handles the ones it can so far.
    if format.is_plain_words() {
        buffer_access(model, instruction, loading, channels)
    } else {
        packed_buffer_memory(model, instruction, format, loading)
    }
}

/// One narrow integer component pulled out of a packed word, sign-extended when `signed`.
///
/// The component occupies bits `bit..bit + width` of `packed`, with `width < 32`. Unsigned:
/// shift it down and mask to its width. Signed: shift its high bit up to bit 31, then an
/// arithmetic shift right brings it to the low `width` bits with the sign copied into the rest -
/// two shifts, no mask. Both are exact, which is what keeps the integer kinds free of the rounding
/// a real conversion has.
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

/// Extracts one component from the packed word and produces the value the register is to hold:
/// the integer itself for the integer kinds, or the bits of a float for the converting ones.
///
/// The kinds it understands are exactly the ones [`packed_buffer_memory`] admits before a lane is
/// emitted, so the `unreachable!` guards a case that cannot arise rather than one left to chance.
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
        // A UNORM component is its unsigned field over the field's maximum, landing in 0.0..=1.0.
        // The maximum is converted from the integer domain the same way the field is, rather than
        // written as a float constant, so both reach the division having travelled the same path.
        ComponentKind::Unorm => {
            let field = packed_integer_component(model, packed, bit, width, false);
            let field_f = model.unsigned_to_float_bits(field);
            let maximum = model.constant((1u32 << width) - 1);
            let maximum_f = model.unsigned_to_float_bits(maximum);
            model.f32_binary(op::FDIV, field_f, maximum_f)
        }
        // A SNORM component is its signed field over the signed maximum 2^(width-1) - 1, so it
        // spans -1.0..=1.0. The most-negative code (-2^(width-1)) over that maximum lands a hair
        // past -1.0 - -128/127 for a byte - and the reference pins it at -1.0, so the low end is
        // clamped. Selecting between -1.0's bits and the value's bits is bit-identical to
        // selecting between the two floats, so the clamp needs no float-typed select.
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
        // USCALED and SSCALED are UNORM and SNORM with the division taken out: the field's
        // value as a float, unscaled, so an eight-bit 255 arrives as 255.0 rather than 1.0.
        // They share the extraction and the convert with the normalised kinds and add nothing,
        // which is why they carry no clamp - there is no range to clamp *to*.
        ComponentKind::Uscaled => {
            let field = packed_integer_component(model, packed, bit, width, false);
            model.unsigned_to_float_bits(field)
        }
        ComponentKind::Sscaled => {
            let field = packed_integer_component(model, packed, bit, width, true);
            model.signed_to_float_bits(field)
        }
        // A float, by one of two routes. At width 16 it is an IEEE half and the hardware's own
        // `FConvert` widens it. Narrower - the 11- and 10-bit channels of a format like
        // `10_11_11` - it is not an IEEE type at all: no sign bit, a five-bit exponent biased by
        // 15, and the rest mantissa. That one is built by hand, from measured values.
        ComponentKind::Float => {
            let field = packed_integer_component(model, packed, bit, width, false);
            if width == 16 {
                model.half_to_float_bits(field)
            } else {
                model.narrow_float_to_float_bits(field, width)
            }
        }
        // The one kind left, and the only one whose conversion is not arithmetic on the field:
        // an sRGB component is its byte through a transfer curve, which is a piecewise function
        // rather than a scale. `packed_buffer_memory` refuses it before a lane is emitted.
        ComponentKind::Srgb => {
            unreachable!(
                "packed_buffer_memory admits only the integer, normalised, scaled and half kinds"
            )
        }
    }
}

/// Translates a typed buffer access whose components are packed within words, so they must be
/// unpacked and converted rather than moved whole as [`buffer_access`] moves them.
///
/// # Built up by kind, lowest risk first
///
/// A wrong conversion here does not fail - it renders the wrong thing, silently - so this grows
/// one component kind at a time, each checked on a real device before the next. What is
/// translated so far, all single-word: **`UINT`/`SINT`**, exact bit fields that shift and mask
/// (and sign-extend) without rounding; **`UNORM`**/**`SNORM`**, the field over its range into
/// 0.0..=1.0 or -1.0..=1.0 (the low end clamped); **`USCALED`/`SSCALED`**, the same field as a
/// float of the same value, which is the normalised pair with the division removed; and
/// **`FLOAT`** at width 16, a half widened by the driver's own conversion. Narrower packed
/// floats, `SRGB`, formats wider than one word, and stores are still refused - each with a
/// detail that names which, so a shader that needs one is a loud gap rather than a quiet wrong
/// render.
/// Whether a packed format is one [`packed_buffer_memory`] can translate, and its width in bits.
///
/// Split out so the refusal is one thing in one place: every kind that is admitted has a lane
/// of arithmetic below, and a kind that reaches that arithmetic without passing here is the bug
/// the `unreachable!` in [`packed_component`] exists to catch.
fn packed_format_admitted(
    instruction: &Instruction,
    format: &orbistoun_shader::BufferFormat,
    loading: bool,
) -> Result<u32, TranslateError> {
    use orbistoun_shader::ComponentKind;

    let total_bits: u32 = format.widths.iter().sum();
    // Floats are admitted at 16 bits, where they are IEEE halves the hardware widens, and at 11
    // and 10, where they are the sign-less packed floats of formats like `10_11_11` and are
    // widened by the arithmetic in `narrow_float_to_float_bits` - measured, not derived
    // (obSCEne `REQ-...b3d4`). Any other float width is still refused: nothing measured says
    // how it decodes, and every one of these has an exponent bias and a subnormal rule that a
    // wrong guess would render plausibly and silently.
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

    // **Where a component lives is arithmetic, not an assumption.** An element is a
    // little-endian byte sequence with component zero first, so the component at bit `b` is in
    // word `b / 32` at bit `b % 32`, exactly as the within-a-word case already reads it.
    //
    // What is *not* arithmetic is a component that spans a word boundary: its value would have
    // to be assembled from two reads, and which end goes where is a rule nothing here has
    // measured. No format in the table does it - every width divides the word it sits in - so
    // this refuses a case that does not arise rather than guessing at one that might.
    let mut bit = 0;
    let straddles = format.widths.iter().any(|&width| {
        let crosses = bit / 32 != (bit + width - 1) / 32;
        bit += width;
        crosses
    });

    // A **store** is admitted only for the packed 10/11-bit floats measured for it (obSCEne
    // `REQ-...2f7a`, `REQ-...9f1c`): those are exactly a 32-bit word and are packed by
    // clamp-and-truncate (`Model::float_to_narrow_float_bits`). Every other store conversion - the
    // integers, the normalised and scaled formats, the 16-bit halves - has a rounding and a
    // saturation rule nothing has measured, so writing one would be the plausible-output this file
    // refuses. Loads are unchanged.
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

/// The widest element a packed typed access can move: four components of a whole word each.
///
/// A format wider than this does not exist in the table, and one that did would not fit the
/// four registers a `_xyzw` access names.
const ELEMENT_BITS: u32 = 128;

fn packed_buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    format: &orbistoun_shader::BufferFormat,
    loading: bool,
) -> Result<(), TranslateError> {
    let total_bits = packed_format_admitted(instruction, format, loading)?;

    // The operands and addressing modifiers are read exactly as the untyped path reads them:
    // only the per-component work below differs.
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

    // The element is bounds-checked **once, whole**, and then read a word at a time. That is
    // the difference from [`buffer_access`], which checks each word separately because each is
    // an independent component: here the words are one value, and a check per word would let
    // half an element be read at the very end of a buffer.
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

        // **`widths` is listed as the format's name lists it: highest bits first.** So the last
        // entry is the component at bit 0, which is `x`, and both directions walk them in reverse.
        //
        // This was the other way round until 2026-09-16 and nothing caught it, because every
        // packed format exercised until then had equal widths - `8_8_8_8` reads the same
        // forwards or backwards. The measurement that found it is obSCEne `REQ-...b3d4`:
        // `BUF_FMT_10_11_11_FLOAT` over the word `0x00200401` returns x = 2.03125, which is an
        // **eleven**-bit channel at bit 0 (exponent 16, mantissa 1). Read the old way, x took
        // the ten-bit width the name mentions first and decoded to 2^-19 - a subnormal, off by
        // thirteen orders of magnitude, from a format whose name looked like it was being
        // honoured.
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
                // `packed_format_admitted` has already refused anything that spans a word, so the
                // component is wholly inside this one and the index cannot run off the end.
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
            // assembles that word and writes it. Split out to keep this function under the ceiling.
            packed_store(model, instruction, data, format, base, outside, lane)?;
        }
    }
    model.count();
    Ok(())
}

/// Packs a lane's channels and writes the one word a packed float store produces.
///
/// The store half of [`packed_buffer_memory`]'s per-lane body, split out so that function stays under
/// the line ceiling. The admitted store formats are exactly one word, so it assembles a single word
/// from the packed channels - each read from its register, packed by clamp-and-truncate
/// ([`Model::float_to_narrow_float_bits`]), and shifted into place - and writes it, keeping the
/// previous contents out of bounds exactly as the untyped store does.
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
/// # What is and is not translated
///
/// A raw or structured `buffer_load_dword` / `buffer_store_dword`, addressed through a
/// resource constant in four scalar registers. Swizzled buffers and the thread-id
/// addressing mode are refused - at run time, by forcing the access out of bounds, since
/// which the descriptor asks for is not known until the shader runs.
///
/// One dword and no format, which is the whole of what makes it untyped. The typed
/// accesses share the body below through [`buffer_access`]: the descriptor, the
/// addressing equation and both bounds checks are identical, and what a typed one adds is
/// a component count and a format that has to be checked first. See
/// [`typed_buffer_memory`].
fn buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    // Untyped: no format and no conversion, so the only thing the name adds beyond the single
    // form is a width. A bare `dword` moves one, `dwordx2`/`x3`/`x4` move that many consecutive
    // dwords into consecutive registers - which is exactly what `buffer_access` already does per
    // component for the typed word forms, so this is a component count and nothing else.
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
/// They differ in exactly two things - whether a format was checked, and how many
/// components move - and share the descriptor, the addressing equation and both bounds
/// checks. Splitting them would mean two copies of the addressing, which is the part
/// where being wrong is silent.
///
/// `components` are consecutive dwords at consecutive addresses, written to consecutive
/// registers. That is the whole of what a multi-channel access adds once the format has
/// been established to need no conversion.
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

        // With both modifiers the address register is a *pair*: the index first, then the
        // offset. With one it is a single register holding whichever that one selects.
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
            // Consecutive dwords. The step is added to both the address and the offset
            // the bounds check sees - checking only the first component's offset would
            // let a four-channel access at the very end of a buffer read three words
            // past it, which is precisely the case the check exists for.
            let step = model.constant(component * 4);
            let address = model.add(base, step);
            let offset = model.add(offset, step);

            // Two bounds, and they are different questions: the *buffer* says how many
            // records it has, and the window says how much guest memory this module can
            // reach at all. An access can satisfy one and not the other.
            let outside = buffer_out_of_bounds(model, &resource, flags, offset, index, 4);
            let register = register + component;

            if loading {
                // Out of range reads zero, which the reference states outright.
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
/// A store reads consecutive registers just as a load writes them. The load side steps a
/// bare number because the destination was already reduced to one; this side still holds
/// an [`Operand`], and only a vector register can be stepped - stepping a scalar or an
/// inline constant would be reading a neighbouring value as data.
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

/// Reads guest memory, answering zero for an address outside the window.
///
/// Paired with [`write_guarded`]. Between them they are the whole of what stops an
/// out-of-window access aliasing onto a real one - see [`Model::address_within_window`].
fn read_guarded<M: Model + ?Sized>(model: &mut M, address: Id) -> Id {
    let inside = model.address_within_window(address);
    let index = model.word_index(address);
    let value = model.read_memory(index);
    let zero = model.constant(0);
    pick(model, inside, value, zero)
}

/// Writes guest memory, dropping a write to an address outside the window.
///
/// Expressed by writing back what was already there rather than by branching, for the
/// same reason the masked writes are: a branch here is divergent control flow around a
/// store.
fn write_guarded<M: Model + ?Sized>(model: &mut M, address: Id, value: Id, lane: u32) {
    let inside = model.address_within_window(address);
    let index = model.word_index(address);
    let previous = model.read_memory(index);
    let kept = pick(model, inside, value, previous);
    model.write_memory(index, kept, lane);
}

/// Translates a texture sample at level zero.
///
/// # What the instruction says
///
/// Five operands, solved from assembled probes like every other layout here: a destination
/// register, the register pair holding the coordinate, the first register of an eight-register
/// image descriptor, the first of a four-register sampler descriptor, and a four-bit mask
/// saying which components come back.
///
/// # What becomes of the two descriptors
///
/// Nothing, and D690 is the argument for that. They describe a surface at a guest address that
/// no host image stands behind, so the sample reads the one texture the pipeline bound and
/// [`Model::sampled_image`] refuses a module that names a second. The registers are still read:
/// their **numbers** are what that refusal is built on.
///
/// # Level zero, named
///
/// The `lz` in the guest's mnemonic is "level zero", stated rather than derived, so this emits
/// the explicit form with a level of zero rather than the implicit one. The two are different
/// instructions with different operands, and only one of them is available to a stage that has
/// no derivatives to choose a level from.
///
/// The zero is a bitcast of the integer zero rather than a float constant, because the constant
/// pool here is typed as registers are - every value in it is a word. Bitcasting one is what
/// every other float in this translation is, so a second kind of constant would be a second
/// thing to keep right.
/// What a texture access's operands say, once they have been checked.
///
/// Register numbers rather than [`Operand`]s, because every one of them has been through a
/// bounds check by the time this exists - which is the point of separating the reading from the
/// emitting.
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
/// Two for a coordinate, and **three** where the level is one of them. Measured rather than
/// assumed: the assembler refuses an address operand whose register count does not match the
/// instruction's dimensionality field, so a levelled two-dimensional sample takes three and a
/// level-zero one takes two, and it says so by refusing the wrong count (worklog 577).
const fn address_registers(levelled: bool) -> u32 {
    if levelled { 3 } else { 2 }
}

/// Reads a texture access's operands, refusing anything it cannot use.
///
/// **The operand at index three is not the same field in both instructions**, which is the only
/// awkward thing here: a sample names a sampler there and a fetch's mask is there instead. The
/// mnemonic is what says which, and the solved layouts agree with it - a sample has five
/// operands and a fetch has four.
fn image_access(instruction: &Instruction, name: &str) -> Result<ImageAccess, TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };

    // **Two dimensions, checked rather than assumed.** Every image translation here reads two
    // coordinate registers; a one- or three-dimensional image has a different count, and one
    // read as two would sample somewhere else entirely and draw a frame that looks plausible.
    // The instruction says which, and until this was measured nothing here could ask.
    let (shift, mask) = IMAGE_DIMENSION;
    let dimension = (instruction.word >> shift) & mask;
    if dimension != IMAGE_DIMENSION_2D {
        return Err(refuse(concat!(
            "only a two-dimensional image is translated - this instruction's coordinate has a ",
            "different number of components, and reading two of them would sample a place the ",
            "guest did not name"
        )));
    }

    // A load reads a texel by its integer coordinate and names no sampler; a sample takes a
    // position across the image and names one. Everything that differs between them follows
    // from this one line.
    let fetches = name == "image_load";
    // The level from a register rather than named. It is the **last** address element, after the
    // coordinates - measured, by compiling a levelled sample whose three arguments arrive in
    // known registers and reading which the compiler put where (worklog 577).
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

    // Which of the sampling instructions this is. `lz` in a guest mnemonic is "level zero,
    // named", `_l` takes the level from a register, and the plain form lets the implementation
    // choose from the derivatives of the coordinate - which is a different SPIR-V instruction
    // with different operands rather than an option on one (worklog 566).
    let named_level = name.ends_with("_lz") || levelled;

    let f32_type = model.f32_type();
    let u32_type = model.u32_type();
    let zero = model.constant(0);
    let constant_level = model.as_float(zero);

    for lane in running_lanes(model) {
        // Two dimensions, from consecutive registers - the dimensionality field was checked
        // above, so the count is not in doubt here.
        let u = model.read_source(instruction, &Operand::Vector(address_of(address)), lane)?;
        let v = model.read_source(instruction, &Operand::Vector(address_of(address + 1)), lane)?;
        // The level, where the instruction takes one from a register: the **last** address
        // element, after the coordinates.
        let level = if levelled {
            let bits =
                model.read_source(instruction, &Operand::Vector(address_of(address + 2)), lane)?;
            model.as_float(bits)
        } else {
            constant_level
        };
        // **A fetch's coordinate is already what it needs to be.** The registers hold a texel
        // index, which is an integer, and every register in this model is one - so the words go
        // straight in. A sample's coordinate is a position across the image and the same words
        // are float bits, which is the reinterpretation the other branch does.
        let (u, v, kind) = if fetches {
            (u, v, texture.texel)
        } else {
            (model.as_float(u), model.as_float(v), texture.coordinate)
        };

        let builder = model.builder();
        let coordinate = builder.id();
        builder.function(op::COMPOSITE_CONSTRUCT, &[kind.0, coordinate.0, u.0, v.0]);
        // Loaded per lane rather than hoisted: the load is what turns the bound descriptor into
        // a value, and a translation that hoisted it would be reordering the guest's program on
        // an assumption about aliasing nobody here has checked.
        let bound = builder.id();
        builder.function(op::LOAD, &[texture.sampled.0, bound.0, texture.variable.0]);
        let texel = builder.id();
        if fetches {
            // A fetch takes an image rather than an image and its sampler, so the bound pair is
            // unwrapped first. The level is named because Vulkan requires one here for an image
            // that is not multi-sampled, and zero is the only level this harness binds.
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

        // The mask says which components come back, and they land in **consecutive** registers
        // - so a mask with a hole in it does not leave a hole in the destination.
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
/// # What the instruction says
///
/// Four operands, the same shape as a fetch: the **data** registers rather than a destination,
/// the coordinate registers, the image descriptor, and the component mask. The mask decides how
/// many consecutive registers hold the texel, exactly as it decides how many a fetch fills.
///
/// # The components the mask does not select
///
/// Written as zero. A guest that stores three channels leaves the fourth to whatever the format
/// says, and the format is in a descriptor nothing here decodes (D692) - so there is no value to
/// preserve and no way to leave it alone. Zero is stated rather than inherited, which is the
/// difference between a choice and an accident.
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

        // The texel's components, in the order the mask selects them out of consecutive
        // registers - so a mask with a hole in it reads no register for that component.
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
/// Saturating rather than wrapping, and unreachable either way: the caller has already checked
/// the pair fits inside the register file. It exists so the conversion is in one place instead
/// of at both coordinate reads.
fn address_of(register: u32) -> u16 {
    u16::try_from(register).unwrap_or(u16::MAX)
}

/// Translates the flat memory instructions.
///
/// Loads and stores of one, two or four consecutive words. A load's destination and a
/// store's data sit in **different fields**, which is why these have per-opcode operand
/// layouts rather than a shared one (D096) - and why a test helper that built both the
/// same way put a destination where nothing read it.
fn flat_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // The flat loads, which differ only in how many consecutive registers they
        // fill. Probed rather than assumed to share an operand layout - "differs only in
        // width" is the assumption that produced four disagreeing scalar loads.
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
                    // Consecutive words, so consecutive addresses. Stepped by address
                    // rather than by index, so each word is bounds-checked on its own -
                    // a multi-word access starting inside the window can end outside it.
                    let stepped = step_address(model, address, word);
                    let value = read_guarded(model, stepped);
                    model.write_vector_lane(register + word, lane, value);
                }
            }
            model.count();
            Ok(())
        }

        // The flat stores. Per lane, and masked - an inactive lane must not write,
        // because another lane will read what it would have left behind.
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
/// The wide flat forms make the end reachable: `global_load_dwordx4` into v253 would
/// write four registers where only three exist, and the file is an array with nothing
/// past it.
pub const VECTOR_REGISTERS: u32 = 256;

/// The byte address `offset` words past `address`.
///
/// Multi-word accesses step by *address* rather than by index so each word is checked
/// against the window on its own: an access starting inside it can end outside, and
/// stepping a masked index would wrap the tail onto the front of the buffer.
fn step_address<M: Model + ?Sized>(model: &mut M, address: Id, offset: u32) -> Id {
    if offset == 0 {
        return address;
    }
    let step = model.constant(offset * 4);
    model.add(address, step)
}

/// Translates `s_wqm_b64`: whole quad mode.
///
/// Each group of four bits of the result is set if any of the corresponding four bits of
/// the source is. Computed by folding the group together with two shifts and two ors,
/// then spreading the answer back across all four positions - which is exact and needs no
/// per-bit loop over sixty-four lanes.
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
/// One register rather than a pair, so one [`quad_expand`] and the destination rules
/// `s_mov_b32` has: `exec_lo` or `vcc_lo` sets that mask's low half, anything else must be
/// a scalar register. The GL context's textured pixel shader runs it on `exec_lo` itself so
/// the helper pixels of every covered quad execute the sample and its derivatives exist
/// (oops-sdk `tools/shader/tex-prolog.s`; worklog 819).
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
/// Reached only for families the supported list already admits, so the fallthrough is
/// a translator bug rather than an unhandled guest instruction - but it still returns
/// an error rather than panicking, because the two are indistinguishable from the
/// outside and one of them is recoverable.
fn memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // The scalar loads, which differ only in how many consecutive registers they
        // fill. One arm rather than four, because writing them separately would be four
        // copies of the same address arithmetic differing in a loop bound.
        "s_load_dword" | "s_load_dwordx2" | "s_load_dwordx4" | "s_load_dwordx8" => {
            // The suffix says how many consecutive registers are filled. Read from
            // the name rather than computed from the opcode, because the opcodes are
            // only consecutive on the generation they were numbered for.
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

            // A destination that would run off the end of the register file is refused
            // rather than truncated. The registers stop at 101 and the wide forms take
            // up to eight, so this is reachable from a legal-looking encoding.
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
                // Consecutive words, so consecutive indices - the address arithmetic is
                // done once and stepped, rather than recomputed per word.
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

        // Flat memory, split out because the scalar loads above already fill this
        // function and these ask a different question - a per-lane address rather than a
        // uniform one.
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
