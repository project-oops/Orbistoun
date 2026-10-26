//! What each of a title's kept shader programs costs, measured without running the title.
//!
//! Every distinct translation a title's store keeps is made again from its recorded program and
//! context by this build's translator, and its module's size is set against the guest
//! instructions it came from. A caller with a device also has each module compiled and timed
//! ([`Program`]), and a caller with a validator has each module validated ([`Validate`]), so a
//! module a driver happens to accept is still found invalid. Rows are ranked by compile time and by
//! size, summed, and written as TSV, so two builds' censuses diff line for line.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{Stage, Window};
use orbistoun_translate::{Fidelity, Translated, Warning, translate_with_user_data};

use crate::translations::{Context, Kept, TranslationStore};

/// The most threads a census translates and compiles on: the machine it runs on is usually in use.
pub const MOST_THREADS: usize = 4;

/// How a module's compile was timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timed {
    /// The whole pipeline, laid out as a guest dispatch's is: what a dispatch waits for.
    Pipeline,
    /// The shader module alone: a draw's pipeline needs state the store does not keep.
    Module,
}

impl Timed {
    const fn name(self) -> &'static str {
        match self {
            Self::Pipeline => "pipeline",
            Self::Module => "module",
        }
    }
}

/// One module's compile.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// How long the driver took, in seconds.
    pub seconds: f64,
    /// What was compiled.
    pub timed: Timed,
    /// Why the driver refused it, where it did.
    pub failed: Option<String>,
}

/// Times a module's compile.
pub type Measure<'a> = dyn Fn(&Program<'_>) -> Compiled + Sync + 'a;

/// Validates a module: `None` where it is valid, else the validator's message.
pub type Validate<'a> = dyn Fn(&[u32]) -> Option<String> + Sync + 'a;

/// A translated module as a compile is asked of it.
#[derive(Debug, Clone, Copy)]
pub struct Program<'a> {
    /// The host stage it was translated for.
    pub stage: Stage,
    /// The SPIR-V module.
    pub module: &'a [u32],
    /// How many images it samples, each at its slot's binding.
    pub fetched: usize,
    /// How many images it stores to, each at its storage-image binding.
    pub stored: usize,
}

impl Program<'_> {
    /// What its compile can be timed as: a compute module's whole pipeline, a draw's module.
    pub fn timed(&self) -> Timed {
        if self.stage == Stage::Compute {
            Timed::Pipeline
        } else {
            Timed::Module
        }
    }
}

/// How a census translated a kept program: as it was kept, or as a run would prepare it on another
/// route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Route {
    /// Against the context the store kept.
    Kept,
    /// A compute dispatch kept host-traced, translated again device-addressed (D964), as a run
    /// prepares one whose ranges the host cannot know.
    DeviceAddressed,
}

impl Route {
    const fn name(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::DeviceAddressed => "device",
        }
    }
}

/// What a refusal is, so a census groups refusals that one fix answers: the decisions it cites and
/// a category, where in the program it first stood and the instruction or construct it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// The decision numbers the refusal cites, in order, each once.
    pub decisions: Vec<u32>,
    /// What kind of refusal: the translator's error, or `per-lane` for a dispatch kept whole.
    pub category: &'static str,
    /// The byte offset in the program it names, where it names one.
    pub offset: Option<u32>,
    /// The instruction or construct, without the offsets and counts that differ between programs.
    pub construct: String,
}

impl Class {
    /// The category, decisions and construct together: what rows of one class share.
    pub fn key(&self) -> String {
        let decisions: Vec<String> = self.decisions.iter().map(|d| format!("D{d}")).collect();
        format!(
            "{} [{}] {}",
            self.category,
            decisions.join(","),
            self.construct
        )
    }

    /// The class of a translator's refusal.
    pub fn of(error: &orbistoun_translate::TranslateError) -> Self {
        use orbistoun_translate::TranslateError as E;
        let message = error.to_string();
        let (category, offset, construct) = match error {
            E::StrategyNotImplemented(_) => ("strategy", None, String::new()),
            E::FidelityNotImplemented { level, .. } => ("fidelity", None, level.to_string()),
            E::UntrustworthyDecode { reason } => ("decode", None, (*reason).to_owned()),
            E::Unrecognised { offset } => ("unrecognised", Some(*offset), String::new()),
            E::WaveGroup { .. } => ("wave-group", None, String::new()),
            E::TooManyBuffers { .. } => ("too-many-buffers", None, String::new()),
            E::Unsupported { offset, detail } => ("unsupported", Some(*offset), general(detail)),
            E::NotTranslated {
                offset,
                mnemonic,
                detail,
            } => (
                "not-translated",
                Some(*offset),
                format!("{}: {}", general(mnemonic), general(detail)),
            ),
            E::Untraced {
                offset,
                mnemonic,
                store,
                ..
            } => (
                "untraced",
                Some(*offset),
                format!("{mnemonic} after {store}"),
            ),
            E::ImageDimension {
                offset,
                mnemonic,
                dimension,
            } => (
                "image-dimension",
                Some(*offset),
                format!("{mnemonic} {dimension}"),
            ),
            E::ReadsGeometryInputs => ("geometry-inputs", None, String::new()),
            E::NeedsBufferFormats => ("buffer-formats", None, String::new()),
            E::UnresolvedBufferFormat { offset, .. } => {
                ("buffer-format-slot", Some(*offset), String::new())
            }
            E::NeedsFlatTwins => ("flat-twins", None, String::new()),
            E::OperandsUnknown { offset, name, .. } => {
                ("operands-unknown", Some(*offset), name.clone())
            }
            E::MalformedModule(_) => ("malformed", None, general(&message)),
        };
        Self {
            decisions: decisions(&message),
            category,
            offset,
            construct,
        }
    }

