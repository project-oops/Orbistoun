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
use orbistoun_spirv::{Builder, Id, op};

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
    "buffer_load_dword",
    "buffer_store_dword",
    "tbuffer_load_format_x",
    "tbuffer_load_format_xy",
    "tbuffer_load_format_xyzw",
    "tbuffer_store_format_x",
    "tbuffer_store_format_xyzw",
    "ds_read_b32",
    "ds_write_b32",
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
    "s_mov_b64",
    "s_movk_i32",
    "s_mulk_i32",
    "s_or_b32",
    "s_or_b64",
    "s_sub_i32",
    "s_waitcnt",
    "s_wqm_b64",
    "s_xor_b32",
    "v_add_co_u32",
    "v_add_f32_e32",
    "v_add_f32_e64",
    "v_add_nc_u32_e32",
    "v_add_co_ci_u32_e64",
    "v_cmp_eq_f32_e32",
    "v_cmp_gt_f32_e32",
    "v_cmp_lt_f32_e32",
    "v_cmp_lt_u32_e32",
    "v_cndmask_b32_e64",
    "v_div_fixup_f32",
    "v_fmac_f32_e32",
    "v_div_scale_f32",
    "v_div_fmas_f32",
    "v_fma_f32",
    "v_lshlrev_b32_e32",
    "v_mbcnt_hi_u32_b32",
    "v_mbcnt_lo_u32_b32",
    "v_mov_b32_e32",
    "v_mul_f32_e32",
    "v_mul_f32_e64",
    "v_rcp_f32_e32",
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
/// It is the top of the seven-bit field, which the shared operand numbering also uses
/// for the high half of the execution mask - so a decoded no-base marker arrives named
/// `exec_hi`. The same code means different things in different fields, which is a fact
/// about the encoding rather than a decoding fault, and is why this is matched by name
/// here rather than fixed in the operand table.
pub const FLAT_NO_BASE: &str = "exec_hi";

