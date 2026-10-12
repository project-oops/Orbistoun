//! What no measurement covers, across the corpus: `census`.
//!
//! The capability sub-loop's first step (`docs/THE_LOOP.md`): every import of every title module
//! whose knowledge entry is missing or rests on something weaker than a measurement, and every
//! shader refusal the runs recorded, each ranked by how many titles reach it. A census row is a
//! hardware question; grouped by library or GPU layer, the rows become one obSCEne request per
//! group (D818).

use crate::common::library_or;
use anyhow::Result;
use orbistoun_service::Service;
use std::collections::{BTreeMap, BTreeSet};

/// One import nothing has measured: where it lives, what is known, and who reaches it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ImportRow {
    /// The library the import table names.
    pub library: String,
    /// The symbol, or the hash as the trace labels it when nothing names it.
    pub name: String,
    /// The knowledge entry's `known_by`, or `-` when there is no entry.
    pub known_by: String,
    /// Whether orbistoun has an implementation behind it.
    pub implemented: bool,
    /// The titles whose modules import it.
    pub titles: BTreeSet<String>,
    /// Calls across every persisted trace, for the label the trace records.
    pub calls: u64,
}

/// One shader refusal reason and the titles whose runs hit it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RefusalRow {
    /// The translator's reason, with the stage and address stripped.
    pub reason: String,
    /// How many shaders were refused for it, across every trace.
    pub shaders: usize,
    /// The guests whose traces carried it.
    pub titles: BTreeSet<String>,
}

/// Whether a `known_by` settles an import, so the census leaves it out.
///
/// Only a measurement on the target or a published source does; `differential`, `guest-observed`
/// and `assumed` are all questions a probe can still answer.
fn is_settled(known_by: &str) -> bool {
    matches!(known_by, "measured" | "published")
}

/// The reason in a recorded shader failure, with every address in it made the same.
///
/// Traces record each as `stage at address: reason`, and the reason can name addresses of its
/// own (`the shader at 0x7400014c3800 ...`, `instruction at 0x4 ...`). Two shaders refused for
/// one reason differ only in those, so each hexadecimal number becomes `0x_` and they count as
/// one row.
fn refusal_reason(failure: &str) -> String {
    let reason = failure
        .split_once(": ")
        .map_or(failure, |(_, reason)| reason)
        .trim();
    let mut out = String::with_capacity(reason.len());
    let mut rest = reason;
    while let Some(at) = rest.find("0x") {
        out.push_str(&rest[..at]);
        let digits = rest[at + 2..]
            .bytes()
            .take_while(u8::is_ascii_hexdigit)
            .count();
        if digits == 0 {
            out.push_str("0x");
        } else {
            out.push_str("0x_");
        }
        rest = &rest[at + 2 + digits..];
    }
    out.push_str(rest);
    out
}

/// Whether a title directory is one the census was asked about.
///
/// A prefix, so `PPSA02664` selects `PPSA02664-app0`; no filter selects everything.
fn selected(title: &str, filter: &[String]) -> bool {
    filter.is_empty() || filter.iter().any(|f| title.starts_with(f.as_str()))
}

