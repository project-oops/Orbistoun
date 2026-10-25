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
use orbistoun_translate::wavefront::Stage;
use orbistoun_translate::{Fidelity, Translated, translate_with_user_data};

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

/// What one kept translation costs this build.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
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

/// What makes two translations one compile: the module, and the images its layout binds.
type ModuleKey = (u64, usize, usize);

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
    let rows = Mutex::new(vec![None; programs.len()]);
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
                    let made = row(
                        title,
                        kept,
                        (encodings, operands),
                        (measure, validate),
                        &|key| first(key, at),
                    );
                    if let Some(made) = &made {
                        done(made);
                    }
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
    for (at, contexts) in modules.into_values() {
        if let Some(row) = &mut rows[at] {
            row.contexts = contexts;
        }
    }
    rows.into_iter().flatten().collect()
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
    title: &str,
    kept: &Kept,
    tables: (&EncodingTable, &OperandTable),
    (measure, validate): (Option<&Measure<'_>>, Option<&Validate<'_>>),
    first: &dyn Fn(ModuleKey) -> bool,
) -> Option<Row> {
    let context = &kept.context;
    let (instructions, translated) = translate(&kept.bytes, context, tables);
    let mut row = Row {
        title: title.to_owned(),
        program: bytes_hash(&kept.bytes),
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
            );
            if !first(key) {
                return None;
            }
            row.words = translated.module.len();
            row.model = Some(translated.fidelity);
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
        Err(refusal) => row.refusal = Some(refusal),
    }
    Some(row)
}

/// The program's instruction count, and its translation or why there is none: the checks a store's
/// refill makes, then the translator's own.
fn translate(
    bytes: &[u8],
    context: &Context,
    (encodings, operands): (&EncodingTable, &OperandTable),
) -> (usize, Result<Translated, String>) {
    let decoded = decode_program(bytes, encodings, operands);
    let instructions = decoded.instructions.len();
    if !decoded.terminated || !decoded.is_trustworthy() || decoded.consumed != bytes.len() {
        return (
            instructions,
            Err("the program no longer decodes cleanly".to_owned()),
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
    .map_err(|e| e.to_string());
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
        "title\tprogram\tcontext\tcontexts\tstage\tinstructions\twords\twords_per_instruction\tmodel\ttimed\tcompile_seconds\trefused\tvalid\tinvalid\n",
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
        let _ = writeln!(
            out,
            "{}\t{:016x}\t{:016x}\t{}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{refused}\t{valid}\t{invalid}",
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
        );
    }
    out
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
                ""
            ]
        );
        let second = fields(lines[2]);
        assert_eq!(second[8..], ["", "", "", "bad thing here", "", ""]);
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