    /// The class of a guest dispatch that keeps its whole wave in one invocation rather than
    /// running one per lane (D880, D884, D973), for `because`.
    pub fn kept_whole(because: &str) -> Self {
        Self {
            decisions: decisions(because),
            category: "per-lane",
            offset: None,
            construct: general(because),
        }
    }

    /// The class of a refusal given only as text: the decode checks a census makes itself.
    fn text(category: &'static str, message: &str) -> Self {
        Self {
            decisions: decisions(message),
            category,
            offset: None,
            construct: general(message),
        }
    }
}

/// The decision numbers `text` cites (`D` and three or four digits), in order, each once.
fn decisions(text: &str) -> Vec<u32> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for (at, _) in text.match_indices('D') {
        let before = at.checked_sub(1).map(|b| bytes[b]);
        if before.is_some_and(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        let digits: String = text[at + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if (3..=4).contains(&digits.len())
            && let Ok(number) = digits.parse()
            && !found.contains(&number)
        {
            found.push(number);
        }
    }
    found
}

/// `text` with its hexadecimal numbers made `#`, so two programs' refusals of one kind read alike.
fn general(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("0x") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 2..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .count();
        if digits == 0 {
            out.push_str("0x");
        } else {
            out.push('#');
        }
        rest = &rest[at + 2 + digits..];
    }
    out.push_str(rest);
    out
}

/// What one kept translation costs this build.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The route it was translated on.
    pub route: Route,
    /// What its refusal is, where it was refused, or why a guest dispatch kept its whole wave in
    /// one invocation (category `per-lane`, D973).
    pub class: Option<Class>,
    /// The guest address it was translated for, where its context names one.
    pub address: Option<u64>,
    /// The title whose store kept it.
    pub title: String,
    /// The guest program's content hash.
    pub program: u64,
    /// The hash of what it was translated against, which tells apart one program's translations.
    pub context: u64,
    /// The host stage.
    pub stage: Stage,
    /// Guest instructions.
    pub instructions: usize,
    /// SPIR-V words of the module; zero where it was refused.
    pub words: usize,
    /// How the wave was modelled: `wavefront` is the whole wave in one invocation, `lane` one
    /// invocation per lane. `None` where it was refused.
    pub model: Option<Fidelity>,
    /// Why the translator refused it.
    pub refusal: Option<String>,
    /// Its compile, where one was asked for.
    pub compiled: Option<Compiled>,
    /// Whether its module was validated, and the validator's message where it is invalid: `None`
    /// where no validation was asked for or the program was refused.
    pub validated: Option<Option<String>>,
    /// How many of the program's kept contexts made this same module, and so one row: a
    /// dispatch's window or user data can differ where its module does not.
    pub contexts: usize,
}

impl Row {
    /// Module words per guest instruction: how much the translation multiplied the program.
    #[allow(clippy::cast_precision_loss)] // word and instruction counts far below 2^52
    pub fn words_per_instruction(&self) -> f64 {
        self.words as f64 / self.instructions.max(1) as f64
    }

    fn seconds(&self) -> Option<f64> {
        self.compiled.as_ref().map(|compiled| compiled.seconds)
    }

    /// The validator's message, where its module was validated and found invalid.
    pub fn invalid(&self) -> Option<&str> {
        self.validated.as_ref()?.as_deref()
    }
}

/// What makes two translations one compile: the module, the images its layout binds, and the route
/// that made it.
type ModuleKey = (u64, usize, usize, Route);

