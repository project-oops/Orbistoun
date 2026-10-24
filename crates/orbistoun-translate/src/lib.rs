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
pub mod evaluate;
pub mod lane_exact;
pub mod model;
pub mod modifiers;
pub mod predicated;
pub mod quick_lanes;
pub mod scratch;
pub mod user_data_reads;
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

    /// A compute dispatch's thread group is more than one wave, and one translated invocation is
    /// one wave; what its program shares between its waves is named, since that is what running
    /// them together has to keep.
    #[error(
        "a thread group of {threads} threads is {waves} waves, and one translated invocation is one wave; its program {} and {}",
        if *barrier { "waits at s_barrier" } else { "waits at no barrier" },
        if *local_share { "uses the local data share" } else { "uses no local data share" }
    )]
    WaveGroup {
        /// Threads in one group.
        threads: u32,
        /// Waves those threads are.
        waves: u32,
        /// Whether the program executes `s_barrier`.
        barrier: bool,
        /// Whether the program uses the local data share.
        local_share: bool,
    },

    /// A shader reads through more buffers than a stage binds (D733): how many it traced, so a
    /// report says how far past the binding it is, and how many of them are descriptors or bases
    /// it computes, each its own buffer unless it reads what an earlier access read (D952).
    #[error(
        "the shader reads through {traced} buffers, {computed} of them computed, more than the {most} a stage binds (D733)"
    )]
    TooManyBuffers {
        /// The buffers its accesses were traced to.
        traced: usize,
        /// How many of them the program computes (D753).
        computed: usize,
        /// The most a stage binds.
        most: u32,
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

    /// An instruction the decoder named and this translator has no translation for, or none that
    /// holds where it sits - a guest dispatch's access to memory not yet checked against its
    /// window.
    ///
    /// The mnemonic is carried so a report names the instruction a shader needs, not only where
    /// it sits.
    #[error("instruction at {offset:#x} ({mnemonic}) cannot be translated: {detail}")]
    NotTranslated {
        /// Byte offset within the shader.
        offset: u32,
        /// The instruction's name, as the encoding table gives it.
        mnemonic: String,
        /// Why: [`model::NO_TRANSLATION`], or what keeps the translation from holding.
        detail: &'static str,
    },

    /// A guest dispatch's access to memory reads through no traced buffer, because a store in the
    /// program kept every buffer from being traced.
    #[error(
        "instruction at {offset:#x} ({mnemonic}) reads through no traced buffer: the program stores through {store} at {at:#x}, which is not a buffer store, so none of its buffers were traced"
    )]
    Untraced {
        /// Byte offset within the shader.
        offset: u32,
        /// The access's name.
        mnemonic: String,
        /// The store's byte offset.
        at: u32,
        /// The store's name.
        store: String,
    },

    /// An image access of a dimensionality not translated: every image translation reads two
    /// coordinates, and reading two of another image's would reach a place the guest did not name.
    #[error(
        "instruction at {offset:#x} ({mnemonic}) reaches a {dimension} image; only a two-dimensional image, a cube or a sampled volume is translated, and reading two coordinates of another would reach a place the guest did not name"
    )]
    ImageDimension {
        /// Byte offset within the shader.
        offset: u32,
        /// The access's name.
        mnemonic: String,
        /// The image's dimensionality, as its instruction encodes it.
        dimension: &'static str,
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

    /// A buffer load or store converts by its descriptor's format, and the draw's descriptors were
    /// given, but not the one in the slot it goes through: that slot's descriptor word was not
    /// resolved at the draw (D738).
    #[error(
        "instruction at {offset:#x} converts by the format of the descriptor in draw buffer slot {slot}, which this draw's descriptors did not resolve (D738)"
    )]
    UnresolvedBufferFormat {
        /// Byte offset within the shader.
        offset: u32,
        /// The draw buffer slot.
        slot: u32,
    },

    /// A pixel shader reads one attribute both interpolated and flat, which a host input cannot
    /// be, and the translation was not given the draw's flat twins (D742).
    #[error(
        "the shader reads an attribute both interpolated and flat, which takes a flat twin per draw (D742), and no draw's twins are given"
    )]
    NeedsFlatTwins,

    /// An instruction with no known operand layout was reached.
    #[error(
        "instruction at {offset:#x} ({name}, first word {word:#010x}{before}) has no operand layout; cannot translate what it operates on{}",
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
        /// What came before it and how long that was read as, for telling a missing layout from a
        /// word mistaken for an instruction after a misread length; empty at the program's start.
        before: String,
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
    /// came from. Empty for a module that samples nothing, and for the predicated lane and
    /// subgroup models; a guest dispatch run one invocation per lane is built by the wavefront
    /// model's emitter and has them (D880).
    pub textures: Vec<wavefront::TextureSource>,
    /// Where each of a compute module's stored images' descriptors came from, in the order it
    /// first stores to them (D893). Empty for a module that stores to no image, and for a pixel
    /// shader, whose storage image is the harness's.
    pub storage: Vec<wavefront::TextureSource>,
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
        let before = preceding(decode, first, encodings);
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
        before: preceding(decode, first, encodings),
        others,
    })
}

