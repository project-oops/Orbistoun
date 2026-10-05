//! Turning decoded guest shaders into SPIR-V.
//!
//! The guest runs sixty-four (or thirty-two) lanes in lockstep under an explicit execution
//! mask, with branches as mask arithmetic; SPIR-V describes one invocation with structured
//! control flow. [`Strategy`] chooses how control flow is expressed and [`Fidelity`] how the
//! wavefront is modelled; fidelity is a field of [`Strategy::Predicated`] so an invalid
//! pairing cannot be written. The fidelity levels are a differential oracle: two levels
//! disagreeing localises a bug to one shader and instruction (D100). [`Fidelity::Lane`] has
//! no mask and refuses a shader that writes one, which lets [`Fidelity::Auto`] choose; an
//! unbuilt path is an error, never a substitution (D098).

pub mod blocks;
mod buffer;
pub mod control;
pub mod draw_buffers;
pub mod model;
pub mod modifiers;
pub mod predicated;
pub mod wavefront;

pub use predicated::{OBSERVED_REGISTERS, REGISTER_COUNT};

use orbistoun_shader::{Decode, EncodingTable};

/// How the wavefront is modelled.
///
/// The guest executes sixty-four lanes in lockstep; SPIR-V describes one invocation. The
/// levels differ in correctness, not only speed, so none is picked silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Fidelity {
    /// Pick the cheapest level valid for this shader on this machine.
    #[default]
    Auto,

    /// One invocation per lane, and lanes never interact.
    ///
    /// The execution mask is implicit and cross-lane instructions are impossible, so it is
    /// correct only for shaders that never treat the wavefront as an object.
    Lane,

    /// One invocation per lane, with the mask materialised by subgroup ballot.
    ///
    /// Cross-lane instructions become subgroup operations. Correct when the host subgroup
    /// size matches the guest wavefront, which is not guaranteed.
    Subgroup,

    /// One invocation simulates an entire wavefront.
    ///
    /// Registers are arrays indexed by lane and the execution mask is an ordinary value,
    /// so cross-lane instructions are array reads and mask arithmetic translates directly.
    /// Very slow and unconditionally correct, which makes it the oracle the other levels
    /// are judged against.
    Wavefront,
}

impl core::fmt::Display for Fidelity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Lane => write!(f, "lane"),
            Self::Subgroup => write!(f, "subgroup"),
            Self::Wavefront => write!(f, "wavefront"),
        }
    }
}

/// How guest control flow is expressed in SPIR-V.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Strategy {
    /// The guest's flat instruction stream, executed under a mask.
    Predicated {
        /// How the wavefront is modelled.
        fidelity: Fidelity,
        /// How many lanes the shader was compiled for.
        width: Width,
    },
    /// Reconstructed structured control flow. Refused as not implemented (D098).
    Structured,
}

impl Default for Strategy {
    fn default() -> Self {
        Self::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        }
    }
}

/// How many lanes a shader's wavefront has.
///
/// The width is chosen per shader at compile time and the encodings are identical either
/// way, so it is supplied by the caller from the pipeline state rather than inferred
/// (D145). Defaults to sixty-four, the previous generation's width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Width {
    /// Thirty-two lanes, the narrow mode this generation adds.
    Wave32,
    /// Sixty-four lanes.
    #[default]
    Wave64,
}

impl Width {
    /// The number of lanes.
    pub const fn lanes(self) -> u32 {
        match self {
            Self::Wave32 => 32,
            Self::Wave64 => 64,
        }
    }

    /// Whether a lane mask needs a second register to hold its upper half.
    ///
    /// The narrow mode's mask fits in one, so its shaders use the 32-bit mask instructions.
    pub const fn needs_upper_half(self) -> bool {
        matches!(self, Self::Wave64)
    }
}

impl core::fmt::Display for Width {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "wave{}", self.lanes())
    }
}

impl core::fmt::Display for Strategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Predicated { fidelity, width } => write!(f, "predicated/{fidelity}/{width}"),
            Self::Structured => write!(f, "structured"),
        }
    }
}