/// Translates every distinct translation `store` keeps for `title` again, on `threads` threads, and
/// has `measure` time each distinct module's compile and `validate` validate it, each where given.
/// `done` hears of each row as it is made, in no set order; the rows come back in the store's order.
pub fn census(
    (title, store): (&str, &TranslationStore),
    (encodings, operands): (&EncodingTable, &OperandTable),
    threads: usize,
    (measure, validate): (Option<&Measure<'_>>, Option<&Validate<'_>>),
    done: &(dyn Fn(&Row) + Sync),
) -> Vec<Row> {
    let programs = distinct(store);
    let next = AtomicUsize::new(0);
    let rows = Mutex::new(vec![Vec::new(); programs.len()]);
    // Each module's first row, and how many translations made it.
    let modules: Mutex<HashMap<ModuleKey, (usize, usize)>> = Mutex::default();
    let first = |key: ModuleKey, at: usize| {
        let mut modules = modules
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let held = modules.entry(key).or_insert((at, 0));
        held.1 += 1;
        held.1 == 1
    };
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, MOST_THREADS) {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(kept) = programs.get(at) else {
                        break;
                    };
                    let made: Vec<Row> = routes(&kept.context)
                        .into_iter()
                        .map(|(route, context)| match route {
                            Route::Kept => (route, context),
                            Route::DeviceAddressed => (
                                route,
                                renumbered(
                                    &kept.bytes,
                                    (&kept.context, context),
                                    (encodings, operands),
                                ),
                            ),
                        })
                        .filter_map(|(route, context)| {
                            row(
                                (title, route),
                                (&kept.bytes, &context),
                                (encodings, operands),
                                (measure, validate),
                                &|key| first(key, at),
                            )
                        })
                        .collect();
                    made.iter().for_each(done);
                    rows.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)[at] = made;
                }
            });
        }
    });
    let mut rows = rows
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let modules = modules
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for ((.., route), (at, contexts)) in modules {
        if let Some(row) = rows[at].iter_mut().find(|row| row.route == route) {
            row.contexts = contexts;
        }
    }
    rows.into_iter().flatten().collect()
}

/// A device-addressed context's per-slot state carried over from the host-traced one it was made
/// from: a device-addressed dispatch binds only the buffers whose descriptors the host resolves
/// (D964), so its slots are the kept ones less the computed and global ones, numbered again. Each
/// slot's format word (D738) and its sampled and table bits (D951, D966) move to the slot its
/// source now has, as a run's preparation would find them; a slot whose source the kept trace does
/// not hold has none. Unchanged where either trace is refused.
fn renumbered(
    bytes: &[u8],
    (kept, mut device): (&Context, Context),
    (encodings, operands): (&EncodingTable, &OperandTable),
) -> Context {
    let decoded = decode_program(bytes, encodings, operands);
    let trace = |context: &Context| {
        orbistoun_translate::wavefront::draw_buffers_for(
            &decoded,
            encodings,
            context.stage,
            context.user_data,
        )
        .ok()
    };
    let (Some(before), Some(after)) = (trace(kept), trace(&device)) else {
        return device;
    };
    let kept_inputs = kept.user_data.compute.unwrap_or_default();
    let mut formats = kept
        .user_data
        .buffer_formats
        .map(|_| orbistoun_translate::wavefront::BufferFormats::default());
    let (mut sampled, mut tables) = (0u32, 0u64);
    for (slot, source) in after.sources.iter().enumerate() {
        let Some(was) = before.sources.iter().position(|held| held == source) else {
            continue;
        };
        if let (Some(formats), Some(held)) = (&mut formats, kept.user_data.buffer_formats) {
            orbistoun_translate::wavefront::BufferFormats::set(formats, slot, held.get(was));
        }
        if slot < 32 && kept_inputs.sampled_buffers >> was & 1 == 1 {
            sampled |= 1 << slot;
        }
        if slot < 64 && kept_inputs.table_buffers >> was & 1 == 1 {
            tables |= 1 << slot;
        }
    }
    device.user_data.buffer_formats = formats;
    if let Some(inputs) = &mut device.user_data.compute {
        inputs.sampled_buffers = sampled;
        inputs.table_buffers = tables;
    }
    device
}

/// The routes a run may translate a kept context on: as kept, and a compute dispatch kept
/// host-traced also device-addressed (D964), each with its context.
fn routes(context: &Context) -> Vec<(Route, Context)> {
    let mut routes = vec![(Route::Kept, *context)];
    if let Some(inputs) = context.user_data.compute
        && context.stage == Stage::Compute
        && !inputs.device_addressed
    {
        let mut device = *context;
        if let Some(inputs) = &mut device.user_data.compute {
            inputs.device_addressed = true;
        }
        // A device-addressed dispatch has no window: what no bound buffer serves it reaches where
        // it lies.
        device.window = Window::default();
        routes.push((Route::DeviceAddressed, device));
    }
    routes
}

/// The store's translations, each program under each context once.
fn distinct(store: &TranslationStore) -> Vec<&Kept> {
    let mut seen: HashMap<&[u8], Vec<&Context>> = HashMap::new();
    store
        .entries()
        .filter(|kept| {
            let contexts = seen.entry(&kept.bytes).or_default();
            let new = !contexts.contains(&&kept.context);
            contexts.push(&kept.context);
            new
        })
        .collect()
}