/// Scalar registers the guest has.
///
/// The shared operand numbering runs past this into specials and inline constants, so a
/// scalar destination at or above it is not a register at all. The wide loads make this
/// reachable: `s_load_dwordx8` at s100 would write four registers and then four
/// specials, and nothing downstream would notice.
pub const SCALAR_REGISTERS: u32 = 102;

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
    let whole_quad = name == "s_wqm_b64";

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
            detail: "a 64-bit source is neither a register pair, an inline constant, \
                     nor the execution mask",
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
/// # Why this is not just the absence of an entry in [`SUPPORTED`]
///
/// "No translation for this instruction" is true of everything unimplemented, and it
/// makes the worklist say the same thing about an instruction nobody has looked at and
/// one that is blocked on a whole subsystem. Those rank differently: the first is an
/// afternoon and the second is not, and a list that cannot tell them apart sends effort
/// at whichever is most frequent rather than whichever is next.
///
/// An entry here is a claim that the semantics are understood and the dependency named.
/// It is not a to-do list - anything that could simply be written should be written.
pub const BLOCKED: &[(&str, &str)] = &[(
    "exp",
    "exporting needs a render target to export to, and there is no concept of one \
         yet - every translated module today is a compute dispatch writing to a storage \
         buffer. Mapping an export onto that buffer would let fragment shaders \
         translate, but the mapping would be invented here rather than derived from \
         anything, and a shader that appears to work while writing its colour somewhere \
         arbitrary is worse than one that refuses",
)];

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

    /// How many lanes this model emits code for.
    ///
    /// One where an invocation *is* a lane; the full wavefront where one invocation
    /// simulates all of them.
    fn lanes(&self) -> u32;

    /// A constant of the given value, declared once however often it is used.
    fn constant(&mut self, value: u32) -> Id;

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
        // The window is a power of two words, so masking is the whole of keeping the
        // index legal.
        let limit = self.constant(self.memory_words() - 1);
        let u32_type = self.u32_type();
        let b = self.builder();
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, address.0, two.0],
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
        let words = self.constant(self.memory_words());
        let u32_type = self.u32_type();
        let bool_type = self.bool_type();
        let b = self.builder();
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, address.0, two.0],
        );
        let inside = b.id();
        b.function(op::ULESS_THAN, &[bool_type.0, inside.0, shifted.0, words.0]);
        inside
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

    /// A float comparison, producing a boolean.
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
        let offset = self.read_source(instruction, vaddr, lane)?;
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
            detail: "this target has no recorded name for that opcode, so there is \
                     nothing to translate it as",
        })
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
        //   s_clause  - "define a clause of instructions which are executed together"
        // (the instruction-set reference for this generation). It groups
        // the instructions that follow so they are issued without
        // interruption; the instructions themselves are unchanged and it
        // computes nothing. A scheduling directive, and scheduling is the
        // host driver's business once this is SPIR-V.
        //
        // Matched explicitly rather than falling through, because "emits nothing" and
        // "nobody handled it" must never look the same.
        "s_endpgm" | "s_waitcnt" | "s_clause" => Ok(()),

        // s_mov_b32: once for the whole wavefront, since scalar registers are uniform.
        // The scalar moves. Split out for the same reason the memory instructions
        // were: the combined match outgrew a screen, and these two ask a question the
        // others do not - how wide the operand is.
        "s_mov_b32" | "s_mov_b64" => scalar_move(model, instruction, name),

        // s_wqm_b64: whole quad mode. Sets each group of four bits of the result if any
        // of the corresponding four in the source is set - so a derivative computed
        // across a quad has all four pixels live even where only one is covered.
        "s_wqm_b64" => whole_quad_mode(model, instruction),

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
        "v_add_co_u32" | "v_sub_co_u32" | "v_add_co_ci_u32_e64" => {
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
            for lane in 0..model.lanes() {
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
            for lane in 0..model.lanes() {
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
        | "v_lshlrev_b32_e32" | "v_add_nc_u32_e32" | "v_fmac_f32_e32" => {
            short_form_arithmetic(model, instruction, name)
        }

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
        "buffer_load_dword" | "buffer_store_dword" => buffer_memory(model, instruction, name),
        name if name.starts_with("tbuffer_") => typed_buffer_memory(model, instruction, name),
        "ds_write_b32" | "ds_read_b32" => local_share(model, instruction, name),

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

    for lane in 0..model.lanes() {
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

    for lane in 0..model.lanes() {
        let lhs = model.read_source(instruction, first, lane)?;
        let rhs = model.read_source(instruction, second, lane)?;
        let value = match name {
            // Integer: address arithmetic, and the shift whose amount comes *first*.
            // Reading a shift in written order computes `2 << index` where `index << 2`
            // was meant, and those agree for lane two and no other.
            "v_add_nc_u32_e32" => model.binary(op::IADD, lhs, rhs),
            "v_lshlrev_b32_e32" => model.binary(op::SHIFT_LEFT_LOGICAL, rhs, lhs),
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

/// The bit pattern of 1.0f.
const ONE_F32: u32 = 0x3F80_0000;

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
                detail: "a source modifier on integer carry arithmetic, which is not \
                         translated",
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
    let mask_name = match scalar_destination {
        Operand::Named(name) => lane_mask_name(name).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a carry destination this translator does not know",
        })?,
        // An ordinary register pair as the carry destination is legal and needs the
        // general per-register write the lane-mask methods do not offer. Refused rather
        // than dropped: a carry silently going nowhere is an address that is wrong only
        // sometimes.
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "carry arithmetic into an ordinary register pair is not \
                         translated yet; only the condition mask is",
            });
        }
    };

    let carry_in = if name == ADDC {
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
            ADDC => {
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

    model.write_lane_mask(mask_name, mask.0, mask.1)?;
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

/// `v_addc_co_u32`, which takes a carry in as well as producing one.
const ADDC: &str = "v_add_co_ci_u32_e64";

/// Translates the long-form vector ALU instructions.
///
/// One arm for the two-, three- and four-operand shapes, because what separates them is
/// only how many sources they read - and the modifiers apply the same way to each.
fn long_form_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read(
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

    if name == CNDMASK {
        return select_per_lane(model, instruction, register, &sources, modifiers);
    }
    if name == "v_div_fmas_f32" {
        return division_fmas(model, instruction, register, &sources, modifiers);
    }

    for lane in 0..model.lanes() {
        let mut read = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let raw = model.read_source(instruction, source, lane)?;
            read.push(apply_modifiers(model, raw, modifiers, index));
        }
        let value = combine(model, instruction, name, &read)?;
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
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

    for lane in 0..model.lanes() {
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
    let mask_name = match scalar_destination {
        Operand::Named(named) => lane_mask_name(named).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the division pre-scale writes a destination this translator does \
                     not know as a lane mask",
        })?,
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "the division pre-scale writes its flag somewhere other than a \
                         lane mask, which is not translated",
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

    model.write_lane_mask(mask_name, halves.0, halves.1)?;
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

    for lane in 0..model.lanes() {
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
            detail: "a 64-bit scalar logical destination is neither a register pair nor \
                     the execution mask",
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
                    detail: "s_mov_b64 destination is neither a scalar register nor the \
                             execution mask",
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

            let Operand::Scalar(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "s_mov_b32 destination is neither a scalar register nor a \
                             lane mask",
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
            detail: "a local-data-share access carries no byte offset - the operand \
                     layout for this opcode is missing one",
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

    for lane in 0..model.lanes() {
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
/// Only formats whose components are all thirty-two bits wide. Those move whole words
/// unchanged, so the work is an untyped access repeated per component and the format
/// contributes nothing but a component count.
///
/// Everything else is **refused by name**. A narrower component has to be extracted from
/// within a word and converted - a normalised eight-bit value becomes a float by dividing
/// by 255, a half-precision one needs a real conversion - and none of that is written.
/// Translating it as if it were a word would produce a shader that runs, draws, and is
/// wrong in a way only a rendered frame would show, which is the failure this project is
/// least equipped to catch.
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
    if !format.is_plain_words() {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer format needing component conversion",
        });
    }
    if format.components() != channels as usize {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer access whose format and channel count disagree",
        });
    }

    buffer_access(model, instruction, loading, channels)
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
    // One dword, which is what "untyped" means here: no format, no conversion, no
    // component count beyond the one.
    buffer_access(model, instruction, name == "buffer_load_dword", 1)
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
            detail: "a buffer access names something other than a vector register for \
                     its data",
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

    for lane in 0..model.lanes() {
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
                        detail: "a buffer access with both address modifiers does not \
                                 name a vector register pair",
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
            let (destination, vaddr, base) = three_operands(instruction)?;
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
            for lane in 0..model.lanes() {
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
            let (vaddr, data, base) = three_operands(instruction)?;
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
            for lane in 0..model.lanes() {
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