/// Why a shader could not be translated.
#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    /// A strategy that has not been built was asked for.
    #[error(
        "the {0} strategy is not implemented. It would reconstruct structured control flow from execution-mask arithmetic; only the predicated strategy exists. This is not falling back to it, because a silent substitution would present as unexplained slowness rather than as a missing feature"
    )]
    StrategyNotImplemented(Strategy),

    /// A fidelity level that has not been built was asked for.
    ///
    /// Not a fallback: the levels differ in correctness, so a substitution would produce
    /// wrong output with nothing to point at.
    #[error(
        "the {level} model is not implemented ({would}). This is not falling back to another level, because the levels differ in correctness rather than only in speed - a substitution would render something subtly wrong with nothing to indicate it"
    )]
    FidelityNotImplemented {
        /// Which level.
        level: Fidelity,
        /// What it would have done, so the message says what is missing.
        would: &'static str,
    },

    /// The decode this was handed cannot be trusted.
    #[error(
        "refusing to translate an untrustworthy decode ({reason}) - the instruction stream may not be what it appears to be"
    )]
    UntrustworthyDecode {
        /// Which property failed.
        reason: &'static str,
    },

    /// An instruction whose encoding family was not recognised at all.
    #[error("instruction at {offset:#x} was not recognised; there is nothing to translate")]
    Unrecognised {
        /// Byte offset within the shader.
        offset: u32,
    },

    /// An instruction this translator does not handle yet.
    ///
    /// Never silently skipped: a shader missing one instruction computes the wrong thing
    /// while appearing to work.
    #[error("instruction at {offset:#x} cannot be translated: {detail}")]
    Unsupported {
        /// Byte offset within the shader.
        offset: u32,
        /// What specifically was not handled.
        detail: &'static str,
    },

    /// An instruction the decoder named and this translator has no translation for.
    ///
    /// The mnemonic is carried so a report names the instruction a shader needs, not only where
    /// it sits.
    #[error("instruction at {offset:#x} ({mnemonic}) cannot be translated: {detail}")]
    NotTranslated {
        /// Byte offset within the shader.
        offset: u32,
        /// The instruction's name, as the encoding table gives it.
        mnemonic: String,
        /// Why: [`model::NO_TRANSLATION`].
        detail: &'static str,
    },

    /// A primitive shader reads the geometry engine's inputs before writing them, and the
    /// translation was not given the draw's geometry to seed them with (D730).
    #[error(
        "the primitive shader reads the geometry engine's inputs - its system SGPRs (gs_tg_info, merged_wave_info) or its input VGPRs (vertex offsets, vertex id) - before writing them, and no draw's geometry is given to seed them"
    )]
    ReadsGeometryInputs,

    /// A buffer load converts by its descriptor's format, which is the draw's (D738), and the
    /// translation was not given a draw's descriptors.
    #[error(
        "the shader converts a buffer load by the format its descriptor names, which is the draw's (D738), and no draw's descriptors are given"
    )]
    NeedsBufferFormats,

    /// An instruction with no known operand layout was reached.
    #[error(
        "instruction at {offset:#x} ({name}, first word {word:#010x}) has no operand layout; cannot translate what it operates on{}",
        if others.is_empty() { String::new() } else { format!(" - nor can the program's other instructions without one: {}", others.join(", ")) }
    )]
    OperandsUnknown {
        /// Byte offset within the shader.
        offset: u32,
        /// Its mnemonic, or its encoding and opcode where the table names none - what a layout
        /// would be recorded for.
        name: String,
        /// Its first word, as decoded.
        word: u32,
        /// Every other instruction of the program with no layout, by name, each once: what to
        /// record alongside it.
        others: Vec<String>,
    },

    /// The module built does not hang together.
    ///
    /// A translator bug, caught here rather than as an undiagnosed driver fault.
    #[error("the translated module is malformed: {0}. This is a translator bug")]
    MalformedModule(#[from] orbistoun_spirv::ModuleError),
}

/// Something worth telling the caller about a translation that succeeded.
///
/// Distinct from [`TranslateError`]: these describe output that is correct and costs
/// something the caller may not expect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// [`Fidelity::Auto`] had to fall back to the slowest level.
    ///
    /// The lane model has no execution mask, so a shader that turns lanes off falls back to
    /// simulating the whole wavefront in one invocation, sixty-four lanes per instruction.
    /// One instruction touching `exec` triggers it, so it is a warning rather than only a
    /// field.
    SlowestFidelity {
        /// What in the shader forced it.
        because: &'static str,
        /// The width a subgroup would have to be for the faster masking level to work.
        ///
        /// [`Fidelity::Subgroup`] is as fast as the lane model and has a mask; whether it fits
        /// is a property of the device, which the translator has not seen, so it reports what
        /// would be needed.
        subgroup_would_need: u32,
    },
}

impl core::fmt::Display for Warning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SlowestFidelity {
                because,
                subgroup_would_need,
            } => write!(
                f,
                concat!(
                    "translated at wavefront fidelity, which simulates every lane in one ",
                    "invocation and is far slower: {}. Subgroup fidelity would do the ",
                    "same work with a mask if this device's subgroup is {} ",
                    "wide"
                ),
                because, subgroup_would_need
            ),
        }
    }
}

/// A translated shader.
#[derive(Debug, Clone)]
pub struct Translated {
    /// The SPIR-V module, as words.
    pub module: Vec<u32>,
    /// Which strategy produced it.
    pub strategy: Strategy,
    /// The level actually used, with [`Fidelity::Auto`] already resolved.
    ///
    /// Reported so a silently slower level is distinguishable from a slow shader.
    pub fidelity: Fidelity,
    /// Guest instructions translated.
    pub instructions: usize,
    /// Things the caller should be told rather than left to look for.
    ///
    /// Empty in the common case; see [`Warning`].
    pub warnings: Vec<Warning>,
    /// The host subgroup width this module needs, when it needs a particular one.
    ///
    /// Set only by [`Fidelity::Subgroup`], where one invocation is one guest lane and the
    /// widths must match. Reported rather than checked: the translator does not know the
    /// device.
    pub required_subgroup: Option<u32>,
    /// The textures the module samples, in binding order, and where each one's descriptor
    /// came from. Empty for a module that samples nothing and for every model but the wavefront.
    pub textures: Vec<wavefront::TextureSource>,
    /// Where a compute module's stored image's descriptor came from. `None` for a module that
    /// stores to no image, and for a pixel shader, whose storage image is the harness's.
    pub storage: Option<wavefront::TextureSource>,
}