/// One kept translation made again, and compiled where `measure` is given and validated where
/// `validate` is; `None` where `first` says another translation already made the same module.
fn row(
    (title, route): (&str, Route),
    (bytes, context): (&[u8], &Context),
    tables: (&EncodingTable, &OperandTable),
    (measure, validate): (Option<&Measure<'_>>, Option<&Validate<'_>>),
    first: &dyn Fn(ModuleKey) -> bool,
) -> Option<Row> {
    let (instructions, translated) = translate(bytes, context, tables);
    let mut row = Row {
        route,
        class: None,
        address: context.user_data.program_address,
        title: title.to_owned(),
        program: bytes_hash(bytes),
        context: serde_json::to_vec(context).map_or(0, |json| bytes_hash(&json)),
        stage: context.stage,
        instructions,
        words: 0,
        model: None,
        refusal: None,
        compiled: None,
        validated: None,
        contexts: 1,
    };
    match translated {
        Ok(translated) => {
            let key = (
                crate::content_hash(&translated.module),
                translated.textures.len(),
                translated.storage.len(),
                route,
            );
            if !first(key) {
                return None;
            }
            row.words = translated.module.len();
            row.model = Some(translated.fidelity);
            // A guest dispatch kept whole says why: what keeps it from one invocation a lane.
            if context.user_data.compute.is_some() && translated.fidelity == Fidelity::Wavefront {
                row.class = Some(Class::kept_whole(&kept_whole(&translated.warnings)));
            }
            row.validated = validate.map(|validate| validate(&translated.module));
            row.compiled = measure.map(|measure| {
                measure(&Program {
                    stage: context.stage,
                    module: &translated.module,
                    fetched: translated.textures.len(),
                    stored: translated.storage.len(),
                })
            });
        }
        Err((refusal, class)) => {
            row.refusal = Some(refusal);
            row.class = Some(class);
        }
    }
    Some(row)
}

/// Why a guest dispatch's translation kept its whole wave in one invocation, from its warnings.
fn kept_whole(warnings: &[Warning]) -> String {
    warnings
        .iter()
        .find_map(|warning| match warning {
            Warning::SlowestFidelity { because, .. } => Some((*because).to_owned()),
            Warning::FloatMode { .. } => None,
        })
        .unwrap_or_else(|| "the program reads or writes a lane mask whole".to_owned())
}

/// The program's instruction count, and its translation or why there is none, with its class: the
/// checks a store's refill makes, then the translator's own.
fn translate(
    bytes: &[u8],
    context: &Context,
    (encodings, operands): (&EncodingTable, &OperandTable),
) -> (usize, Result<Translated, (String, Class)>) {
    let decoded = decode_program(bytes, encodings, operands);
    let instructions = decoded.instructions.len();
    if !decoded.terminated || !decoded.is_trustworthy() || decoded.consumed != bytes.len() {
        let why = "the program no longer decodes cleanly";
        return (
            instructions,
            Err((why.to_owned(), Class::text("decode", why))),
        );
    }
    let translated = translate_with_user_data(
        &decoded,
        encodings,
        context.strategy,
        (context.stage, context.primitive),
        context.window,
        context.user_data,
    )
    .map_err(|e| (e.to_string(), Class::of(&e)));
    (instructions, translated)
}

/// [`crate::content_hash`] over bytes, the last word padded with zeroes.
fn bytes_hash(bytes: &[u8]) -> u64 {
    let words: Vec<u32> = bytes
        .chunks(4)
        .map(|chunk| {
            let mut word = [0; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            u32::from_le_bytes(word)
        })
        .collect();
    crate::content_hash(&words)
}

/// Longest compile first; rows never compiled last. Ties go to the larger module, then the program,
/// so a ranking is the same from run to run.
pub fn sort_by_compile(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        let seconds = |row: &Row| row.seconds().unwrap_or(-1.0);
        seconds(b)
            .total_cmp(&seconds(a))
            .then_with(|| b.words.cmp(&a.words))
            .then_with(|| (a.program, a.context).cmp(&(b.program, b.context)))
    });
}

/// Largest module first. Ties go to the program.
pub fn sort_by_words(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        b.words
            .cmp(&a.words)
            .then_with(|| (a.program, a.context).cmp(&(b.program, b.context)))
    });
}

