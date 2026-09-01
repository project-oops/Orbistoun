//! Turning decoded guest shaders into SPIR-V.
//!
//! # The mismatch this crate exists to bridge
//!
//! The guest architecture runs sixty-four lanes in lockstep under an explicit
//! execution mask. There is no `if` in its machine code - a branch is mask arithmetic
//! followed by a jump taken when no lane survives. Structure is *implied*.
//!
//! SPIR-V is the opposite. It describes one invocation, and it demands **structured**
//! control flow: explicit merge blocks forming a reducible graph, with the hardware
//! handling divergence.
//!
//! # Two axes, not one
//!
//! [`Strategy`] chooses how *control flow* is expressed. [`Fidelity`] chooses how the
//! *wavefront* is modelled. They are separate questions and were conflated at first.
//!
//! Fidelity is a field of [`Strategy::Predicated`] rather than a parameter beside it,
//! because the combinations are not free: structured reconstruction implies the
//! per-lane model and nothing else. Making it a field means an invalid pairing cannot
//! be written down, which is better than rejecting one at run time.
//!
//! # Why three fidelity levels rather than one
//!
//! Not as a fallback ladder. As a **differential oracle**.
//!
//! Run one shader at [`Fidelity::Wavefront`] and at [`Fidelity::Lane`]. If they
//! disagree, the faster one has a bug, and it is localised to that shader and
//! bisectable to an instruction - with no reference hardware, no console and no title
//! involved. That is the same trick the decoder's differential test plays, one layer
//! up, and it is worth more than either level alone.
//!
//! # What is built
//!
//! | Level | Model | State |
//! |---|---|---|
//! | [`Fidelity::Lane`] | one invocation per lane; lanes never interact | built |
//! | [`Fidelity::Wavefront`] | one invocation simulates all lanes; mask is a value | built |
//! | [`Fidelity::Subgroup`] | one invocation per lane; mask via subgroup ballot | **stub** |
//!
//! Every unbuilt path is an error naming what it would do, never a quiet substitution.
//! A silent fallback would present as unexplained slowness or - worse, since these
//! differ in *correctness* rather than only speed - as output that is subtly wrong with
//! nothing to point at. Principle 3, applied to a subsystem. (D098)
//!
//! # The execution mask is where the levels stop being interchangeable
//!
//! [`Fidelity::Wavefront`] holds the mask as an ordinary value in two scalar registers,
//! so the guest's own mask arithmetic - `s_mov_b64 exec, …`, `s_and_b64 exec, exec, …`,
//! `s_andn2_b64` for an else-branch - translates directly rather than being
//! reconstructed. Every vector write and every store then selects on the lane's bit.
//!
//! [`Fidelity::Lane`] has no mask and **refuses** any shader that writes one. It cannot
//! represent an inactive lane, so ignoring the write would run every lane regardless:
//! plausible output, wrong answer, nothing in it to indicate the problem. That refusal
//! is what makes [`Fidelity::Auto`] able to choose - it picks the wavefront model for a
//! shader that touches the mask and the lane model otherwise, rather than defaulting to
//! one and hoping.
//!
//! What is still missing is **branching**. The mask can be computed and honoured; a
//! jump taken when no lane survives it cannot yet be expressed, because SPIR-V demands
//! structured control flow and the guest's is implied.

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
/// The guest executes sixty-four lanes in lockstep. SPIR-V describes one invocation.
/// This is the choice of how to reconcile those, and the levels differ in
/// **correctness**, not only in speed - which is why picking one silently would be
/// worse than refusing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fidelity {
    /// Pick the cheapest level valid for this shader on this machine.
    #[default]
    Auto,

    /// One invocation per lane, and lanes never interact.
    ///
    /// The execution mask is implicit and cross-lane instructions are impossible.
    /// Correct only for shaders that never look at the wavefront as an object - which
    /// is a real and useful subset, and not most shaders.
    Lane,

    /// One invocation per lane, with the mask materialised by subgroup ballot.
    ///
    /// Cross-lane instructions become subgroup operations. Correct and fast **when the
    /// hardware's subgroup size matches the guest's wavefront**, which is not
    /// guaranteed: the guest is sixty-four wide and some hardware is thirty-two.
    Subgroup,

    /// One invocation simulates an entire wavefront.
    ///
    /// Registers are arrays indexed by lane and the execution mask is an ordinary
    /// value, so cross-lane instructions are array reads and the mask can be
    /// manipulated arithmetically exactly as the guest does.
    ///
    /// Very slow, and correct unconditionally - no subgroup dependency, no size to
    /// match, nothing to negotiate. That combination is what makes it the oracle the
    /// other two are judged against.
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
    /// Reconstructed structured control flow. **Not implemented.**
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
/// # Why this is the caller's to say
///
/// This architecture generation runs shaders at either width, chosen **per shader** when
/// it is compiled, and nothing in the instruction stream states which - the encodings are
/// identical either way (D141). A 32-lane shader is recognisable only by which mask
/// instructions it uses, and inferring from that would mean guessing from an absence for
/// any shader whose masks are all still untouched at the point of the guess.
///
/// So it is supplied. On a real target it comes from the pipeline state the guest set up
/// alongside the shader; here it comes from whoever is calling, and defaults to the wider
/// one because that is what the previous generation had and what every existing fixture
/// is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Width {
    /// Thirty-two lanes. The narrow mode this generation added.
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
    /// The narrow mode's mask fits in one, which is why its shaders use the 32-bit mask
    /// instructions rather than the 64-bit ones.
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
        "the {0} strategy is not implemented. It would reconstruct structured control \
         flow from execution-mask arithmetic; only the predicated strategy exists. \
         This is not falling back to it, because a silent substitution would present \
         as unexplained slowness rather than as a missing feature"
    )]
    StrategyNotImplemented(Strategy),

    /// A fidelity level that has not been built was asked for.
    ///
    /// Deliberately not a fallback, and for a sharper reason than the strategy above:
    /// the levels differ in *correctness*, so substituting one would produce output
    /// that is wrong rather than merely slow, with nothing to point at.
    #[error(
        "the {level} model is not implemented ({would}). This is not falling \
         back to another level, because the levels differ in correctness rather than \
         only in speed - a substitution would render something subtly wrong with \
         nothing to indicate it"
    )]
    FidelityNotImplemented {
        /// Which level.
        level: Fidelity,
        /// What it would have done, so the message says what is missing.
        would: &'static str,
    },

    /// The decode this was handed cannot be trusted.
    #[error(
        "refusing to translate an untrustworthy decode ({reason}) - the instruction \
         stream may not be what it appears to be"
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
    /// Never silently skipped. A shader missing one instruction computes the wrong
    /// thing while appearing to work, which is far harder to find than a translator
    /// that stops and names what it hit.
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
    /// A translator bug rather than anything about the guest, and caught here because
    /// the alternative is a driver fault with no diagnosis attached. Emitting the
    /// module anyway and letting the device decide has been tried twice; both times it
    /// cost hours and the answer came from `spirv-val` in the end.
    #[error("the translated module is malformed: {0}. This is a translator bug")]
    MalformedModule(#[from] orbistoun_spirv::ModuleError),
}