/// What came before `first`, and how long it was read as: a word mistaken for an instruction
/// follows one whose length was misread. Empty at the program's start.
fn preceding(
    decode: &Decode,
    first: &orbistoun_shader::Instruction,
    encodings: &EncodingTable,
) -> String {
    decode
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

/// The first instruction whose SDWA or DPP modifier word is not translated, refused by name with
/// its modifier and that word, so the form to translate next is plain.
fn refused_modifier(decode: &Decode, encodings: &EncodingTable) -> Option<TranslateError> {
    for instruction in &decode.instructions {
        if let Some(marker) = modifier_of(instruction, encodings)
            && !model::sdwa_translated(marker, instruction, encodings)
            && !model::dpp_translated(marker, instruction, encodings)
        {
            let family = instruction
                .encoding
                .and_then(|e| encodings.encodings().get(usize::from(e)))
                .map_or("", |e| e.name.as_str());
            let name = encodings
                .mnemonic_for(family, instruction.opcode)
                .unwrap_or("an unnamed instruction");
            return Some(TranslateError::NotTranslated {
                offset: instruction.offset,
                mnemonic: format!(
                    "{name} with {} word {:#010x}",
                    // Operand code 0xf9 selects SDWA; 0xfa, 0xe9 and 0xea the DPP forms.
                    if marker == 0xF9 { "SDWA" } else { "DPP" },
                    instruction.second_word.unwrap_or(0)
                ),
                detail: concat!(
                    "the instruction carries an SDWA or DPP modifier word, which selects parts ",
                    "of its operands or other lanes' values; only SDWA on the integer ",
                    "instructions `model::SDWA_INTEGER` names, and a quad permute of a one- or ",
                    "two-source vector instruction's first source, are translated"
                ),
            });
        }
    }
    None
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
/// graphics stage, a guest dispatch whose program meets its wave (D880), or a shader that touches
/// a lane mask. `dispatch` is why a guest dispatch kept the whole wave in one invocation, and
/// `per_lane` whether it runs one invocation per lane instead, which warns of nothing: a program
/// that reads a mask whole runs so too (D884).
fn fidelity_warnings(
    (staged, dispatch, per_lane): (bool, Option<&'static str>, bool),
    asked_for: Fidelity,
    fidelity: Fidelity,
    width: Width,
) -> Vec<Warning> {
    // Said as a warning rather than left in a field: the wavefront model costs a factor of
    // sixty-four.
    if staged && asked_for != Fidelity::Wavefront {
        vec![Warning::SlowestFidelity {
            because: concat!(
                "the module is a primitive shader, and the wavefront model is the only one ",
                "with its vertex inputs and mesh outputs"
            ),
            subgroup_would_need: width.lanes(),
        }]
    } else if let Some(because) = dispatch.filter(|_| asked_for != Fidelity::Wavefront) {
        vec![Warning::SlowestFidelity {
            because,
            subgroup_would_need: width.lanes(),
        }]
    } else if asked_for == Fidelity::Auto && fidelity == Fidelity::Wavefront && !per_lane {
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

/// Whether a guest dispatch runs one invocation per lane (D880): asked for the lane model or left to
/// choose, and its program never meets its wave, or meets it only through masks its lanes can
/// exchange (D884) on the host subgroup its caller gave as its `per_invocation`. When it does not,
/// why the whole wave stays in one invocation. Neither for a module that is not a guest dispatch,
/// `None` here. A dispatch's `per_invocation` is left the subgroup it runs per lane on, zero where
/// that is not known, or `None` where it keeps the whole wave.
fn dispatch_per_lane(
    decode: &Decode,
    encodings: &EncodingTable,
    (asked_for, width): (Fidelity, Width),
    user_data: Option<&mut wavefront::UserData>,
) -> (bool, Option<&'static str>) {
    let Some(user_data) = user_data else {
        return (false, None);
    };
    let subgroup = user_data.per_invocation.filter(|&lanes| lanes != 0);
    user_data.per_invocation = None;
    let Some(inputs) = user_data.compute else {
        return (false, None);
    };
    let needs = lane_exact::lane_needs(decode, encodings, width.lanes());
    let lanes = width.lanes();
    match lane_exact::lane_masks(&needs, lanes, subgroup, inputs.waves(lanes)) {
        Ok(_) if matches!(asked_for, Fidelity::Auto | Fidelity::Lane) => {
            user_data.per_invocation = Some(subgroup.unwrap_or(0));
            (true, None)
        }
        Ok(_) => (
            false,
            Some(concat!(
                "the module is a guest dispatch, and the subgroup model has none of its entry ",
                "state or exact memory"
            )),
        ),
        Err(because) => (false, Some(because)),
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
    // What the shader itself needs, before a stage or a dispatch decides for it: only this says
    // whether it reads or writes a lane mask.
    let needed = fidelity;
    let staged = stage != wavefront::Stage::Compute;
    // A guest dispatch's entry state and exact memory are the wavefront model's. Its emitter runs
    // one invocation per lane for a program that never meets its wave, which is the lane model a
    // dispatch has (D880), or only through its masks (D884); any other keeps the whole wave.
    let dispatch = !staged && user_data.compute.is_some();
    let mut user_data = user_data;
    let held = dispatch.then_some(&mut user_data);
    let (per_lane, kept_whole) = dispatch_per_lane(decode, encodings, (asked_for, width), held);
    if per_lane {
        fidelity = Fidelity::Lane;
    } else if staged || dispatch {
        fidelity = Fidelity::Wavefront;
    }

    // A fragment module simulates one lane, its pixel, so only a mesh module pays for the whole
    // wavefront in one invocation.
    let whole = staged && stage != wavefront::Stage::Fragment;
    let warnings = fidelity_warnings((whole, kept_whole, per_lane), asked_for, needed, width);

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
    if let Some(refusal) = refused_modifier(decode, encodings) {
        return Err(refusal);
    }

    match fidelity {
        // The wavefront model's emitter, which also builds a guest dispatch run one invocation per
        // lane (D880).
        _ if per_lane || fidelity == Fidelity::Wavefront => {
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
                storage: Vec::new(),
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
                storage: Vec::new(),
            })
        }
        // The wavefront model is built above, and `resolve` turns Auto into a concrete level, so
        // this is unreachable.
        Fidelity::Wavefront | Fidelity::Auto => Err(TranslateError::FidelityNotImplemented {
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
                    input_controls: None,
                    compute: None,
                    geometry: None,
                    window_space: crate::wavefront::WindowSpace::Clip,
                    per_invocation: None,
                    draw_buffers: false,
                    buffer_formats: None,
                    flat_twins: None,
                    discard_vertex_exports: false,
                    capture_positions: false,
                    position_vectors: None,
                    point_size: None,
                    saturated: [[false; 2]; crate::wavefront::MAX_TEXTURES],
                    mirrored: [[false; 2]; crate::wavefront::MAX_TEXTURES],
                    compared: [0; crate::wavefront::MAX_TEXTURES],
                    local_words: None,
                    program_address: None,
                    integer_targets: 0,
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

    /// A fragment module simulates one lane, the pixel it is invoked for (`simulated_lanes`), so
    /// it is not the sixty-four-lane cost the slowest-fidelity warning describes, and says nothing.
    #[test]
    fn a_fragment_module_is_not_warned_as_the_slowest_fidelity() {
        let (table, operands) = tables();
        let decoded = decode(&stream(TRIVIAL), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::default(),
            (
                super::wavefront::Stage::Fragment,
                super::wavefront::MeshPrimitive::default(),
            ),
            Window::default(),
            super::wavefront::UserData::default(),
        )
        .expect("translates");
        assert!(translated.warnings.is_empty(), "{:?}", translated.warnings);
    }

    /// A guest dispatch whose program reads a mask whole runs one invocation per lane where its
    /// lanes can exchange the mask, and says nothing; a group of several waves on a narrower
    /// subgroup keeps the whole wave and says why (D884). `s_cbranch_vccz +0` reads `vcc` whole.
    #[test]
    fn a_dispatch_reading_a_mask_whole_warns_only_where_it_keeps_the_wave() {
        let (table, operands) = tables();
        let decoded = decode(&stream(&[0xBF86_0000, TRIVIAL[0]]), &table, &operands);
        let dispatch = |threads| {
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::default(),
                (
                    super::wavefront::Stage::Compute,
                    super::wavefront::MeshPrimitive::default(),
                ),
                Window::default(),
                super::wavefront::UserData {
                    compute: Some(super::wavefront::ComputeInputs {
                        thread_id_components: 1,
                        threads: [threads, 1, 1],
                        ..super::wavefront::ComputeInputs::default()
                    }),
                    per_invocation: Some(32),
                    ..super::wavefront::UserData::default()
                },
            )
            .expect("translates")
        };
        let one_wave = dispatch(64);
        assert_eq!(one_wave.fidelity, Fidelity::Lane);
        assert!(one_wave.warnings.is_empty(), "{:?}", one_wave.warnings);
        let two_waves = dispatch(128);
        assert_eq!(two_waves.fidelity, Fidelity::Wavefront);
        assert!(
            two_waves
                .warnings
                .iter()
                .any(|w| w.to_string().contains("D884")),
            "{:?}",
            two_waves.warnings
        );
    }

    /// A colour target the draw holds integers in (`integer_targets` bit `K` for `mrtK`) is a
    /// `uvec4` output, which an attachment of an integer format takes; any other is a `vec4`
    /// (D851). `exp mrt0 v0, v1, v2, v3 done` into each.
    #[test]
    fn an_integer_colour_target_is_an_unsigned_output() {
        const EXP_MRT0: [u32; 3] = [0xF800_080F, 0x0302_0100, 0xBF81_0000];
        let (table, operands) = tables();
        let decoded = decode(&stream(&EXP_MRT0), &table, &operands);
        let translate_for = |integer_targets| {
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::default(),
                (
                    super::wavefront::Stage::Fragment,
                    super::wavefront::MeshPrimitive::default(),
                ),
                Window::default(),
                super::wavefront::UserData {
                    integer_targets,
                    ..super::wavefront::UserData::default()
                },
            )
            .expect("translates")
            .module
        };
        // Whether the module has an `Output` variable of a four-component vector of `u32`.
        let unsigned_output = |module: &[u32]| {
            const TYPE_INT: u32 = 21;
            const TYPE_VECTOR: u32 = 23;
            const TYPE_POINTER: u32 = 32;
            const VARIABLE: u32 = 59;
            const OUTPUT: u32 = 3;
            let mut at = 5;
            let (mut u32s, mut uvec4s, mut pointers) = (Vec::new(), Vec::new(), Vec::new());
            let mut found = false;
            while at < module.len() {
                let (count, opcode) = ((module[at] >> 16) as usize, module[at] & 0xFFFF);
                let operands = &module[at + 1..at + count.max(1)];
                match (opcode, operands) {
                    (TYPE_INT, [id, 32, 0]) => u32s.push(*id),
                    (TYPE_VECTOR, [id, of, 4]) if u32s.contains(of) => uvec4s.push(*id),
                    (TYPE_POINTER, [id, OUTPUT, to]) if uvec4s.contains(to) => pointers.push(*id),
                    (VARIABLE, [ty, _, OUTPUT, ..]) if pointers.contains(ty) => found = true,
                    _ => {}
                }
                at += count.max(1);
            }
            found
        };
        assert!(unsigned_output(&translate_for(1)), "mrt0 holds integers");
        assert!(!unsigned_output(&translate_for(0)), "mrt0 holds floats");
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

    /// A primitive shader fetches its images at the last slots, which a pixel shader's leave free,
    /// and a sample through a sampler there is still refused: Grand Theft Auto V's primitive shader
    /// at `0x74005ab83600` does `image_load v6, v10, s[8:15]` (words 0xf0000108 0x0002060a).
    #[test]
    fn a_primitive_shader_fetches_at_its_own_slots() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        // v_mov_b32 v10, 0; v_mov_b32 v7, 0; v_mov_b32 v11, 0; image_load v6, v10, s[8:15]
        // dmask:0x1; image_load v9, [v7, v11], s[8:15] dmask:0x1 - its third word the address
        // registers after the first - then s_endpgm. Both read the user data the stage was seeded
        // with at `s8`, its first word: one texture, which an unresolved first fetch made two.
        let words = [
            0x7E14_0280,
            0x7E0E_0280,
            0x7E16_0280,
            0xF000_0108,
            0x0002_060A,
            0xF000_010A,
            0x0002_0907,
            0x0000_000B,
            0xBF81_0000,
        ];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Mesh, MeshPrimitive::default()),
            Window::default(),
            UserData {
                first_register: 8,
                count: 16,
                ..UserData::default()
            },
        )
        .expect("translates");
        let first = orbistoun_spirv::PRIMITIVE_FIRST_TEXTURE as u32;
        assert_eq!(
            translated
                .textures
                .iter()
                .map(|texture| (texture.slot, texture.user_data))
                .collect::<Vec<_>>(),
            [(first, Some(0))]
        );
    }

    /// A pixel shader's buffer store is traced, as a dispatch's is, and translates through a
    /// writable buffer its draw writes back: Grand Theft Auto V's prologue pixel shaders store so,
    /// and a store once kept every one of a draw's buffers from being traced.
    #[test]
    fn a_pixel_shader_s_buffer_store_is_traced_and_translates() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        // v_mov_b32 v1, 7; buffer_store_dword v1, off, s[0:3], 0; s_endpgm.
        let words = [0x7E02_0287, 0xE070_0000, 0x8000_0100, 0xBF81_0000];
        let decoded = decode(&stream(&words), &table, &operands);
        let user_data = UserData {
            count: 4,
            draw_buffers: true,
            ..UserData::default()
        };
        let buffers =
            crate::wavefront::draw_buffers_for(&decoded, &table, Stage::Fragment, user_data)
                .expect("traced");
        assert_eq!(buffers.untraced_by, None, "a store keeps nothing untraced");
        assert!(!buffers.stored.is_empty(), "its store is recorded");
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            user_data,
        );
        assert!(translated.is_ok(), "{:?}", translated.err());
    }

    /// The short packed multiply-accumulate translates, as Grand Theft Auto V's scene compute program
    /// uses it (`v_pk_fmac_f16 v12, v46, v6`, word 0x78180d2e).
    #[test]
    fn a_packed_multiply_accumulate_translates() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        // v_pk_fmac_f16 v12, v46, v6; s_endpgm.
        let words = [0x7818_0D2E, 0xBF81_0000];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData::default(),
        );
        assert!(translated.is_ok(), "{:?}", translated.err());
    }

    /// A sixteen-bit SDWA multiply reads a named scalar's half as a register's: Grand Theft Auto V's
    /// scene compute program does `v_mul_f16_sdwa v16, vcc_lo, v17 dst_sel:WORD_1
    /// dst_unused:UNUSED_PRESERVE` (words 0x6a2022f9 0x0686156a).
    #[test]
    fn a_half_float_sdwa_form_reads_the_condition_mask() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let words = [0x6A20_22F9, 0x0686_156A, 0xBF81_0000];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData::default(),
        );
        assert!(translated.is_ok(), "{:?}", translated.err());
    }

    /// A primitive shader samples through a sampler at a named level, as Grand Theft Auto V's do
    /// with `image_sample_lz v0, v2, s[8:15], s[16:19]` (words 0xf09c0308 0x00820002), at its own
    /// slots; one sampling at the level derivatives choose is refused, having none (D904).
    #[test]
    fn a_primitive_shader_samples_at_a_named_level() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let mesh = |sample: [u32; 2]| {
            // v_mov_b32 v2, 0; v_mov_b32 v3, 0; the sample; s_endpgm.
            let words = [0x7E04_0280, 0x7E06_0280, sample[0], sample[1], 0xBF81_0000];
            let decoded = decode(&stream(&words), &table, &operands);
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::default(),
                },
                (Stage::Mesh, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    first_register: 8,
                    count: 16,
                    ..UserData::default()
                },
            )
        };
        let level_zero = mesh([0xF09C_0308, 0x0082_0002]).expect("a level-zero sample translates");
        assert_eq!(
            level_zero.textures.first().map(|texture| texture.slot),
            Some(orbistoun_spirv::PRIMITIVE_FIRST_TEXTURE as u32)
        );
        // The same sample at the level derivatives choose: `image_sample` (MIMG opcode 32).
        let implicit = (0xF09C_0308 & !(0x7F << 18)) | 32 << 18;
        assert!(
            mesh([implicit, 0x0082_0002]).is_err(),
            "no derivatives to choose a level from"
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

    /// A triangle mesh module reading per-draw user data declares one `uvec3`, shared by its
    /// indices and its `WorkgroupId` input. Two were invalid SPIR-V that NVIDIA's driver accepted
    /// and AMD's faulted compiling (a null read inside `amdvlk64.dll` on MDEM00001).
    #[test]
    fn a_triangle_mesh_reading_user_data_declares_one_uvec3() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        const END: u32 = 0xBF81_0000;
        const TYPE_VECTOR: u32 = 23;
        let (table, operands) = tables();
        let decoded = decode(&stream(&[END]), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Mesh, MeshPrimitive::Triangles),
            Window::default(),
            UserData {
                count: 4,
                ..UserData::default()
            },
        )
        .expect("a triangle mesh module with user data translates");

        // Walk the module's instructions after the five-word header, counting three-component
        // vectors.
        let words = &translated.module;
        let mut at = 5;
        let mut three_wide = 0;
        while at < words.len() {
            let (length, opcode) = ((words[at] >> 16) as usize, words[at] & 0xFFFF);
            if opcode == TYPE_VECTOR && words.get(at + 3) == Some(&3) {
                three_wide += 1;
            }
            at += length.max(1);
        }
        assert_eq!(three_wide, 1, "one uvec3, whoever asks for it");
    }

    /// A mesh module declares its output counts once, after the program has run, and writes every
    /// output after that: `SPV_EXT_mesh_shader` forbids a path to an output write that does not
    /// pass `OpSetMeshOutputsEXT`. The program runs as a loop over its blocks, so the allocation
    /// request and the exports inside it put the declaration on some paths only, which AMD's driver
    /// faulted compiling (a null read in `amdvlk64.dll`, MDEM00001) and NVIDIA's ran.
    #[test]
    fn a_mesh_module_declares_its_counts_before_any_output_is_written() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        const END: u32 = 0xBF81_0000;
        // Opcodes and the `Output` storage class, as the format numbers them.
        let (variable, access_chain, store, loop_merge, label, set_mesh_outputs, output) =
            (59, 65, 62, 246, 248, 5295, 3);
        let program = [
            // m0 = three vertices, one primitive; s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
            0xbefc_03ff,
            0x0000_1003,
            0xbf90_0009,
            // s_cbranch_scc0 +1 over an s_nop: a second block, so the program runs as a loop
            0xbf84_0001,
            0xbf80_0000,
            // v2, v3, v4 = 0; v12 = 1.0
            0x7e04_0280,
            0x7e06_0280,
            0x7e08_0280,
            0x7e18_02f2,
            // exp prim v1 = 0 | 1 << 10 | 2 << 20
            0x7e02_02ff,
            0x0020_0400,
            0xf800_0941,
            0x0000_0001,
            // exp pos0 v2, v3, v4, v12 done
            0xf800_08cf,
            0x0c04_0302,
            END,
        ];
        let (table, operands) = tables();
        let decoded = decode(&stream(&program), &table, &operands);
        let module = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Mesh, MeshPrimitive::Triangles),
            Window::default(),
            UserData::default(),
        )
        .expect("the primitive shader translates")
        .module;

        // Each instruction's position, opcode and operands, after the five-word header.
        let mut instructions = Vec::new();
        let mut at = 5;
        while at < module.len() {
            let length = ((module[at] >> 16) as usize).max(1);
            instructions.push((module[at] & 0xFFFF, &module[at + 1..at + length]));
            at += length;
        }
        let outputs: Vec<u32> = instructions
            .iter()
            .filter(|(opcode, words)| *opcode == variable && words.get(2) == Some(&output))
            .map(|(_, words)| words[1])
            .collect();
        let output_pointers: Vec<u32> = instructions
            .iter()
            .filter(|(opcode, words)| *opcode == access_chain && outputs.contains(&words[2]))
            .map(|(_, words)| words[1])
            .collect();
        let position = |wanted: &dyn Fn(u32, &[u32]) -> bool| -> Vec<usize> {
            (0..instructions.len())
                .filter(|&i| wanted(instructions[i].0, instructions[i].1))
                .collect()
        };
        let declared = position(&|opcode, _| opcode == set_mesh_outputs);
        let stores =
            position(&|opcode, words| opcode == store && output_pointers.contains(&words[0]));
        let loops = position(&|opcode, _| opcode == loop_merge);
        assert_eq!(declared.len(), 1, "one declaration of the counts");
        assert!(!stores.is_empty(), "the exports reach the outputs");
        assert!(
            !loops.is_empty(),
            "the fixture runs as a loop over its blocks"
        );
        // The loop's merge block: everything after it runs once, after the program.
        let merge = instructions[loops[0]].1[0];
        let after_loop = position(&|opcode, words| opcode == label && words[0] == merge)[0];
        assert!(
            declared[0] > after_loop,
            "the counts are declared inside the program's loop, on some paths only"
        );
        assert!(
            stores.iter().all(|&store| store > declared[0]),
            "an output is written before the counts are declared"
        );
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

    /// A compressed colour export may enable only some channels: Grand Theft Auto V's pixel shaders
    /// export `exp mrt4, v3, v0, v0, v0` compressed with only red and green enabled (words
    /// 0xf8000c43 0x00000003), the first packed register's two halves. Each enable bit is one
    /// channel, as in an uncompressed export, so it translates; here into `mrt0`.
    #[test]
    fn a_compressed_export_of_two_channels_translates() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        // exp mrt0 v3, v0 compr done vm, enabling red and green; s_endpgm.
        let words = [0xf800_1c03, 0x0000_0003, 0xbf81_0000];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData::default(),
        );
        assert!(translated.is_ok(), "{:?}", translated.err());
    }

    /// A pixel shader reading up to thirty-two textures from its descriptor table translates, each
    /// at its own slot and offset (D690): PPSA02664's samples three, PPSA03416's more than four,
    /// Grand Theft Auto V's shadow shaders more than sixteen. A thirty-third is refused by name,
    /// since thirty-two bindings exist. Each descriptor is loaded into
    /// the same registers just before its sample, as ACO reuses them.
    #[test]
    fn up_to_thirty_two_textures_from_the_table_translate_and_a_thirty_third_is_refused() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let fragment = |count: u32| {
            let mut words = Vec::new();
            for texture in 0..count {
                // s_load_dwordx8 s[8:15], s[0:1], 0x40 * n; s_waitcnt lgkmcnt(0)
                words.extend([0xf40c_0000 | (8 << 6), 0xfa00_0000 | (0x40 * texture)]);
                words.push(0xbf8c_c07f);
                // image_load_mip v[0:3], v[4:6], s[8:15] dmask:0xf dim:2D unorm
                words.extend([0xf004_1f08, (8 / 4) << 16 | 0x04]);
            }
            // exp mrt0 v0, v1, v2, v3 done vm
            words.extend([0xf800_180f, 0x0302_0100, 0xbf81_0000]);
            let decoded = decode(&stream(&words), &table, &operands);
            super::translate_with_user_data(
                &decoded,
                &table,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::default(),
                },
                (Stage::Fragment, MeshPrimitive::default()),
                Window::default(),
                UserData::default(),
            )
        };
        for count in [3, 4, 16, 17, 32] {
            let translated = fragment(count).expect("up to thirty-two textures translate");
            assert_eq!(
                translated
                    .textures
                    .iter()
                    .map(|t| (t.slot, t.table_offset))
                    .collect::<Vec<_>>(),
                (0..count).map(|n| (n, Some(0x40 * n))).collect::<Vec<_>>()
            );
        }
        let refused = fragment(33).expect_err("refused").to_string();
        assert!(
            refused.contains("more textures than its stage has bindings"),
            "{refused}"
        );
    }

    /// GTA V's intro video shader (`0x740043183c00`), as the title ships it.
    const GTA_V_VIDEO_SHADER: [u32; 72] = [
        0xbfa0_0003,
        0xbefc_031e,
        0xf408_080e,
        0xfa00_0020,
        0xbf8c_c07f,
        0xf420_1a90,
        0xfa00_0010,
        0xbe9e_047e,
        0xbefe_0a7e,
        0xc810_0400,
        0xc814_0500,
        0xc808_0600,
        0xc80c_0700,
        0xc820_0000,
        0xc811_0401,
        0xc815_0501,
        0xc809_0601,
        0xc80d_0701,
        0xc81c_0200,
        0xc818_0100,
        0xf080_0109,
        0x00c0_0904,
        0xc821_0001,
        0xf080_0109,
        0x00c2_0502,
        0xf080_0109,
        0x00c4_0402,
        0xc81d_0201,
        0xc819_0101,
        0xbf8c_c07f,
        0x7c09_00f9,
        0x8686_006a,
        0xbf8c_3f72,
        0x1012_12ff,
        0x3f95_0200,
        0xbf8c_3f71,
        0x4004_1305,
        0xbf50_4000,
        0x4006_1305,
        0x3fcc_4300,
        0xbf8c_3f70,
        0x400a_1304,
        0x4001_2400,
        0x4004_0504,
        0xbec8_6c00,
        0x0606_06ff,
        0xbf5e_e340,
        0x0608_0aff,
        0xbf8a_7420,
        0x060a_04ff,
        0x3f07_9ac0,
        0x1004_0708,
        0x100e_0907,
        0x1006_0b06,
        0x7e0a_0302,
        0x7e08_0307,
        0xbf86_0007,
        0xf40c_000e,
        0xfa00_0000,
        0xbf8c_c07f,
        0xf09c_0710,
        0x00c0_0502,
        0xbf8c_3f70,
        0x7e06_0306,
        0xc800_0300,
        0x5e04_0705,
        0xc801_0301,
        0x5e00_0107,
        0xbefe_041e,
        0xf800_1c0f,
        0x0000_0002,
        0xbf81_0000,
    ];

    /// GTA V's intro video shader samples its luma, red and blue planes through descriptors in user
    /// data `s[0:23]`, then loads a volume's descriptor from its table into `s[0:7]` and samples
    /// that: the registers were filled by a load no texture before came from, so it is a second
    /// texture, of its own shape, not the luma plane sampled again as a volume - which was refused,
    /// and with it every draw of the intro.
    #[test]
    fn registers_a_table_load_refills_name_another_texture() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let decoded = decode(&stream(&GTA_V_VIDEO_SHADER), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 31,
                ..UserData::default()
            },
        )
        .expect("the intro video's shader translates");
        let volumes: Vec<bool> = translated.textures.iter().map(|t| t.volume).collect();
        assert_eq!(
            volumes,
            [false, false, false, true],
            "three planes, then the volume"
        );
        assert_eq!(
            translated.textures[3].table_offset,
            Some(0),
            "loaded from its table"
        );
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
                        ..ComputeInputs::default()
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
        let stored = translated.storage.first().copied().expect("a stored image");
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

    /// A pixel shader that samples through a descriptor and a sampler still in the registers its
    /// user data seeded - never loaded from a table, never written - names both by their user-data
    /// words, as PPSA28061's does: the image in `s[0:7]` and the sampler in `s[8:11]`.
    #[test]
    fn a_pixel_shader_samples_through_its_user_data() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        // image_sample v[0:3], v[0:1], s[0:7], s[8:11] dmask:0xf dim:SQ_RSRC_IMG_2D; s_endpgm
        let words = [0xf080_0f08, 0x0040_0000, 0xbf81_0000];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 12,
                ..UserData::default()
            },
        )
        .expect("translates");
        let sampled = translated.textures.first().copied().expect("a texture");
        assert_eq!(
            (
                sampled.user_data,
                sampled.sampler_user_data,
                sampled.table_offset
            ),
            (Some(0), Some(8), None)
        );
    }

    /// A compare sample whose image is still in the user data and whose sampler was loaded from a
    /// table the user data names finds its sampler there: Grand Theft Auto V's shadow-sampling
    /// pixel shaders take the image from `s[16:23]` and load the sampler with `s_load_dwordx4 s80,
    /// s[28:29], 0x0`.
    #[test]
    fn a_user_data_image_finds_its_table_sampler() {
        use crate::wavefront::{MeshPrimitive, Stage, TableBase, TableWord, UserData};
        let (table, operands) = tables();
        let words = [
            0xf408_140e, // s_load_dwordx4 s[80:83], s[28:29], 0x0
            0xfa00_0000,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            // image_sample_c_lz v28, v[33:35], s[16:23], s[80:83] dmask:0x1
            0xf0bc_010a,
            0x0284_1c21,
            0xbf81_0000,
        ];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 30,
                ..UserData::default()
            },
        )
        .expect("translates");
        let sampled = translated.textures.first().copied().expect("a texture");
        assert_eq!(
            (
                sampled.user_data,
                sampled.sampler_offset,
                sampled.table,
                sampled.compare
            ),
            (
                Some(16),
                Some(0),
                TableBase {
                    low: TableWord::UserData(28),
                    high: TableWord::UserData(29),
                },
                true
            )
        );
    }

    /// A sampler register used for another descriptor first and loaded again as the sampler before
    /// the sample is the sampler that load put there: Grand Theft Auto V's shadow-sampling pixel
    /// shaders load a constant buffer into `s[88:91]` from the table's 0x60, read it, then load the
    /// sampler into the same registers from its 0x0.
    #[test]
    fn a_reloaded_sampler_register_is_the_last_load() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let words = [
            0xf408_160e, // s_load_dwordx4 s[88:91], s[28:29], 0x60
            0xfa00_0060,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            0xf420_002c, // s_buffer_load_dword s0, s[88:91], 0x0
            0xfa00_0000,
            0xf408_160e, // s_load_dwordx4 s[88:91], s[28:29], 0x0
            0xfa00_0000,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            // image_sample_c_lz v27, v[14:16], s[16:23], s[88:91] dmask:0x1
            0xf0bc_010a,
            0x02c4_1b0e,
            0xbf81_0000,
        ];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 30,
                ..UserData::default()
            },
        )
        .expect("translates");
        let sampled = translated.textures.first().copied().expect("a texture");
        assert_eq!(
            (sampled.user_data, sampled.sampler_offset),
            (Some(16), Some(0))
        );
    }

    /// A sampler whose registers held other values first, loaded across a loop from the sample and
    /// overwritten after it, is still the load that reached the sample: Grand Theft Auto V's shadow
    /// shader at `0x74005ad10e00` fills `s[32:47]` from a buffer, loads its sampler into `s[40:43]`
    /// from the table's 0x60 and its image into `s[56:63]` from 0x0, loops, compares, and loads over
    /// `s[32:47]` again.
    #[test]
    fn a_sampler_loaded_over_values_across_a_loop_is_traced() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let words = [
            0xf408_050e, // s_load_dwordx4 s[20:23], s[28:29], 0x40
            0xfa00_0040,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            0xf430_080a, // s_buffer_load_dwordx16 s[32:47], s[20:23], 0x0
            0xfa00_0000,
            0xf408_0a0e, // s_load_dwordx4 s[40:43], s[28:29], 0x60
            0xfa00_0060,
            0xf40c_0e0e, // s_load_dwordx8 s[56:63], s[28:29], 0x0
            0xfa00_0000,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            0x8000_8100, // s_add_u32 s0, s0, 1
            0xbf0a_8400, // s_cmp_lt_u32 s0, 4
            0xbf85_fffd, // s_cbranch_scc1 back to the add
            // image_sample_c_lz v52, [v12, v13, v14], s[56:63], s[40:43] dmask:0x1, its third
            // word the non-sequential addresses
            0xf0bc_010a,
            0x014e_340c,
            0x0000_0e0d,
            0xf408_0e0e, // s_load_dwordx4 s[56:59], s[28:29], 0xa0
            0xfa00_00a0,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            0xf430_081c, // s_buffer_load_dwordx16 s[32:47], s[56:59], 0x2b0
            0xfa00_02b0,
            0xbf81_0000,
        ];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Lane,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 30,
                ..UserData::default()
            },
        )
        .expect("translates");
        let sampled = translated.textures.first().copied().expect("a texture");
        assert_eq!(
            (sampled.table_offset, sampled.sampler_offset),
            (Some(0), Some(0x60))
        );
    }

    /// A sampler loaded more than once is still traced across a loop between its last load and the
    /// sample, where the loop's body leaves its registers alone: Grand Theft Auto V's shadow pixel
    /// shaders reload their compare sampler, then run a waterfall loop over a vector of
    /// descriptors before sampling.
    #[test]
    fn a_reloaded_sampler_is_traced_across_a_loop_that_leaves_it() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let words = [
            0xf408_0a0e, // s_load_dwordx4 s[40:43], s[28:29], 0x20
            0xfa00_0020,
            0xf40c_0e0e, // s_load_dwordx8 s[56:63], s[28:29], 0x0
            0xfa00_0000,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            0xf408_0a0e, // s_load_dwordx4 s[40:43], s[28:29], 0x60
            0xfa00_0060,
            0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
            0x8000_8100, // s_add_u32 s0, s0, 1
            0xbf0a_8400, // s_cmp_lt_u32 s0, 4
            0xbf85_fffd, // s_cbranch_scc1 back to the add
            // image_sample_c_lz v52, [v12, v13, v14], s[56:63], s[40:43] dmask:0x1
            0xf0bc_010a,
            0x014e_340c,
            0x0000_0e0d,
            0xbf81_0000,
        ];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Lane,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 30,
                ..UserData::default()
            },
        )
        .expect("translates");
        let offsets: Vec<_> = translated
            .textures
            .iter()
            .map(|t| (t.table_offset, t.sampler_offset))
            .collect();
        assert!(
            offsets.contains(&(Some(0), Some(0x60))),
            "the sample after the loop reads the sampler last loaded: {offsets:?}"
        );
    }

    /// A descriptor the program copied out of its user data before sampling is still read from
    /// there: PPSA28061's notice-screen pixel shader moves its image descriptor from `s[6:13]` to
    /// `s[20:27]` and its sampler from `s[14:17]` to `s[8:11]` with `s_mov_b64`, then samples.
    #[test]
    fn a_descriptor_moved_out_of_the_user_data_is_read_from_there() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData};
        let (table, operands) = tables();
        let words = [
            0xbe94_0406, // s_mov_b64 s[20:21], s[6:7]
            0xbe96_0408, // s_mov_b64 s[22:23], s[8:9]
            0xbe98_040a, // s_mov_b64 s[24:25], s[10:11]
            0xbe9a_040c, // s_mov_b64 s[26:27], s[12:13]
            0xbe88_040e, // s_mov_b64 s[8:9], s[14:15]
            0xbe8a_0410, // s_mov_b64 s[10:11], s[16:17]
            // image_sample v0, v2, s[20:27], s[8:11] dmask:0x7
            0xf080_0708,
            0x0045_0002,
            0xbf81_0000,
        ];
        let decoded = decode(&stream(&words), &table, &operands);
        let translated = super::translate_with_user_data(
            &decoded,
            &table,
            Strategy::Predicated {
                fidelity: Fidelity::Wavefront,
                width: Width::default(),
            },
            (Stage::Fragment, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 18,
                ..UserData::default()
            },
        )
        .expect("translates");
        let sampled = translated.textures.first().copied().expect("a texture");
        assert_eq!(
            (
                sampled.user_data,
                sampled.sampler_user_data,
                sampled.table_offset
            ),
            (Some(6), Some(14), None)
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
    /// instruction whose SDWA form is translated: a DPP16 `v_mov_b32` is refused, radeonsi's
    /// `v_lshlrev_b32_sdwa v2, 10, v0 src1_sel:WORD_1` is not. A float SDWA `v_mul_f32` is
    /// translated over whole registers, and refused where it selects a byte of one.
    #[test]
    fn a_modifier_word_is_refused_by_name_unless_its_sdwa_form_is_translated() {
        let (table, operands) = tables();
        let outcome = |words: [u32; 2]| {
            let decoded = decode(&stream(&words), &table, &operands);
            assert!(decoded.is_trustworthy());
            translate(&decoded, &table, Strategy::default()).map_err(|e| e.to_string())
        };
        // A DPP word whose control, 0x100, the instruction set leaves reserved.
        let dpp = outcome([0x7e00_02fa, 0xff01_00ff]).expect_err("DPP refused");
        assert!(dpp.contains("SDWA or DPP"), "{dpp}");
        let byte = outcome([0x1000_00f9, 0x0006_0600]).expect_err("a byte of a float refused");
        assert!(byte.contains("selects part of a register"), "{byte}");
        assert!(
            outcome([0x1000_00f9, 0x0606_0600]).is_ok(),
            "a whole-register product"
        );
        assert!(outcome([0x3404_00f9, 0x0586_068a]).is_ok());
    }

    /// A pixel shader reading attribute 0's `x` interpolated and its `y` flat, as Mesa packs a value
    /// the same at every vertex beside interpolated ones (`nir_opt_varyings.c`), is refused without
    /// the draw's flat twins, and with them declares attribute 0 smooth and its twin flat (D742).
    #[test]
    fn an_attribute_read_both_ways_takes_a_flat_twin() {
        use crate::wavefront::{FlatTwins, MeshPrimitive, Stage, UserData};
        use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // VINTRP: v_interp_p1_f32 v2, v0, attr0.x; v_interp_p2_f32 v2, v1, attr0.x;
        // v_interp_mov_f32 v3, p0, attr0.y; s_endpgm.
        let words: [u32; 4] = [0xC808_0000, 0xC809_0001, 0xC80E_0102, 0xBF81_0000];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        assert_eq!(
            crate::wavefront::mixed_attributes(&decode, &encodings),
            [0],
            "attribute 0 is read both ways"
        );
        let translate = |flat_twins| {
            super::translate_with_user_data(
                &decode,
                &encodings,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::default(),
                },
                (Stage::Fragment, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    flat_twins,
                    ..UserData::default()
                },
            )
        };
        assert!(matches!(
            translate(None),
            Err(TranslateError::NeedsFlatTwins)
        ));
        let module = translate(FlatTwins::from_first(&[0], 1))
            .expect("translated with a twin")
            .module;
        // `OpDecorate` (71): a `Location` (30) of 1, and the same variable `Flat` (14).
        let decorations: Vec<&[u32]> = {
            let mut at = 5;
            let mut found = Vec::new();
            while at < module.len() {
                let count = (module[at] >> 16) as usize;
                if module[at] & 0xffff == 71 {
                    found.push(&module[at + 1..at + count]);
                }
                at += count.max(1);
            }
            found
        };
        let twin = decorations
            .iter()
            .find(|d| d[1..] == [30, 1])
            .map(|d| d[0])
            .expect("an input at location 1");
        assert!(
            decorations.iter().any(|d| d[..] == [twin, 14]),
            "it is flat"
        );
        let own = decorations
            .iter()
            .find(|d| d[1..] == [30, 0])
            .map(|d| d[0])
            .expect("an input at location 0");
        assert!(
            !decorations.iter().any(|d| d[..] == [own, 14]),
            "it is smooth"
        );
    }

    /// An instruction with no operand layout is refused, naming the instruction.
    #[test]
    fn an_instruction_with_no_operand_layout_is_refused() {
        let (table, operands) = tables();
        // MUBUF opcode 81, `buffer_atomic_cmpswap_x2`, whose layout nothing has recorded.
        let decoded = decode(&stream(&[0xE144_0000, 0x0000_0000]), &table, &operands);
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
        // And what came before it, so a word mistaken for an instruction after a misread length
        // shows as one.
        let decoded = decode(
            &stream(&[0xBF80_0000, 0xE144_0000, 0x0000_0000]),
            &table,
            &operands,
        );
        let said = translate(&decoded, &table, Strategy::default())
            .expect_err("refused")
            .to_string();
        assert!(
            said.contains("after s_nop at 0x0 read as 4 bytes"),
            "{said}"
        );
    }
}
