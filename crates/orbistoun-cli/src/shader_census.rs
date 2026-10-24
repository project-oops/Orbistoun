//! What every kept shader program of a title costs this build: `shader-census`.

use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, bail};
use orbistoun_gpu::shader_census::{self, Compiled, MOST_THREADS, Program, Row};
use orbistoun_shader::{EncodingTable, OperandTable};

/// Translates every program `title`'s store keeps - every title's when none is named - again,
/// compiles each where `compile` asks, prints the summary and the `top` of each ranking, and writes
/// every row as TSV to `write`.
pub(crate) fn cmd_shader_census(
    title: Option<&str>,
    compile: bool,
    top: usize,
    write: Option<&std::path::Path>,
) -> Result<()> {
    let paths = orbistoun_paths::Paths::resolve();
    let tables = (EncodingTable::builtin()?, OperandTable::builtin()?);
    if compile
        && let orbistoun_gpu_vulkan::Availability::Unavailable { reason } =
            orbistoun_gpu_vulkan::probe()
    {
        bail!("--compile needs a device: {reason}");
    }
    let titles = if let Some(title) = title {
        vec![title.to_owned()]
    } else {
        let mut titles: Vec<String> = std::fs::read_dir(paths.titles_dir())?
            .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_owned()))
            .filter(|title| paths.title_translations_file(title).is_file())
            .collect();
        titles.sort();
        titles
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(MOST_THREADS);
    let time: &(dyn Fn(&Program<'_>) -> Compiled + Sync) =
        &orbistoun_gpu_vulkan::shader_census::time_compile;
    let measure = compile.then_some(time);

    let mut rows = Vec::new();
    for title in &titles {
        let store = orbistoun_gpu::translations::TranslationStore::load(
            &paths.title_translations_file(title),
        );
        if store.is_empty() {
            bail!("{title} keeps no translations");
        }
        eprintln!(
            "{title}: {} kept translations, {threads} threads",
            store.len()
        );
        let made = AtomicUsize::new(0);
        rows.extend(shader_census::census(
            (title, &store),
            (&tables.0, &tables.1),
            threads,
            measure,
            &|row| progress(made.fetch_add(1, Ordering::Relaxed) + 1, row),
        ));
    }
    print!("{}", shader_census::report(&rows, top));
    if let Some(path) = write {
        std::fs::write(path, shader_census::tsv(&rows))?;
        println!("\nwrote {} rows to {}", rows.len(), path.display());
    }
    Ok(())
}

/// A line on standard error for every slow compile, and every hundredth row, so a long census is
/// seen to move.
fn progress(made: usize, row: &Row) {
    let seconds = row.compiled.as_ref().map(|compiled| compiled.seconds);
    if made % 100 == 0 || seconds.is_some_and(|seconds| seconds >= 1.0) {
        eprintln!(
            "  {made:>6} made; {:016x} {} words{}",
            row.program,
            row.words,
            seconds.map(|s| format!(", {s:.1} s")).unwrap_or_default()
        );
    }
}