/// Something worth telling the caller about a translation that succeeded.
///
/// Distinct from [`TranslateError`], which is a translation that did not happen. These
/// all describe output that is *correct* and costs something the caller may not have
/// expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// [`Fidelity::Auto`] had to fall back to the slowest level.
    ///
    /// # Why this is said out loud
    ///
    /// The lane model has no execution mask, so a shader that turns lanes off cannot use
    /// it and `Auto` falls back to simulating a whole wavefront inside one invocation -
    /// sixty four lanes, in a loop, per instruction.
    ///
    /// That is correct and it is *enormously* slower, and until now the only sign was a
    /// field on the result. A field is something you have to know to look for; a warning
    /// is something you have to decide to ignore. One instruction touching `exec`
    /// anywhere in a shader is enough to trigger it, which is most real shaders, so the
    /// difference matters.
    SlowestFidelity {
        /// What in the shader forced it.
        because: &'static str,
        /// The width a subgroup would have to be for the faster masking level to work.
        ///
        /// Carried because it is the actionable part: [`Fidelity::Subgroup`] is as fast
        /// as the lane model *and* has a mask, and whether it fits is a property of the
        /// device rather than of the shader. The translator cannot check it - it has
        /// never seen the device - so it reports what would be needed and leaves the
        /// comparison to whoever knows.
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
                "translated at wavefront fidelity, which simulates every lane in one \
                 invocation and is far slower: {because}. Subgroup fidelity would do the \
                 same work with a mask if this device's subgroup is {subgroup_would_need} \
                 wide"
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
    /// Reported rather than inferred: "it is slow" and "it quietly dropped to a slower
    /// level" look identical from the outside, and only one of them is worth
    /// investigating.
    pub fidelity: Fidelity,
    /// Guest instructions translated.
    pub instructions: usize,
    /// Things the caller should be told rather than left to look for.
    ///
    /// Empty is the common case. The list exists because a fact recorded in a field is a
    /// fact somebody has to go and read, and the one thing here costs a factor of sixty
    /// four - see [`Warning`].
    pub warnings: Vec<Warning>,
    /// The host subgroup width this module needs, when it needs a particular one.
    ///
    /// Set only by [`Fidelity::Subgroup`], where one invocation is one guest lane and
    /// the two widths therefore have to match. Reported rather than checked here: the
    /// translator does not know what device this will run on, and inventing a default
    /// would produce a module that is silently wrong on half of them.
    pub required_subgroup: Option<u32>,
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
/// [`Fidelity::Wavefront`]. The lane model has one invocation per lane and no way to
/// represent an inactive one, so a shader that disables lanes would run every lane
/// regardless: plausible output, wrong answer, and nothing in it to indicate the
/// problem.
///
/// # This used to be safe by accident
///
/// The answer was previously always the lane model, on the reasoning that any shader
/// needing more would contain an instruction the translator refused anyway - so the
/// wrong level could not be chosen, not by analysis but because translation stopped
/// first. That reasoning expired the moment `s_mov_b64 exec, …` became translatable,
/// and the note saying so was written before it did.
///
/// # Why this is not the silent substitution D098 forbids
///
/// Because the caller asked for [`Fidelity::Auto`], which is a request to be told what
/// the shader needs. Asking for the lane model explicitly and getting it is not affected
/// - that shader is refused, loudly, by the model that cannot represent it.
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
                // Asked by name, because whether an instruction touches a lane mask is a
                // property of the instruction and not of the number this generation
                // happens to give it.
                //
                // An opcode with no recorded name answers `false` here, which picks the
                // lane model. That is deliberate: translation refuses the instruction
                // either way, and it refuses on the *name lookup*, so the report says
                // the opcode is unknown rather than blaming a fidelity that was never
                // the problem.
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

/// Translates a decoded shader.
///
/// Refuses rather than approximates. Every error here is a case where producing
/// *something* would mean inventing behaviour the guest did not ask for.
pub fn translate(
    decode: &Decode,
    encodings: &EncodingTable,
    strategy: Strategy,
) -> Result<Translated, TranslateError> {
    let Strategy::Predicated { fidelity, width } = strategy else {
        return Err(TranslateError::StrategyNotImplemented(strategy));
    };
    let asked_for = fidelity;
    let fidelity = resolve(fidelity, decode, encodings);

    // Said out loud rather than left in a field. `Auto` reaching the wavefront model is
    // the common case for any shader that masks, and it costs a factor of sixty four.
    let warnings = if asked_for == Fidelity::Auto && fidelity == Fidelity::Wavefront {
        vec![Warning::SlowestFidelity {
            because: "the shader reads or writes a lane mask, which the per-lane model                       cannot represent",
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
            let (module, instructions) = predicated::translate(decode, encodings)?;
            Ok(Translated {
                module,
                strategy,
                fidelity,
                instructions,
                warnings,
                required_subgroup: None,
            })
        }
        Fidelity::Wavefront => {
            let (module, instructions) = wavefront::translate(decode, encodings, width)?;
            Ok(Translated {
                module,
                strategy,
                fidelity,
                instructions,
                warnings,
                required_subgroup: None,
            })
        }
        Fidelity::Subgroup => {
            let (module, instructions, required_subgroup) =
                predicated::translate_subgroup(decode, encodings, width)?;
            Ok(Translated {
                module,
                strategy,
                fidelity,
                instructions,
                warnings,
                required_subgroup: Some(required_subgroup),
            })
        }
        // `resolve` turns Auto into something concrete, so reaching here would mean it
        // stopped doing that.
        Fidelity::Auto => Err(TranslateError::FidelityNotImplemented {
            level: fidelity,
            would: "have been resolved to a concrete level before dispatch",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{Fidelity, Strategy, TranslateError, Warning, Width, translate};
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

    #[test]
    fn asking_for_the_unbuilt_strategy_is_an_error_not_a_fallback() {
        let (table, operands) = tables();
        let decoded = decode(&stream(TRIVIAL), &table, &operands);
        let error = translate(&decoded, &table, Strategy::Structured).expect_err("must refuse");
        let text = error.to_string();
        assert!(text.contains("not implemented"), "got: {text}");
        assert!(text.contains("not falling back"), "got: {text}");
    }

    #[test]
    fn falling_back_to_the_slowest_level_is_a_warning_not_a_footnote() {
        // `Auto` picking the wavefront model costs a factor of sixty four, and one
        // instruction touching a lane mask anywhere in a shader is enough to trigger it -
        // which is most real shaders. It used to be recorded only in a field, and a field
        // is something a caller has to know to look for.
        //
        // The warning also carries the width a subgroup would need, because that is the
        // actionable part: the subgroup level is as fast as the lane model and has a mask,
        // and whether it fits is a property of the device rather than of the shader.
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

        // And a shader that does not need a mask says nothing, so the warning stays worth
        // reading rather than becoming noise every caller learns to skip.
        let quiet = decode(&stream(TRIVIAL), &table, &operands);
        let quiet = translate(&quiet, &table, Strategy::default()).expect("translates");
        assert_eq!(quiet.fidelity, Fidelity::Lane);
        assert!(quiet.warnings.is_empty(), "{:?}", quiet.warnings);
    }

    #[test]
    fn asking_for_automatic_fidelity_reports_the_level_it_chose() {
        // **Every level is built now**, so there is no longer an unimplemented one to
        // assert about - this test used to check that the subgroup level refused, and it
        // was the only thing doing so, which would have quietly required it to stay
        // unimplemented.
        //
        // What is still worth pinning is that `Auto` never escapes as itself. It is a
        // request to be told, and a caller that got `Auto` back would have learned
        // nothing about which semantics its shader actually ran under.
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

    #[test]
    fn auto_resolves_to_a_built_level_and_reports_which() {
        // A caller asking for `auto` must be able to find out what it got. Otherwise
        // "slow" and "silently dropped a level" are indistinguishable.
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
