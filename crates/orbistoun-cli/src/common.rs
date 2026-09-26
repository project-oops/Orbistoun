//! Helpers shared by several commands.

/// Formats a fraction as a percentage without a lossy `usize` cast.
pub(crate) fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    let part = f64::from(u32::try_from(part).unwrap_or(u32::MAX));
    let whole = f64::from(u32::try_from(whole).unwrap_or(u32::MAX));
    part / whole * 100.0
}

/// The trace a previous run of this module left behind.
///
/// Supplies the traces directory to `orbistoun_report::trace::load_previous`; reading the format
/// lives below the shims because the GUI compares runs from the same files (D034).
pub(crate) fn previous_trace(
    module: &std::path::Path,
) -> Option<orbistoun_report::trace::CallTrace> {
    let paths = orbistoun_paths::Paths::resolve();
    orbistoun_report::trace::load_previous(&paths.traces_dir(), module)
}

/// Where a library's knowledge file lives.
pub(crate) fn knowledge_path(library: &str) -> std::path::PathBuf {
    std::path::Path::new("crates/orbistoun-hle/data/knowledge").join(format!("{library}.toml"))
}

/// Every measurement a turn established, in the shape the learned file keeps (D297).
///
/// Each carries the guest that demonstrated it, the date, the build and what it assumes, so
/// somebody else can check it.
pub(crate) fn measurements(
    title: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Vec<orbistoun_hle::learned::Measurement> {
    use orbistoun_hle::learned::{Evidence, Measurement};
    use orbistoun_turn::{patch, turn};

    let mut out = Vec::new();
    for (step, result) in plan.iter().zip(taken.iter()) {
        // Two shapes produce a measurement: a swept out-parameter contract, and the function whose
        // placeholder the guest dereferences. The second writes nothing, so it is keepable on a
        // moved wall (D296). The qualified name travels beside the patch because `from_finding`
        // strips the library half and promotion needs it.
        let proposed = match (step, result) {
            (turn::Step::SweepArguments { target }, turn::Taken::Swept(finding)) => {
                patch::from_finding(target, finding).map(|patch| (target.as_str(), patch))
            }
            // The answer that reached further; both were run.
            (
                _,
                turn::Taken::Sourced {
                    function, answer, ..
                },
            ) => Some((
                function.as_str(),
                match answer {
                    turn::Answer::Zero => patch::from_placeholder_source(function),
                    turn::Answer::Region { .. } => {
                        patch::from_placeholder_source_as_region(function)
                    }
                },
            )),
            _ => None,
        };
        let Some((qualified, proposed)) = proposed else {
            continue;
        };
        out.push(Measurement {
            function: proposed.function,
            library: qualified
                .split_once("::")
                .map_or_else(String::new, |(library, _)| library.to_owned()),
            // The containing directory, not the path: an entry meant to travel carries nothing
            // about the sender's disk.
            measured: title_id(title).unwrap_or_else(|| "unknown".to_owned()),
            on: orbistoun_nid::today(),
            by: build_stamp(),
            known: orbistoun_hle::knowledge::Oracle::GuestObserved,
            evidence: match proposed.evidence {
                patch::Evidence::Further => Evidence::Further,
                patch::Evidence::ConformanceCheck => Evidence::ConformanceCheck,
            },
            answers: proposed.answers,
            region: proposed.region,
            assumes: proposed.assumptions,
        });
    }
    out
}

/// Which build established a measurement.
pub(crate) fn build_stamp() -> String {
    format!(
        "orbistoun {} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("ORBISTOUN_COMMIT").unwrap_or("unknown")
    )
}

/// `given`, or the one shared title library every OOPS tool reads (`orbistoun-cli paths`).
pub(crate) fn library_or(given: Option<&std::path::Path>) -> std::path::PathBuf {
    given.map_or_else(
        || orbistoun_paths::Paths::resolve().titles_dir(),
        std::path::Path::to_path_buf,
    )
}

/// The title a module path belongs to.
///
/// The containing directory, the same identifier the knowledge files use in `found_in`. Not the
/// file name: every title has a file called `eboot.bin`.
pub(crate) fn title_id(path: &std::path::Path) -> Option<String> {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
}

/// A count with thousands separated; a title can make tens of millions of calls.
pub(crate) fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (seen, ch) in digits.chars().rev().enumerate() {
        if seen > 0 && seen % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// How often each function has been called, across every trace on disk.
///
/// Keyed by the bare function name so it joins against the knowledge base, which does not key on
/// library.
pub(crate) fn calls_by_function() -> std::collections::BTreeMap<String, (u64, usize)> {
    let mut totals: std::collections::BTreeMap<String, (u64, usize)> =
        std::collections::BTreeMap::new();
    let paths = orbistoun_paths::Paths::resolve();
    let Ok(entries) = std::fs::read_dir(paths.traces_dir()) else {
        return totals;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(trace) = serde_json::from_str::<orbistoun_report::trace::CallTrace>(&text) else {
            continue;
        };
        for call in &trace.calls {
            // The label is `library::name`; the knowledge base keys on the name alone.
            let name = call.label.rsplit("::").next().unwrap_or(&call.label);
            let slot = totals.entry(name.to_owned()).or_insert((0, 0));
            slot.0 += call.calls;
            slot.1 += 1;
        }
    }
    totals
}

/// What a record says was done, in one line, for the audit's per-tier listing.
///
/// The subtype comes first because it is the closed vocabulary that can be counted and grepped; the
/// free text after it is for a person.
pub(crate) fn how_it_was_found(method: &orbistoun_nid::Method) -> String {
    use orbistoun_nid::{Method, RuntimeSource, StaticSource};
    match method {
        Method::Static { by, from } => {
            let by = match by {
                StaticSource::ModuleStrings => "module-strings",
                StaticSource::CrossModule => "cross-module",
                StaticSource::FirmwareLayout => "firmware-layout",
            };
            format!("{by}  {from}")
        }
        Method::Runtime { by, how } => {
            let by = match by {
                RuntimeSource::CallTrace => "call-trace",
                RuntimeSource::ArgumentDump => "argument-dump",
                RuntimeSource::ProbeTranscript => "probe-transcript",
            };
            format!("{by}  {how}")
        }
        Method::Supplied { source } => source.clone(),
        Method::PublishedStandard { list } => list.clone(),
        Method::Generated { pattern, index } => format!("{pattern}[{index}]"),
        Method::Affixed { seed, rule } => format!("{rule}  applied to {seed}"),
    }
}

#[cfg(test)]
mod tests {
    use super::percent;

    #[test]
    fn percent_handles_the_empty_case_without_dividing_by_zero() {
        assert!((percent(0, 0) - 0.0).abs() < f64::EPSILON);
        assert!((percent(1, 4) - 25.0).abs() < f64::EPSILON);
        assert!((percent(1410, 1410) - 100.0).abs() < f64::EPSILON);
    }
}
