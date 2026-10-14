//! Which of the library's guest compute programs one invocation per lane runs exactly: `lanes`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::Result;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::lane_exact::{LaneNeed, lane_needs};
use orbistoun_translate::{Strategy, Width};

/// One title's guest compute programs, counted by what they need of the wave.
#[derive(Debug, Default)]
struct Tally {
    programs: usize,
    /// Run exactly per lane, holding no per-lane bit.
    plain: usize,
    /// Run exactly per lane, holding per-lane bits.
    lane_bits: usize,
    /// By need, how many programs have it, and how many have only it.
    needs: BTreeMap<LaneNeed, (usize, usize)>,
    /// Of the exact ones, how many are wave32, and how many are groups of several waves.
    exact_wave32: usize,
    exact_multiwave: usize,
    /// Of those that need a whole mask and nothing else, how many are wave32, and how many are
    /// groups of one wave: per lane by ballot on a 32-wide subgroup, and through workgroup memory
    /// on any (D884).
    mask_wave32: usize,
    mask_one_wave: usize,
}

impl Tally {
    fn exact(&self) -> usize {
        self.plain + self.lane_bits
    }

    fn line(&self, name: &str) -> String {
        let mut line = format!(
            "{name:<18} {:>4} programs  {:>4} exact per lane ({} plain, {} with lane bits; {} wave32, {} multi-wave)",
            self.programs,
            self.exact(),
            self.plain,
            self.lane_bits,
            self.exact_wave32,
            self.exact_multiwave,
        );
        for (need, (any, only)) in &self.needs {
            let _ = write!(
                line,
                "\n{:<18}   {any:>4} need {need} ({only} only that)",
                ""
            );
            if *need == LaneNeed::WholeMask {
                let _ = write!(
                    line,
                    "; of those, {} wave32 and {} a group of one wave",
                    self.mask_wave32, self.mask_one_wave
                );
            }
        }
        line
    }

    fn add(&mut self, other: &Self) {
        self.programs += other.programs;
        self.plain += other.plain;
        self.lane_bits += other.lane_bits;
        self.exact_wave32 += other.exact_wave32;
        self.exact_multiwave += other.exact_multiwave;
        self.mask_wave32 += other.mask_wave32;
        self.mask_one_wave += other.mask_one_wave;
        for (need, (any, only)) in &other.needs {
            let held = self.needs.entry(*need).or_default();
            held.0 += any;
            held.1 += only;
        }
    }
}

/// Walks every title's kept translations and counts its guest compute programs by what keeps one
/// invocation per lane from running them exactly. Each program counts once per title, and once in
/// the total however many titles hold it.
pub(crate) fn cmd_lanes() -> Result<()> {
    let paths = orbistoun_paths::Paths::resolve();
    let encodings = EncodingTable::builtin()?;
    let operands = OperandTable::builtin()?;
    let mut titles: Vec<String> = std::fs::read_dir(paths.titles_dir())?
        .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_owned()))
        .collect();
    titles.sort();

    let mut total = Tally::default();
    let mut seen = BTreeSet::new();
    for title in titles {
        let file = paths.title_translations_file(&title);
        if !file.is_file() {
            continue;
        }
        let store = orbistoun_gpu::translations::TranslationStore::load(&file);
        let mut tally = Tally::default();
        let mut distinct = Tally::default();
        let mut here = BTreeSet::new();
        for kept in store.entries() {
            let context = &kept.context;
            let Some(inputs) = context.user_data.compute else {
                continue;
            };
            if context.stage != orbistoun_translate::wavefront::Stage::Compute
                || !here.insert(kept.bytes.clone())
            {
                continue;
            }
            let width = match context.strategy {
                Strategy::Predicated { width, .. } => width,
                Strategy::Structured => Width::default(),
            };
            let decoded = decode_program(&kept.bytes, &encodings, &operands);
            let needs = lane_needs(&decoded, &encodings, width.lanes());
            let mut one = Tally {
                programs: 1,
                ..Tally::default()
            };
            if needs.exact() {
                if needs.lane_bits {
                    one.lane_bits = 1;
                } else {
                    one.plain = 1;
                }
                one.exact_wave32 = usize::from(width == Width::Wave32);
                one.exact_multiwave = usize::from(inputs.waves(width.lanes()) > 1);
            }
            let only = needs.needs.len() == 1;
            if only && needs.has(LaneNeed::WholeMask) {
                one.mask_wave32 = usize::from(width == Width::Wave32);
                one.mask_one_wave = usize::from(inputs.waves(width.lanes()) == 1);
            }
            for (need, _, _) in &needs.needs {
                one.needs.insert(*need, (1, usize::from(only)));
            }
            tally.add(&one);
            if seen.insert(kept.bytes.clone()) {
                distinct.add(&one);
            }
        }
        if tally.programs > 0 {
            println!("{}", tally.line(&title));
            total.add(&distinct);
        }
    }
    println!("{}", total.line("all titles"));
    Ok(())
}