/// The TSV a census is kept as: a header, then one line per row in the order given. A refusal or a
/// compile failure is kept to one field, as is the validator's message, its tabs and line breaks
/// made spaces; `valid` is empty where nothing was validated.
pub fn tsv(rows: &[Row]) -> String {
    let mut out = String::from(
        "title\tprogram\tcontext\tcontexts\tstage\tinstructions\twords\twords_per_instruction\tmodel\ttimed\tcompile_seconds\trefused\tvalid\tinvalid\troute\taddress\tclass\toffset\n",
    );
    for row in rows {
        let compiled = row.compiled.as_ref();
        let refused = row
            .refusal
            .as_deref()
            .or_else(|| compiled.and_then(|compiled| compiled.failed.as_deref()))
            .unwrap_or("")
            .replace(['\t', '\n', '\r'], " ");
        let valid = match &row.validated {
            None => "",
            Some(None) => "yes",
            Some(Some(_)) => "no",
        };
        let invalid = row.invalid().unwrap_or("").replace(['\t', '\n', '\r'], " ");
        let class = row
            .class
            .as_ref()
            .map(|class| class.key().replace(['\t', '\n', '\r'], " "))
            .unwrap_or_default();
        let offset = row
            .class
            .as_ref()
            .and_then(|class| class.offset)
            .map(|offset| format!("{offset:#x}"))
            .unwrap_or_default();
        let address = row
            .address
            .map(|address| format!("{address:#x}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{}\t{:016x}\t{:016x}\t{}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{refused}\t{valid}\t{invalid}\t{}\t{address}\t{class}\t{offset}",
            row.title,
            row.program,
            row.context,
            row.contexts,
            stage_name(row.stage),
            row.instructions,
            row.words,
            row.words_per_instruction(),
            row.model.map(|model| model.to_string()).unwrap_or_default(),
            compiled.map_or("", |compiled| compiled.timed.name()),
            compiled.map_or_else(String::new, |compiled| format!("{:.3}", compiled.seconds)),
            row.route.name(),
        );
    }
    out
}

/// One class of refusal across a census: how many rows it holds, its programs, and where it first
/// stood in the first of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassCount {
    /// The route the rows were translated on.
    pub route: Route,
    /// The class, by [`Class::key`].
    pub key: String,
    /// Rows in it.
    pub rows: usize,
    /// Its distinct programs, by address where known, else by content hash, in first-seen order.
    pub programs: Vec<String>,
    /// The first program's offset, where the class names one.
    pub offset: Option<u32>,
}

/// Every class of refusal in `rows`, the largest reach first: most programs, then most rows, then
/// by route and key, so the order does not move between runs.
pub fn classes(rows: &[Row]) -> Vec<ClassCount> {
    let mut counted: Vec<ClassCount> = Vec::new();
    for row in rows {
        let Some(class) = &row.class else { continue };
        let key = class.key();
        let program = row.address.map_or_else(
            || format!("{:016x}", row.program),
            |address| format!("{address:#x}"),
        );
        let at = if let Some(at) = counted
            .iter()
            .position(|counted| counted.route == row.route && counted.key == key)
        {
            at
        } else {
            counted.push(ClassCount {
                route: row.route,
                key,
                rows: 0,
                programs: Vec::new(),
                offset: class.offset,
            });
            counted.len() - 1
        };
        let counted = &mut counted[at];
        counted.rows += 1;
        if !counted.programs.contains(&program) {
            counted.programs.push(program);
        }
    }
    counted.sort_by(|a, b| {
        b.programs
            .len()
            .cmp(&a.programs.len())
            .then_with(|| b.rows.cmp(&a.rows))
            .then_with(|| (a.route, &a.key).cmp(&(b.route, &b.key)))
    });
    counted
}

/// The refusal classes, as [`classes`] ranks them: route, rows, programs, the first offset and the
/// class, then the programs, the first few.
fn class_table(out: &mut String, rows: &[Row]) {
    let counted = classes(rows);
    if counted.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\nrefusal classes\n{:<7} {:>5} {:>8} {:>8} CLASS",
        "ROUTE", "ROWS", "PROGRAMS", "OFFSET"
    );
    for class in counted {
        let _ = writeln!(
            out,
            "{:<7} {:>5} {:>8} {:>8} {}",
            class.route.name(),
            class.rows,
            class.programs.len(),
            class
                .offset
                .map_or_else(|| "-".to_owned(), |offset| format!("{offset:#x}")),
            class.key
        );
        let shown: Vec<&str> = class.programs.iter().take(6).map(String::as_str).collect();
        let more = class.programs.len().saturating_sub(shown.len());
        let _ = writeln!(
            out,
            "        {}{}",
            shown.join(" "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        );
    }
}

const fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Compute => "compute",
        Stage::Fragment => "fragment",
        Stage::Mesh => "mesh",
    }
}

/// A census's totals.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Rows: distinct modules, and a program's refused contexts.
    pub programs: usize,
    /// The kept translations they stand for.
    pub translations: usize,
    /// Of those, how many the translator refused.
    pub refused: usize,
    /// Words of every module.
    pub words: usize,
    /// How many were compiled, how many of those the driver refused, and their seconds together.
    pub compiled: usize,
    /// See [`Self::compiled`].
    pub compile_failed: usize,
    /// See [`Self::compiled`].
    pub compile_seconds: f64,
    /// How many modules were validated, and how many of those the validator refused.
    pub validated: usize,
    /// See [`Self::validated`].
    pub invalid: usize,
    /// The translated row with the most words per instruction.
    pub worst: Option<Row>,
}

