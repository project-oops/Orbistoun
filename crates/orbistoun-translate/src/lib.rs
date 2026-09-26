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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

    /// An instruction with no known operand layout was reached.
    #[error(
        "instruction at {offset:#x} has no operand layout; cannot translate what it operates on"
    )]
    OperandsUnknown {
        /// Byte offset within the shader.
        offset: u32,
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
}

impl Translated {
    /// The module as bytes, for writing out or handing to a driver.
    pub fn bytes(&self) -> Vec<u8> {
        self.module.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
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
    if staged {
        fidelity = Fidelity::Wavefront;
    }

    // Said as a warning rather than left in a field: the wavefront model costs a factor of
    // sixty-four.
    let warnings = if staged && asked_for != Fidelity::Wavefront {
        vec![Warning::SlowestFidelity {
            because: concat!(
                "the module is for a graphics stage, and the wavefront model is the only one ",
                "with fragment inputs and a colour output"
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
    };

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

    for instruction in &decode.instructions {
        if !instruction.operands_decoded {
            return Err(TranslateError::OperandsUnknown {
                offset: instruction.offset,
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
            })
        }
        Fidelity::Wavefront => {
            let (module, instructions, textures) = wavefront::translate_with_user_data(
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
    /// does not and is unchanged; a stage wanting more than its sixteen words is refused
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
        assert!(with(17).is_err(), "more than a stage's share is refused");
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

    /// An instruction with no operand layout is refused.
    #[test]
    fn an_instruction_with_no_operand_layout_is_refused() {
        let (table, operands) = tables();
        let decoded = decode(&stream(&[0xE000_0000, 0x0000_0000]), &table, &operands);
        let untranslatable = decoded.instructions.iter().any(|i| !i.operands_decoded);
        assert!(untranslatable, "the fixture must include an unknown layout");
        assert!(matches!(
            translate(&decoded, &table, Strategy::default()),
            Err(TranslateError::OperandsUnknown { .. })
        ));
    }
}