impl Translated {
    /// The module as bytes, for writing out or handing to a driver.
    pub fn bytes(&self) -> Vec<u8> {
        self.module.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

/// The refusal for a program with instructions whose operand layout is unknown: the first, and the
/// others by name, each once. Failing that, the one for instructions whose opcode this target has
/// no name for, listed the same way. `None` when every instruction has both.
fn unlaid_instructions(decode: &Decode, encodings: &EncodingTable) -> Option<TranslateError> {
    let named = |instruction: &&orbistoun_shader::Instruction| {
        instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .is_none_or(|e| {
                encodings
                    .mnemonic_for(&e.name, instruction.opcode)
                    .is_some()
            })
    };
    let mut unlaid = decode.instructions.iter().filter(|i| !i.operands_decoded);
    let Some(first) = unlaid.next() else {
        let mut unnamed = decode.instructions.iter().filter(|i| !named(i));
        let first = unnamed.next()?;
        // What came before it, and how long it was read as: a word mistaken for an instruction
        // follows one whose length was misread.
        let before = decode
            .instructions
            .iter()
            .take_while(|i| i.offset < first.offset)
            .last()
            .map_or_else(String::new, |i| {
                format!(
                    ", after {} at {:#x} read as {} bytes",
                    instruction_name(i, encodings),
                    i.offset,
                    i.length
                )
            });
        let mut listed = vec![instruction_name(first, encodings)];
        for other in unnamed.map(|i| instruction_name(i, encodings)) {
            if !listed.contains(&other) {
                listed.push(other);
            }
        }
        return Some(TranslateError::NotTranslated {
            offset: first.offset,
            mnemonic: format!(
                "{}, first word {:#010x}{before}",
                listed.join("; also "),
                first.word
            ),
            detail: concat!(
                "this target has no recorded name for that opcode, so there is ",
                "nothing to translate it as"
            ),
        });
    };
    let name = instruction_name(first, encodings);
    let mut others: Vec<String> = Vec::new();
    for other in unlaid.map(|i| instruction_name(i, encodings)) {
        if other != name && !others.contains(&other) {
            others.push(other);
        }
    }
    Some(TranslateError::OperandsUnknown {
        offset: first.offset,
        name,
        word: first.word,
        others,
    })
}

/// An instruction's mnemonic, or its encoding and opcode where the table names none.
fn instruction_name(
    instruction: &orbistoun_shader::Instruction,
    encodings: &EncodingTable,
) -> String {
    let encoding = instruction
        .encoding
        .and_then(|i| encodings.encodings().get(usize::from(i)));
    encoding
        .and_then(|e| encodings.mnemonic_for(&e.name, instruction.opcode))
        .map_or_else(
            || {
                format!(
                    "{} opcode {}",
                    encoding.map_or("an unknown encoding", |e| e.name.as_str()),
                    instruction.opcode
                )
            },
            str::to_owned,
        )
}

/// Picks the cheapest fidelity level valid for a shader.
///
/// [`Fidelity::Lane`] unless the shader touches the execution mask, in which case
/// [`Fidelity::Wavefront`]: the lane model cannot represent an inactive lane and would run
/// every lane. This is not a silent substitution (D098), because `Auto` asks to be told what
/// the shader needs; an explicit request for the lane model is refused by that model.
fn resolve(requested: Fidelity, decode: &Decode, encodings: &EncodingTable) -> Fidelity {
    match requested {
        Fidelity::Auto => {
            let families: Vec<&str> = encodings
                .encodings()
                .iter()
                .map(|e| e.name.as_str())
                .collect();
            let needs_mask = decode.instructions.iter().any(|instruction| {
                let Some(family) = instruction
                    .encoding
                    .and_then(|e| families.get(usize::from(e)).copied())
                else {
                    return false;
                };
                // Asked by name: touching a lane mask is a property of the instruction,
                // not of its number on this generation. An opcode with no recorded name
                // answers `false`; translation then refuses it on the name lookup, so the
                // report names the unknown opcode rather than the fidelity.
                encodings
                    .mnemonic_for(family, instruction.opcode)
                    .is_some_and(|name| model::touches_mask(instruction, name))
            });
            if needs_mask {
                Fidelity::Wavefront
            } else {
                Fidelity::Lane
            }
        }
        other => other,
    }
}

/// Translates a decoded shader as a compute dispatch.
///
/// Refuses rather than approximates. Callers that know which stage bound the shader want
/// [`translate_staged`]: a fragment shader translated as a compute dispatch is refused at
/// its first interpolation.
pub fn translate(
    decode: &Decode,
    encodings: &EncodingTable,
    strategy: Strategy,
) -> Result<Translated, TranslateError> {
    translate_staged(decode, encodings, strategy, wavefront::Stage::Compute)
}

/// Translates a decoded shader for the stage that bound it.
///
/// Only the wavefront model has fragment inputs and a colour output, so a graphics stage
/// raises the fidelity to it and says so with the same warning automatic resolution uses.
pub fn translate_staged(
    decode: &Decode,
    encodings: &EncodingTable,
    strategy: Strategy,
    stage: wavefront::Stage,
) -> Result<Translated, TranslateError> {
    translate_windowed(
        decode,
        encodings,
        strategy,
        stage,
        wavefront::Window::default(),
    )
}

/// Translates a decoded shader for a stage, over a guest-memory window at a given address.
///
/// A translated shader reaches guest memory through one storage buffer over a fixed span of
/// the address space, with every access checked against it. Where the span sits is the
/// caller's knowledge: wherever the shader's buffers were put. A window at zero covers
/// nothing a guest addresses.
/// # Errors
///
/// Whatever the translation refuses: an unsupported instruction, an untrustworthy decode.
pub fn translate_windowed(
    decode: &Decode,
    encodings: &EncodingTable,
    strategy: Strategy,
    stage: wavefront::Stage,
    window: wavefront::Window,
) -> Result<Translated, TranslateError> {
    // A caller binding a mesh stage whose topology it decoded uses
    // `translate_windowed_primitive`; every other gets the default triangle.
    translate_windowed_primitive(
        decode,
        encodings,
        strategy,
        stage,
        wavefront::MeshPrimitive::default(),
        window,
    )
}

/// As [`translate_windowed`], for a caller that knows the mesh primitive the stream set.
///
/// # Errors
///
/// Whatever the translation refuses: an unsupported instruction, an untrustworthy decode.
pub fn translate_windowed_primitive(
    decode: &Decode,
    encodings: &EncodingTable,
    strategy: Strategy,
    stage: wavefront::Stage,
    primitive: wavefront::MeshPrimitive,
    window: wavefront::Window,
) -> Result<Translated, TranslateError> {
    translate_with_user_data(
        decode,
        encodings,
        strategy,
        (stage, primitive),
        window,
        wavefront::UserData::default(),
    )
}

/// The modifier marker an instruction's first source selects - SDWA or DPP - when it has one.
fn modifier_of(
    instruction: &orbistoun_shader::Instruction,
    encodings: &EncodingTable,
) -> Option<u32> {
    let family = encodings
        .encodings()
        .get(usize::from(instruction.encoding?))?;
    family.modifier_selected(&[instruction.word])
}

/// Why a module was built at the slowest fidelity, when that was not what was asked for: a
/// graphics stage, a guest dispatch, or a shader that touches a lane mask.
fn fidelity_warnings(
    (staged, dispatch): (bool, bool),
    asked_for: Fidelity,
    fidelity: Fidelity,
    width: Width,
) -> Vec<Warning> {
    // Said as a warning rather than left in a field: the wavefront model costs a factor of
    // sixty-four.
    if staged && asked_for != Fidelity::Wavefront {
        vec![Warning::SlowestFidelity {
            because: concat!(
                "the module is for a graphics stage, and the wavefront model is the only one ",
                "with fragment inputs and a colour output"
            ),
            subgroup_would_need: width.lanes(),
        }]
    } else if dispatch && asked_for != Fidelity::Wavefront {
        vec![Warning::SlowestFidelity {
            because: concat!(
                "the module is a guest dispatch, and the wavefront model is the only one with ",
                "its entry state and exact memory"
            ),
            subgroup_would_need: width.lanes(),
        }]
    } else if asked_for == Fidelity::Auto && fidelity == Fidelity::Wavefront {
        vec![Warning::SlowestFidelity {
            because: concat!(
                "the shader reads or writes a lane mask, which the per-lane model cannot ",
                "represent"
            ),
            subgroup_would_need: width.lanes(),
        }]
    } else {
        Vec::new()
    }
}

/// As [`translate_windowed_primitive`], for a module that reads its stage's user data at entry
/// from the push-constant block a draw supplies. A graphics stage is always the wavefront
/// model, which is the one that reads it.
///
/// # Errors
///
/// Whatever the translation refuses, and a stage taking more user-data words than its share of
/// the block.
pub fn translate_with_user_data(
    decode: &Decode,
    encodings: &EncodingTable,
    strategy: Strategy,
    (stage, primitive): (wavefront::Stage, wavefront::MeshPrimitive),
    window: wavefront::Window,
    user_data: wavefront::UserData,
) -> Result<Translated, TranslateError> {
    let Strategy::Predicated { fidelity, width } = strategy else {
        return Err(TranslateError::StrategyNotImplemented(strategy));
    };
    let asked_for = fidelity;
    let mut fidelity = resolve(fidelity, decode, encodings);
    let staged = stage != wavefront::Stage::Compute;
    // A guest dispatch's entry state and exact memory exist only in the wavefront model.
    let dispatch = !staged && user_data.compute.is_some();
    if staged || dispatch {
        fidelity = Fidelity::Wavefront;
    }

    let warnings = fidelity_warnings((staged, dispatch), asked_for, fidelity, width);

    if decode.desynchronised {
        return Err(TranslateError::UntrustworthyDecode {
            reason: "the decode desynchronised",
        });
    }
    if decode.overran {
        return Err(TranslateError::UntrustworthyDecode {
            reason: "an instruction ran past the end of the shader",
        });
    }

    if let Some(refusal) = unlaid_instructions(decode, encodings) {
        return Err(refusal);
    }
    for instruction in &decode.instructions {
        if let Some(marker) = modifier_of(instruction, encodings)
            && !model::sdwa_translated(marker, instruction, encodings)
        {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "the instruction carries an SDWA or DPP modifier word, which selects parts ",
                    "of its operands or other lanes' values; only SDWA on the integer ",
                    "instructions `model::SDWA_INTEGER` names is translated"
                ),
            });
        }
    }

    match fidelity {
        Fidelity::Lane => {
            let (module, instructions) = predicated::translate(decode, encodings, window)?;
            Ok(Translated {
                module,
                strategy,
                fidelity,
                instructions,
                warnings,
                required_subgroup: None,
                textures: Vec::new(),
                storage: None,
            })
        }
        Fidelity::Wavefront => {
            let (module, instructions, (textures, storage)) = wavefront::translate_with_user_data(
                decode,
                encodings,
                width,
                (stage, primitive),
                window,
                user_data,
            )?;
            Ok(Translated {
                module,
                strategy,
                fidelity,
                instructions,
                warnings,
                required_subgroup: None,
                textures,
                storage,
            })
        }
        Fidelity::Subgroup => {
            let (module, instructions, required_subgroup) =
                predicated::translate_subgroup(decode, encodings, width, window)?;
            Ok(Translated {
                module,
                strategy,
                fidelity,
                instructions,
                warnings,
                required_subgroup: Some(required_subgroup),
                textures: Vec::new(),
                storage: None,
            })
        }
        // `resolve` turns Auto into a concrete level, so this is unreachable.
        Fidelity::Auto => Err(TranslateError::FidelityNotImplemented {
            level: fidelity,
            would: "have been resolved to a concrete level before dispatch",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{Fidelity, Strategy, TranslateError, Warning, Width, translate};
    use crate::predicated::MEMORY_WORDS;
    use crate::wavefront::Window;

    /// A module that reads user data declares the push-constant block; one that reads none
    /// does not and is unchanged; a stage wanting more than its thirty-two words is refused
    /// rather than truncated.
    #[test]
    fn user_data_declares_the_block_only_when_read_and_refuses_too_much() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let decode = decode_program(&0xBF81_0000u32.to_le_bytes(), &encodings, &operands);
        let strategy = Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::default(),
        };
        let with = |count| {
            super::translate_with_user_data(
                &decode,
                &encodings,
                strategy,
                (Stage::Fragment, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    first_register: 0,
                    count,
                    block_offset: 16,
                    dx10_clamp: None,
                    pixel_inputs: None,
                    compute: None,
                    geometry: None,
                    window_space: false,
                    draw_buffers: false,
                    buffer_formats: None,
                },
            )
        };
        // `OpVariable` is opcode 59; its storage class is its fourth word.
        let push_variables = |module: &[u32]| {
            let mut at = 5;
            let mut found = 0;
            while at < module.len() {
                let length = (module[at] >> 16) as usize;
                if module[at] & 0xffff == 59 && module.get(at + 3) == Some(&9) {
                    found += 1;
                }
                at += length.max(1);
            }
            found
        };
        let reading = with(2).expect("two words translate");
        assert_eq!(push_variables(&reading.module), 1, "one block, declared");
        let plain = with(0).expect("no words translate");
        assert_eq!(
            push_variables(&plain.module),
            0,
            "no block when nothing is read"
        );
        let unchanged = super::translate_windowed_primitive(
            &decode,
            &encodings,
            strategy,
            Stage::Fragment,
            MeshPrimitive::default(),
            Window::default(),
        )
        .expect("translates");
        assert_eq!(
            plain.module, unchanged.module,
            "and word for word as before"
        );
        assert!(with(33).is_err(), "more than a stage's share is refused");
    }

    /// A window length that is not a power of two is refused, not rounded.
    ///
    /// `Model::word_index` masks with `words - 1` and `address_within_window` compares against
    /// `words`; they agree only for a power of two (with 100, the check admits word 99 while
    /// the mask folds it to word 35).
    #[test]
    fn a_window_length_that_would_alias_is_refused() {
        for bad in [3_u32, 5, 100, 1000, u32::MAX] {
            assert!(
                Window::spanning(0x900_000, bad).is_none(),
                "{bad} words is not a power of two and must be refused"
            );
        }
    }

    /// Zero words is refused: `words - 1` underflows to a mask admitting every address.
    #[test]
    fn a_window_of_no_words_is_refused() {
        assert!(Window::spanning(0x900_000, 0).is_none());
    }

    /// Power-of-two windows keep their length and base.
    #[test]
    fn a_power_of_two_window_keeps_its_length_and_base() {
        for good in [1_u32, 2, 64, 4096, 32_768, 1 << 31] {
            let window = Window::spanning(0x900_000, good).expect("a power of two is accepted");
            assert_eq!(window.words(), good);
            assert_eq!(window.base, 0x900_000);
        }
    }

    /// The declared buffer is the window's length, not a constant beside it.
    ///
    /// The SPIR-V array the shader indexes is declared separately from the checks, so this
    /// asserts a widened length reaches the module: at 65,536 words the literal is present,
    /// and at the default it is not. A real frame's buffers reach 32,768 words past the base,
    /// beyond the default window.
    #[test]
    fn a_widened_window_reaches_the_declared_buffer() {
        let (encodings, operands) = tables();
        let decoded = decode(&stream(TRIVIAL), &encodings, &operands);
        let strategy = Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        };
        let wide = 65_536_u32;

        let translated = crate::translate_windowed(
            &decoded,
            &encodings,
            strategy,
            crate::wavefront::Stage::Compute,
            Window::spanning(0x0090_0000, wide).expect("a power of two"),
        )
        .expect("translates");
        assert!(
            translated.module.contains(&wide),
            "the widened length must reach the module the shader indexes"
        );

        let narrow = crate::translate_windowed(
            &decoded,
            &encodings,
            strategy,
            crate::wavefront::Stage::Compute,
            Window::default(),
        )
        .expect("translates");
        assert!(
            !narrow.module.contains(&wide),
            "and a default window must not declare a buffer it was never given"
        );
        assert!(narrow.module.contains(&MEMORY_WORDS));
    }

    /// The default window keeps the default length and a zero base.
    #[test]
    fn the_default_window_is_the_length_it_always_was() {
        assert_eq!(Window::default().words(), MEMORY_WORDS);
        assert_eq!(Window::default().base, 0);
        assert_eq!(Window::at(0x900_000).words(), MEMORY_WORDS);
    }
    use orbistoun_shader::{EncodingTable, OperandTable, decode};

    fn tables() -> (EncodingTable, OperandTable) {
        (
            EncodingTable::builtin().expect("encodings"),
            OperandTable::builtin().expect("operands"),
        )
    }

    fn stream(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// A shader the lane model handles, for testing everything that is not the shader.
    const TRIVIAL: &[u32] = &[0xBF81_0000];

    /// Asking for the structured strategy is an error, not a fallback.
    #[test]
    fn asking_for_the_unbuilt_strategy_is_an_error_not_a_fallback() {
        let (table, operands) = tables();
        let decoded = decode(&stream(TRIVIAL), &table, &operands);
        let error = translate(&decoded, &table, Strategy::Structured).expect_err("must refuse");
        let text = error.to_string();
        assert!(text.contains("not implemented"), "got: {text}");
        assert!(text.contains("not falling back"), "got: {text}");
    }

    /// `Auto` falling back to the wavefront model warns, naming the subgroup width needed.
    #[test]
    fn falling_back_to_the_slowest_level_is_a_warning_not_a_footnote() {
        let (table, operands) = tables();

        // `s_mov_b64 exec, 0`: a mask write, so the lane model cannot take it.
        let mask_write = table
            .find_by_name("s_mov_b64")
            .map(|(family, opcode)| {
                let encoding = table
                    .encodings()
                    .iter()
                    .find(|e| e.name == family)
                    .expect("the family the name was found in");
                encoding.value | (opcode << encoding.opcode.shift) | (126 << 16) | 128
            })
            .expect("this target has s_mov_b64");

        let decoded = decode(&stream(&[mask_write, TRIVIAL[0]]), &table, &operands);
        let translated = translate(&decoded, &table, Strategy::default()).expect("translates");

        assert_eq!(translated.fidelity, Fidelity::Wavefront);
        let Some(Warning::SlowestFidelity {
            subgroup_would_need,
            ..
        }) = translated.warnings.first()
        else {
            panic!(
                "falling back to the slowest level must warn: {:?}",
                translated.warnings
            );
        };
        assert_eq!(*subgroup_would_need, Width::default().lanes());

        // A shader that needs no mask produces no warning.
        let quiet = decode(&stream(TRIVIAL), &table, &operands);
        let quiet = translate(&quiet, &table, Strategy::default()).expect("translates");
        assert_eq!(quiet.fidelity, Fidelity::Lane);
        assert!(quiet.warnings.is_empty(), "{:?}", quiet.warnings);
    }

    /// `Auto` is never reported as itself, and only the subgroup level sets a subgroup width.
    #[test]
    fn asking_for_automatic_fidelity_reports_the_level_it_chose() {
        let (table, operands) = tables();
        let decoded = decode(&stream(TRIVIAL), &table, &operands);

        let translated = translate(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Auto,
                width: Width::default(),
            },
        )
        .expect("a trivial shader translates");
        assert_ne!(
            translated.fidelity,
            Fidelity::Auto,
            "the level used must be reported concretely"
        );
        assert_eq!(
            translated.required_subgroup, None,
            "only the subgroup level constrains the device's subgroup width"
        );
    }

    /// `Auto` resolves a trivial shader to the lane model and reports it.
    #[test]
    fn auto_resolves_to_a_built_level_and_reports_which() {
        let (table, operands) = tables();
        let decoded = decode(&stream(TRIVIAL), &table, &operands);
        let translated = translate(&decoded, &table, Strategy::default()).expect("auto");
        assert_eq!(translated.fidelity, Fidelity::Lane);
        assert_ne!(
            translated.fidelity,
            Fidelity::Auto,
            "auto must be resolved before it is reported"
        );
    }

    /// The default strategy is predicated, automatic fidelity, sixty-four lanes.
    #[test]
    fn the_default_is_the_combination_that_works() {
        assert_eq!(
            Strategy::default(),
            Strategy::Predicated {
                fidelity: Fidelity::Auto,
                width: Width::Wave64,
            }
        );
    }

    /// A desynchronised decode is refused.
    #[test]
    fn an_untrustworthy_decode_is_refused() {
        let (table, operands) = tables();
        let decoded = decode(&stream(&[0xFFFF_FFF0]), &table, &operands);
        assert!(decoded.desynchronised, "the fixture must desynchronise");
        assert!(matches!(
            translate(&decoded, &table, Strategy::default()),
            Err(TranslateError::UntrustworthyDecode { .. })
        ));
    }

    /// A primitive shader that reads the geometry engine's inputs - its system SGPRs s0-s7 or its
    /// input VGPRs v0-v8 (`si_shader_args.c:304-371`) - before writing them is refused by name:
    /// nothing seeds them, so it would read zero where the hardware hands it its vertex and
    /// primitive counts. The same register written first reads what was written.
    #[test]
    fn a_primitive_shader_reading_unseeded_geometry_inputs_is_refused() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        const END: u32 = 0xBF81_0000;
        let (table, operands) = tables();
        let mesh = |words: &[u32]| {
            let decoded = decode(&stream(words), &table, &operands);
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::default(),
                },
                (Stage::Mesh, MeshPrimitive::default()),
                Window::default(),
                UserData::default(),
            )
        };
        // s_and_b32 s0, s3, s3: radeonsi's first look at merged_wave_info.
        let scalar_input = mesh(&[0x8700_0303, END]).expect_err("refused").to_string();
        assert!(scalar_input.contains("geometry engine"), "{scalar_input}");
        // v_mov_b32 v1, v5: the vertex id.
        let vertex_id = mesh(&[0x7E02_0305, END]).expect_err("refused").to_string();
        assert!(vertex_id.contains("geometry engine"), "{vertex_id}");
        // s_mov_b32 s3, 1 then s_and_b32 s0, s3, s3: written first.
        assert!(mesh(&[0xBE83_0381, 0x8700_0303, END]).is_ok());
        // v_mov_b32 v12, v20: not a geometry input.
        assert!(mesh(&[0x7E18_0314, END]).is_ok());
    }

    /// A descriptor loaded through an address the program formed is found where it was formed:
    /// radeonsi's blit pixel shader moves user-data word 3 into `s0` and the constant 4 into `s1`,
    /// then loads its image descriptor from `s[0:1] + 0x400`. A pair holding anything else - here
    /// a sum - is refused where it is sampled, not read as the table at words 0 and 1.
    #[test]
    fn a_descriptor_table_is_found_where_the_program_formed_its_address() {
        use crate::wavefront::{MeshPrimitive, Stage, TableBase, TableWord, UserData};
        let (table, operands) = tables();
        let fragment = |words: &[u32]| {
            let decoded = decode(&stream(words), &table, &operands);
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::default(),
                },
                (Stage::Fragment, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    count: 4,
                    ..UserData::default()
                },
            )
        };
        let load_and_fetch = [
            // s_load_dwordx8 s[8:15], s[0:1], 0x400
            0xf40c_0200,
            0xfa00_0400,
            0xbf8c_c07f,
            // image_load_mip v[0:3], v[2:4], s[8:15] dmask:0xf dim:2D unorm
            0xf004_1f08,
            0x0002_0002,
            // exp mrt0 v0, v1, v2, v3 done vm
            0xf800_180f,
            0x0302_0100,
            0xbf81_0000,
        ];
        let mut words = vec![0xbe80_0303, 0xbe81_0384];
        words.extend(load_and_fetch);
        let translated = fragment(&words).expect("translates");
        assert_eq!(
            translated
                .textures
                .first()
                .map(|t| (t.table_offset, t.table)),
            Some((
                Some(0x400),
                TableBase {
                    low: TableWord::UserData(3),
                    high: TableWord::Constant(4),
                }
            ))
        );
        // s_add_i32 s0, s3, s2 in place of the move.
        let mut words = vec![0x8100_0203, 0xbe81_0384];
        words.extend(load_and_fetch);
        let refused = fragment(&words).expect_err("refused").to_string();
        assert!(refused.contains("not traced"), "{refused}");
    }

    /// SuperTuxKart's compute image copy, as ACO compiled it for radeonsi in the guest: it fetches
    /// from the image whose descriptor is user data `s[8:15]`, and stores to the one it loads from
    /// `s[3] | 4 << 32` at `0x3c0`. It translates as a compute module, and says where each image's
    /// descriptor is: one in the user data, one in the table.
    #[test]
    fn a_compute_image_copy_names_where_both_its_images_are() {
        use crate::wavefront::{
            ComputeInputs, MeshPrimitive, Stage, TableBase, TableWord, UserData,
        };
        let (table, operands) = tables();
        let copy = [
            0xbe80_0303,
            0xbe81_0384,
            0xf40c_0600,
            0xfa00_03c0,
            0x8700_ff04,
            0x0000_03ff,
            0x9384_ff04,
            0x000a_000a,
            0x9910_1110,
            0x7e00_02f9,
            0x0004_1501,
            0x9900_0400,
            0xcc09_4000,
            0x1c00_0010,
            0xd714_1002,
            0x0202_0081,
            0xd703_0004,
            0x0202_0005,
            0xd703_0803,
            0x0202_0406,
            0xd703_0002,
            0x0202_0406,
            0xd711_0005,
            0x0202_0504,
            0xd703_4004,
            0x0201_0302,
            0xbfa1_0001,
            0xf000_1f08,
            0xc002_0605,
            0xf000_1f08,
            0xc002_0404,
            0xd703_0801,
            0x0202_0005,
            0xd711_0008,
            0x0202_0701,
            0xd703_4001,
            0x0201_0303,
            0xbf8c_0070,
            0xf020_1f08,
            0xc006_0608,
            0xf020_1f08,
            0xc006_0401,
            0xbf81_0000,
        ];
        let compute = |words: &[u32]| {
            let decoded = decode(&stream(words), &table, &operands);
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::default(),
                },
                (Stage::Compute, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    count: 16,
                    compute: Some(ComputeInputs {
                        workgroup_ids: [true, true, false],
                        thread_id_components: 2,
                        threads: [8, 8, 1],
                        unwritten_user_data: 0,
                        partial: None,
                    }),
                    ..UserData::default()
                },
            )
        };
        let translated = compute(&copy).expect("translates");
        // The loaded descriptor's registers name the stored image and hold no value: a program
        // that reads one - here `s_mov_b32 s2, s24` after the load - is refused.
        let mut reads = copy.to_vec();
        reads.insert(4, 0xbe82_0318);
        let refused = compute(&reads).expect_err("refused").to_string();
        assert!(refused.contains("as a value"), "{refused}");
        let fetched = translated
            .textures
            .first()
            .copied()
            .expect("a fetched image");
        assert_eq!((fetched.user_data, fetched.table_offset), (Some(8), None));
        let stored = translated.storage.expect("a stored image");
        assert_eq!(
            (stored.user_data, stored.table_offset, stored.table),
            (
                None,
                Some(0x3c0),
                TableBase {
                    low: TableWord::UserData(3),
                    high: TableWord::Constant(4),
                }
            )
        );
    }

    /// An image load with sixteen-bit address and data (`a16 d16`, ACO's image copy) translates, an
    /// odd number of sixteen-bit components too; one returning a status word (`tfe`), whose
    /// registers are not modelled, is refused rather than read some other way.
    #[test]
    fn sixteen_bit_image_operands_translate_and_unmodelled_layouts_are_refused() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let fragment = |load: [u32; 2]| {
            let words = [
                // s_load_dwordx8 s[8:15], s[0:1], 0x400
                0xf40c_0200,
                0xfa00_0400,
                0xbf8c_c07f,
                load[0],
                load[1],
                // exp mrt0 v0, v1, v0, v1 done vm
                0xf800_180f,
                0x0100_0100,
                0xbf81_0000,
            ];
            let mut program = vec![0xbe80_0303, 0xbe81_0384];
            program.extend(words);
            let decoded = decode(&stream(&program), &table, &operands);
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::default(),
                },
                (Stage::Fragment, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    count: 4,
                    ..UserData::default()
                },
            )
        };
        // image_load v[0:1], v2, s[8:15] dmask:0xf dim:2D unorm a16 d16
        let load = [0xf000_1f08, 0xc002_0002];
        assert!(fragment(load).is_ok(), "{:?}", fragment(load).err());
        let status = fragment([load[0] | 1 << 16, load[1]]).expect_err("refused");
        assert!(status.to_string().contains("status word"), "{status}");
        // dmask:0x7 - three halves, the last register's high half kept.
        assert!(fragment([(load[0] & !0xf00) | 0x700, load[1]]).is_ok());
    }

    /// A modifier word is refused by name, before anything is translated, unless it is SDWA on an
    /// instruction whose SDWA form is translated: a DPP16 `v_mov_b32` and an SDWA `v_mul_f32` are
    /// refused, radeonsi's `v_lshlrev_b32_sdwa v2, 10, v0 src1_sel:WORD_1` is not.
    #[test]
    fn a_modifier_word_is_refused_by_name_unless_its_sdwa_form_is_translated() {
        let (table, operands) = tables();
        for words in [[0x7e00_02fa, 0x0000_00ff], [0x1000_00f9, 0x0006_0600]] {
            let decoded = decode(&stream(&words), &table, &operands);
            assert!(decoded.is_trustworthy());
            let refused = translate(&decoded, &table, Strategy::default())
                .expect_err("refused")
                .to_string();
            assert!(refused.contains("SDWA or DPP"), "{refused}");
        }
        let decoded = decode(&stream(&[0x3404_00f9, 0x0586_068a]), &table, &operands);
        assert!(translate(&decoded, &table, Strategy::default()).is_ok());
    }

    /// An instruction with no operand layout is refused, naming the instruction.
    #[test]
    fn an_instruction_with_no_operand_layout_is_refused() {
        let (table, operands) = tables();
        // MUBUF opcode 48, `buffer_atomic_swap`, whose layout nothing has recorded.
        let decoded = decode(&stream(&[0xE0C0_0000, 0x0000_0000]), &table, &operands);
        let untranslatable = decoded.instructions.iter().any(|i| !i.operands_decoded);
        assert!(untranslatable, "the fixture must include an unknown layout");
        let refused = translate(&decoded, &table, Strategy::default());
        assert!(matches!(
            refused,
            Err(TranslateError::OperandsUnknown { .. })
        ));
        // The refusal names what it reached, so a run report says which layout to record.
        let said = refused.expect_err("refused").to_string();
        assert!(said.contains("first word 0x"), "{said}");
        assert!(!said.contains("(, "), "a name: {said}");
    }
}