impl Summary {
    /// The totals of `rows`.
    pub fn of(rows: &[Row]) -> Self {
        let compiled: Vec<&Compiled> = rows
            .iter()
            .filter_map(|row| row.compiled.as_ref())
            .collect();
        Self {
            programs: rows.len(),
            translations: rows.iter().map(|row| row.contexts).sum(),
            refused: rows.iter().filter(|row| row.refusal.is_some()).count(),
            words: rows.iter().map(|row| row.words).sum(),
            compiled: compiled.len(),
            compile_failed: compiled.iter().filter(|c| c.failed.is_some()).count(),
            compile_seconds: compiled.iter().map(|c| c.seconds).sum(),
            validated: rows.iter().filter(|row| row.validated.is_some()).count(),
            invalid: rows.iter().filter(|row| row.invalid().is_some()).count(),
            worst: rows
                .iter()
                .filter(|row| row.refusal.is_none())
                .max_by(|a, b| {
                    a.words_per_instruction()
                        .total_cmp(&b.words_per_instruction())
                })
                .cloned(),
        }
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} programs ({} kept translations), {} refused, {} words",
            self.programs, self.translations, self.refused, self.words
        )?;
        if self.compiled > 0 {
            write!(
                f,
                "; {} compiled in {:.1} s, {} refused by the driver",
                self.compiled, self.compile_seconds, self.compile_failed
            )?;
        }
        if self.validated > 0 {
            write!(
                f,
                "; {} validated, {} invalid",
                self.validated, self.invalid
            )?;
        }
        if let Some(worst) = &self.worst {
            write!(
                f,
                "\nmost words per instruction: {:.1} ({:016x}, {} {}, {} instructions)",
                worst.words_per_instruction(),
                worst.program,
                stage_name(worst.stage),
                worst
                    .model
                    .map(|model| model.to_string())
                    .unwrap_or_default(),
                worst.instructions
            )?;
        }
        Ok(())
    }
}

/// The summary, then the `top` rows by compile time where any was compiled, then by words.
pub fn report(rows: &[Row], top: usize) -> String {
    let mut out = format!("{}\n", Summary::of(rows));
    let mut ranked = rows.to_vec();
    if rows.iter().any(|row| row.compiled.is_some()) {
        sort_by_compile(&mut ranked);
        table(
            &mut out,
            "by compile time",
            &ranked[..top.min(ranked.len())],
        );
    }
    sort_by_words(&mut ranked);
    table(&mut out, "by words", &ranked[..top.min(ranked.len())]);
    class_table(&mut out, rows);
    invalid(&mut out, rows);
    out
}

/// Every module the validator refused, each with the first lines of its message: all of them, as
/// each is a fault to fix rather than a cost to rank.
fn invalid(out: &mut String, rows: &[Row]) {
    let refused: Vec<&Row> = rows.iter().filter(|row| row.invalid().is_some()).collect();
    if refused.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\ninvalid modules\n{:<8} {:<16} {:<8} TITLE",
        "STAGE", "PROGRAM", "CONTEXT"
    );
    for row in refused {
        let _ = writeln!(
            out,
            "{:<8} {:016x} {:08x} {}",
            stage_name(row.stage),
            row.program,
            row.context >> 32,
            row.title
        );
        for line in row.invalid().unwrap_or("").lines().take(2) {
            let _ = writeln!(out, "    {}", line.trim());
        }
    }
}