/// Ranks import rows: most titles first, then most calls, then by library and name.
fn rank_imports(rows: &mut [ImportRow]) {
    rows.sort_by(|a, b| {
        b.titles
            .len()
            .cmp(&a.titles.len())
            .then(b.calls.cmp(&a.calls))
            .then_with(|| a.library.cmp(&b.library))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Ranks refusal rows: most titles first, then most shaders, then by reason.
fn rank_refusals(rows: &mut [RefusalRow]) {
    rows.sort_by(|a, b| {
        b.titles
            .len()
            .cmp(&a.titles.len())
            .then(b.shaders.cmp(&a.shaders))
            .then_with(|| a.reason.cmp(&b.reason))
    });
}

/// Every persisted call trace, read once: the census needs call counts for imports and shader
/// failures for refusals, and both come from the same files.
fn read_traces() -> Vec<orbistoun_report::trace::CallTrace> {
    let dir = orbistoun_paths::Paths::resolve().traces_dir();
    std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect()
}

/// Calls per trace label, totalled across the traces of the selected titles.
fn calls_by_label(
    traces: &[orbistoun_report::trace::CallTrace],
    filter: &[String],
) -> BTreeMap<String, u64> {
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    for trace in traces {
        if !selected(&trace_title(&trace.module), filter) {
            continue;
        }
        for call in &trace.calls {
            let total = totals.entry(call.label.clone()).or_default();
            *total = total.saturating_add(call.calls);
        }
    }
    totals
}

/// The guest a trace came from, as the title library names it: the directory its executable sits
/// in, so a shipped module's trace counts toward its title.
fn trace_title(module: &str) -> String {
    let parts: Vec<&str> = module
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    match parts.as_slice() {
        [.., parent, _file] => (*parent).to_owned(),
        [only] => (*only).to_owned(),
        _ => module.to_owned(),
    }
}

/// Every module a title runs: its executable and the modules it ships that answer its imports.
fn title_modules(service: &Service, eboot: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut modules = vec![eboot.to_path_buf()];
    if let Ok(found) = service.title_modules_for(eboot) {
        modules.extend(found.into_iter().map(|m| m.path));
    }
    modules
}

/// The import rows for every selected title in the library.
fn gather_imports(
    service: &Service,
    filter: &[String],
    calls: &BTreeMap<String, u64>,
) -> (Vec<ImportRow>, usize) {
    use orbistoun_hle::origin::{self, Origin};

    let implemented: BTreeSet<String> = service
        .declared_symbols()
        .iter()
        .filter(|d| d.implemented)
        .map(|d| d.symbol.clone())
        .collect();
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let mut rows: BTreeMap<(String, String), ImportRow> = BTreeMap::new();
    let mut titles = 0_usize;

    for entry in std::fs::read_dir(library_or(None))
        .into_iter()
        .flatten()
        .flatten()
    {
        let title = entry.file_name().to_string_lossy().into_owned();
        let eboot = entry.path().join("eboot.bin");
        if !eboot.is_file() || !selected(&title, filter) {
            continue;
        }
        titles += 1;
        for module in title_modules(service, &eboot) {
            let Ok(survey) = service.survey_path(&module) else {
                continue;
            };
            for import in &survey.imports {
                let library = import.library.clone().unwrap_or_else(|| "?".to_owned());
                let name = import
                    .symbol
                    .clone()
                    .unwrap_or_else(|| orbistoun_nid::Nid::from_raw(import.nid).to_string());
                // The title's own modules answer these; loading them is the work, not measuring.
                if import.symbol.is_some() && origin::of(&library, &name) == Origin::TitleOwn {
                    continue;
                }
                let known_by = knowledge
                    .get(&name)
                    .and_then(|k| k.known_by)
                    .map_or("-", |o| o.label())
                    .to_owned();
                if is_settled(&known_by) {
                    continue;
                }
                let label = format!("{library}::{name}");
                let row = rows
                    .entry((library.clone(), name.clone()))
                    .or_insert_with(|| ImportRow {
                        implemented: implemented.contains(&name),
                        calls: calls.get(&label).copied().unwrap_or(0),
                        library,
                        name,
                        known_by,
                        titles: BTreeSet::new(),
                    });
                row.titles.insert(title.clone());
            }
        }
    }

    let mut rows: Vec<ImportRow> = rows.into_values().collect();
    rank_imports(&mut rows);
    (rows, titles)
}

/// The refusal rows from every selected trace.
fn gather_refusals(
    traces: &[orbistoun_report::trace::CallTrace],
    filter: &[String],
) -> Vec<RefusalRow> {
    let mut rows: BTreeMap<String, RefusalRow> = BTreeMap::new();
    for trace in traces {
        let title = trace_title(&trace.module);
        if !selected(&title, filter) {
            continue;
        }
        let Some(submission) = &trace.submission else {
            continue;
        };
        for failure in &submission.shader_failures {
            let reason = refusal_reason(failure);
            let row = rows.entry(reason.clone()).or_insert_with(|| RefusalRow {
                reason,
                shaders: 0,
                titles: BTreeSet::new(),
            });
            row.shaders += 1;
            row.titles.insert(title.clone());
        }
    }
    let mut rows: Vec<RefusalRow> = rows.into_values().collect();
    rank_refusals(&mut rows);
    rows
}

/// A title set as one comma-separated cell.
fn joined(titles: &BTreeSet<String>) -> String {
    titles.iter().cloned().collect::<Vec<_>>().join(",")
}

/// The titles column of an import row: `all` when every scanned title reaches it, since the
/// list is then the whole library and says nothing; the JSON keeps the names either way.
fn title_cell(titles: &BTreeSet<String>, scanned: usize) -> String {
    if titles.len() == scanned {
        "all".to_owned()
    } else {
        joined(titles)
    }
}

fn print_imports(rows: &[ImportRow], titles: usize, top: usize) {
    println!(
        "{} imports across {titles} title(s) with no measured or published answer",
        rows.len()
    );
    println!(
        "{:>6}  {:>12}  {:<14}  {:<4}  IMPORT  [TITLES]",
        "TITLES", "CALLS", "KNOWN BY", "IMPL"
    );
    for row in rows.iter().take(top) {
        println!(
            "{:>6}  {:>12}  {:<14}  {:<4}  {}::{}  [{}]",
            row.titles.len(),
            row.calls,
            row.known_by,
            if row.implemented { "yes" } else { "no" },
            row.library,
            row.name,
            title_cell(&row.titles, titles)
        );
    }
    if rows.len() > top {
        // Said out loud: a list truncated silently reads as a list that ended.
        println!("... and {} more (--top to see further)", rows.len() - top);
    }
}

fn print_refusals(rows: &[RefusalRow], top: usize) {
    println!();
    println!(
        "{} shader refusal reason(s) in the persisted traces - each trace keeps its first submission's failures",
        rows.len()
    );
    println!("{:>6}  {:>7}  REASON  [TITLES]", "TITLES", "SHADERS");
    for row in rows.iter().take(top) {
        println!(
            "{:>6}  {:>7}  {}  [{}]",
            row.titles.len(),
            row.shaders,
            row.reason,
            joined(&row.titles)
        );
    }
    if rows.len() > top {
        println!("... and {} more (--top to see further)", rows.len() - top);
    }
}

/// What `census` was asked for.
pub(crate) struct CensusRequest<'a> {
    /// List imports.
    pub imports: bool,
    /// List refusals.
    pub refusals: bool,
    /// Title-directory prefixes, comma-separated; empty means every title.
    pub titles: Option<&'a str>,
    /// Print JSON instead of a table.
    pub json: bool,
    /// How many rows of each table to print.
    pub top: usize,
}

/// `census` - rank what no measurement covers, across the corpus.
///
/// Neither `--imports` nor `--refusals` means both.
pub(crate) fn cmd_census(service: &Service, request: &CensusRequest<'_>) -> Result<()> {
    let filter: Vec<String> = request
        .titles
        .map(|t| {
            t.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let both = !request.imports && !request.refusals;
    let traces = read_traces();

    let imports = (request.imports || both).then(|| {
        let calls = calls_by_label(&traces, &filter);
        gather_imports(service, &filter, &calls)
    });
    let refusals = (request.refusals || both).then(|| gather_refusals(&traces, &filter));

    if request.json {
        let value = serde_json::json!({
            "titles": imports.as_ref().map(|(_, n)| *n),
            "imports": imports.as_ref().map(|(rows, _)| rows),
            "refusals": refusals,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    if let Some((rows, titles)) = &imports {
        print_imports(rows, *titles, request.top);
    }
    if let Some(rows) = &refusals {
        print_refusals(rows, request.top);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Only a measurement or a published source takes an import off the census.
    ///
    /// Differential agreement is still not the target, and an assumption is the census's whole
    /// subject (D818).
    #[test]
    fn only_measured_and_published_settle_an_import() {
        assert!(is_settled("measured"));
        assert!(is_settled("published"));
        for open in ["differential", "guest-observed", "assumed", "-"] {
            assert!(!is_settled(open), "{open} leaves the import on the census");
        }
    }

    /// Two shaders refused for one reason count as one row, whatever their stage and address.
    #[test]
    fn a_refusal_is_keyed_by_its_reason_not_its_address() {
        assert_eq!(
            refusal_reason("Fragment at 0x2008f0000: unsupported image_bvh_intersect_ray"),
            "unsupported image_bvh_intersect_ray"
        );
        assert_eq!(
            refusal_reason("Mesh at 0x1000: unsupported image_bvh_intersect_ray"),
            "unsupported image_bvh_intersect_ray"
        );
        // A failure recorded without a prefix is its own reason.
        assert_eq!(refusal_reason("no stage translates"), "no stage translates");
        // Addresses inside the reason are made the same, so two shaders at different
        // addresses refused for one instruction share a row.
        assert_eq!(
            refusal_reason(
                "Compute: the shader at 0x7400014c3800 could not be translated: instruction at 0x4 has no operand layout"
            ),
            refusal_reason(
                "Compute: the shader at 0x7400014d3800 could not be translated: instruction at 0x4 has no operand layout"
            )
        );
        assert_eq!(
            refusal_reason("Mesh: the shader at 0x7400014d3800 failed"),
            "the shader at 0x_ failed"
        );
        // A bare `0x` with no digits is left as it is.
        assert_eq!(refusal_reason("Mesh: prefix 0x then"), "prefix 0x then");
    }

    /// A row every scanned title reaches prints as `all` rather than the whole list.
    #[test]
    fn a_row_every_title_reaches_prints_as_all() {
        assert_eq!(title_cell(&titles(&["A", "B"]), 2), "all");
        assert_eq!(title_cell(&titles(&["A"]), 2), "A");
    }

    /// A title filter is a prefix, so a bare id selects its `-app0` directory.
    #[test]
    fn a_title_filter_selects_by_prefix() {
        let filter = vec!["PPSA02664".to_owned()];
        assert!(selected("PPSA02664-app0", &filter));
        assert!(!selected("PPSA03416-app0", &filter));
        assert!(selected("anything", &[]), "no filter selects everything");
    }

    /// Reach ranks first: an import three titles need beats one a single title calls often.
    #[test]
    fn imports_rank_by_titles_reached_then_calls() {
        let row = |name: &str, reach: &[&str], calls: u64| ImportRow {
            library: "libSceX".to_owned(),
            name: name.to_owned(),
            known_by: "-".to_owned(),
            implemented: false,
            titles: titles(reach),
            calls,
        };
        let mut rows = vec![
            row("often", &["A"], 1_000_000),
            row("wide", &["A", "B", "C"], 1),
            row("middle", &["A", "B"], 5),
        ];
        rank_imports(&mut rows);
        let order: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(order, ["wide", "middle", "often"]);
    }

    /// Refusals rank the same way: titles reached, then how many shaders.
    #[test]
    fn refusals_rank_by_titles_reached_then_shaders() {
        let row = |reason: &str, reach: &[&str], shaders: usize| RefusalRow {
            reason: reason.to_owned(),
            shaders,
            titles: titles(reach),
        };
        let mut rows = vec![row("many", &["A"], 40), row("wide", &["A", "B"], 2)];
        rank_refusals(&mut rows);
        assert_eq!(rows[0].reason, "wide");
    }

    /// A trace counts toward the title whose directory holds its module.
    #[test]
    fn a_trace_belongs_to_the_directory_its_module_sits_in() {
        assert_eq!(
            trace_title("titles/PPSA02664-app0/eboot.bin"),
            "PPSA02664-app0"
        );
        assert_eq!(
            trace_title("C:\\lib\\titles\\PPSA21564-app0\\eboot.bin"),
            "PPSA21564-app0"
        );
    }
}