fn table(out: &mut String, heading: &str, rows: &[Row]) {
    let _ = writeln!(
        out,
        "\n{heading}\n{:>9} {:<8} {:>9} {:>6} {:>8} {:<9} {:<8} {:<16} {:<8} TITLE",
        "SECONDS", "TIMED", "WORDS", "INSTR", "W/INSTR", "MODEL", "STAGE", "PROGRAM", "CONTEXT"
    );
    for row in rows {
        let compiled = row.compiled.as_ref();
        let _ = writeln!(
            out,
            "{:>9} {:<8} {:>9} {:>6} {:>8.1} {:<9} {:<8} {:016x} {:08x} {}{}",
            compiled.map_or_else(|| "-".to_owned(), |c| format!("{:.2}", c.seconds)),
            compiled.map_or("-", |c| c.timed.name()),
            row.words,
            row.instructions,
            row.words_per_instruction(),
            row.model
                .map_or_else(|| "refused".to_owned(), |model| model.to_string()),
            stage_name(row.stage),
            row.program,
            row.context >> 32,
            row.title,
            compiled
                .and_then(|c| c.failed.as_deref())
                .map(|failed| format!(" (driver refused: {failed})"))
                .unwrap_or_default(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Compiled, Row, Summary, Timed, census, report, sort_by_compile, sort_by_words, tsv,
    };
    use crate::translations::{Context, Kept, TranslationStore};
    use orbistoun_shader::{EncodingTable, OperandTable};
    use orbistoun_translate::wavefront::{MeshPrimitive, Stage, UserData, Window};
    use orbistoun_translate::{Fidelity, Strategy};

    fn row(program: u64, words: usize, seconds: Option<f64>) -> Row {
        Row {
            route: super::Route::Kept,
            class: None,
            address: None,
            title: "T".to_owned(),
            program,
            context: 0,
            stage: Stage::Compute,
            instructions: 10,
            words,
            model: Some(Fidelity::Lane),
            refusal: None,
            compiled: seconds.map(|seconds| Compiled {
                seconds,
                timed: Timed::Pipeline,
                failed: None,
            }),
            validated: None,
            contexts: 1,
        }
    }

    fn programs(rows: &[Row]) -> Vec<u64> {
        rows.iter().map(|row| row.program).collect()
    }

    /// The longest compile ranks first, a row never compiled last, and equal times fall to the
    /// larger module, so a ranking does not move between runs.
    #[test]
    fn rows_rank_by_compile_time_then_words() {
        let mut rows = vec![
            row(1, 100, None),
            row(2, 100, Some(1.0)),
            row(3, 500, Some(1.0)),
            row(4, 50, Some(9.0)),
        ];
        sort_by_compile(&mut rows);
        assert_eq!(programs(&rows), [4, 3, 2, 1]);
    }

    /// The largest module ranks first; equal sizes fall to the program.
    #[test]
    fn rows_rank_by_words_then_program() {
        let mut rows = vec![row(3, 10, None), row(1, 10, None), row(2, 90, None)];
        sort_by_words(&mut rows);
        assert_eq!(programs(&rows), [2, 1, 3]);
    }

    /// A TSV has a header and one line per row with every field in place; an empty compile leaves
    /// its fields empty, and a refusal's tabs and line breaks cannot split its line.
    #[test]
    fn a_tsv_keeps_one_line_per_row() {
        let mut refused = row(2, 0, None);
        refused.model = None;
        refused.refusal = Some("bad\tthing\nhere".to_owned());
        let text = tsv(&[row(1, 120, Some(2.5)), refused]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("title\tprogram\t"));
        let fields = |line: &str| line.split('\t').map(str::to_owned).collect::<Vec<_>>();
        let header = fields(lines[0]);
        assert!(lines.iter().all(|line| fields(line).len() == header.len()));
        assert_eq!(
            fields(lines[1]),
            [
                "T",
                "0000000000000001",
                "0000000000000000",
                "1",
                "compute",
                "10",
                "120",
                "12.0",
                "lane",
                "pipeline",
                "2.500",
                "",
                "",
                "",
                "kept",
                "",
                "",
                ""
            ]
        );
        let second = fields(lines[2]);
        assert_eq!(
            second[8..],
            ["", "", "", "bad thing here", "", "", "kept", "", "", ""]
        );
    }

    /// A refusal's class keeps its decisions and its instruction, and drops the offsets that differ
    /// between programs, so one fix's refusals count as one class; classes rank by the programs
    /// they reach.
    #[test]
    fn refusals_group_into_classes_by_reach() {
        use orbistoun_translate::TranslateError;
        let refusal = |offset| TranslateError::NotTranslated {
            offset,
            mnemonic: "v_example_b32".to_owned(),
            detail: "a form not translated (D979, D884 and D979 again)",
        };
        let class = super::Class::of(&refusal(0x40));
        assert_eq!(class.decisions, [979, 884]);
        assert_eq!(class.offset, Some(0x40));
        assert_eq!(class.key(), super::Class::of(&refusal(0x80)).key());
        assert_eq!(super::general("at 0x1f0 and 0x2"), "at # and #");
        let classed = |program, error: &TranslateError| {
            let mut row = row(program, 0, None);
            row.refusal = Some(error.to_string());
            row.class = Some(super::Class::of(error));
            row
        };
        let other = TranslateError::NeedsFlatTwins;
        let rows = [
            classed(1, &other),
            classed(1, &other),
            classed(2, &refusal(0x10)),
            classed(3, &refusal(0x20)),
            row(4, 10, None),
        ];
        let counted = super::classes(&rows);
        assert_eq!(counted.len(), 2);
        assert_eq!(counted[0].programs.len(), 2);
        assert_eq!(counted[0].offset, Some(0x10));
        assert_eq!((counted[1].rows, counted[1].programs.len()), (2, 1));
        assert!(report(&rows, 1).contains("refusal classes"));
    }

    /// A compute dispatch kept host-traced is also translated device-addressed, without its window;
    /// one kept device-addressed, or a draw's stage, is translated only as kept.
    #[test]
    fn a_host_traced_dispatch_is_also_tried_device_addressed() {
        use orbistoun_translate::wavefront::ComputeInputs;
        let mut context = Context {
            strategy: Strategy::default(),
            stage: Stage::Compute,
            primitive: MeshPrimitive::default(),
            window: Window::at(0x1000),
            user_data: UserData::default(),
        };
        assert_eq!(super::routes(&context).len(), 1, "no entry state");
        context.user_data.compute = Some(ComputeInputs::default());
        let routes = super::routes(&context);
        assert_eq!(routes.len(), 2);
        let (route, device) = &routes[1];
        assert_eq!(*route, super::Route::DeviceAddressed);
        assert!(device.user_data.compute.is_some_and(|i| i.device_addressed));
        assert_eq!(device.window, Window::default());
        assert_eq!(super::routes(device).len(), 1);
    }

    /// The summary counts refusals and compile failures, sums words and seconds, and names the
    /// translated row with the most words per instruction - never a refused one.
    #[test]
    fn a_summary_totals_and_names_the_worst_ratio() {
        let mut refused = row(9, 0, None);
        refused.instructions = 0;
        refused.refusal = Some("no".to_owned());
        let mut failed = row(3, 40, Some(0.5));
        failed.contexts = 3;
        if let Some(compiled) = &mut failed.compiled {
            compiled.failed = Some("driver".to_owned());
        }
        let rows = vec![
            row(1, 100, Some(1.5)),
            row(2, 900, Some(2.0)),
            failed,
            refused,
        ];
        let summary = Summary::of(&rows);
        assert_eq!(
            (
                summary.programs,
                summary.translations,
                summary.refused,
                summary.words
            ),
            (4, 6, 1, 1040)
        );
        assert_eq!((summary.compiled, summary.compile_failed), (3, 1));
        assert!((summary.compile_seconds - 4.0).abs() < 1e-9);
        assert_eq!(summary.worst.map(|worst| worst.program), Some(2));
    }

    /// A report ranks by compile time only where something was compiled, and shows at most `top`
    /// rows a table.
    #[test]
    fn a_report_ranks_what_it_has() {
        let rows = vec![row(1, 100, None), row(2, 300, None), row(3, 200, None)];
        let text = report(&rows, 2);
        assert!(!text.contains("by compile time"));
        let listed: Vec<&str> = text
            .lines()
            .skip_while(|line| *line != "by words")
            .skip(2)
            .collect();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].contains("0000000000000002"));
        let compiled = report(&[row(1, 100, Some(3.0))], 5);
        assert!(compiled.contains("by compile time"));
    }

    /// A census makes each kept program again, once however many entries hold it under one context,
    /// hands each distinct module to the measure once - a context that makes the same module is
    /// counted on its row - and keeps the translator's reason for a program it refuses.
    #[test]
    fn a_census_translates_each_program_once_and_measures_it() {
        let (encodings, operands) = (
            EncodingTable::builtin().expect("encodings"),
            OperandTable::builtin().expect("operands"),
        );
        let context = Context {
            strategy: Strategy::default(),
            stage: Stage::Compute,
            primitive: MeshPrimitive::default(),
            window: Window::default(),
            user_data: UserData::default(),
        };
        // `s_endpgm` alone, the smallest program that translates.
        let trivial = 0xBF81_0000_u32.to_le_bytes();
        // The kept module is not read: the census makes its own.
        let kept = Kept {
            bytes: trivial.to_vec(),
            context,
            module: Vec::new(),
            textures: Vec::new(),
            storage: Vec::new(),
            warnings: Vec::new(),
        };
        let mut refused = kept.clone();
        refused.bytes = vec![0xff; 4];
        // A compute module does not read the primitive, so this is another context, one module.
        let mut same_module = kept.clone();
        same_module.context.primitive = MeshPrimitive::Points;
        let mut store = TranslationStore::default();
        store.insert(1, kept.clone());
        store.insert(2, kept);
        store.insert(3, refused);
        store.insert(4, same_module);

        let measured = std::sync::atomic::AtomicUsize::new(0);
        let measure = |program: &super::Program<'_>| {
            measured.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            assert!(!program.module.is_empty());
            Compiled {
                seconds: 0.25,
                timed: Timed::Pipeline,
                failed: None,
            }
        };
        let heard = std::sync::atomic::AtomicUsize::new(0);
        // Every module is refused here, so each row the validator saw says so.
        let validated = std::sync::atomic::AtomicUsize::new(0);
        let validate = |module: &[u32]| {
            validated.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (!module.is_empty()).then(|| "refused".to_owned())
        };
        let rows = census(
            ("T", &store),
            (&encodings, &operands),
            2,
            (Some(&measure), Some(&validate)),
            &|_| {
                heard.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            },
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(heard.into_inner(), 2);
        assert_eq!(measured.into_inner(), 1);
        assert_eq!(validated.into_inner(), 1);
        let made = rows
            .iter()
            .find(|row| row.refusal.is_none())
            .expect("one translates");
        assert!(made.words > 0 && made.instructions == 1 && made.model.is_some());
        assert_eq!(made.contexts, 2);
        assert_eq!(made.invalid(), Some("refused"));
        assert!(report(&rows, 5).contains("invalid modules"));
        assert_eq!(Summary::of(&rows).invalid, 1);
        assert!(
            rows.iter()
                .any(|row| row.refusal.is_some() && row.compiled.is_none())
        );
    }
}
