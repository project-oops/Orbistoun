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
/// A thin wrapper over `orbistoun_report::trace::load_previous`, kept only to supply the
/// traces directory - the reading and the format knowledge live below the shims, because
/// the GUI compares runs from the same files (D160).
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

/// Every measurement a turn established, in the shape the learned file keeps.
///
/// **Everything the run knew and used to throw away.** Which guest demonstrated it, when,
/// which build, and what the claim rests on that nothing measured - all of it was printed to a
/// terminal and lost before the file carried it, and all of it is what makes an entry
/// checkable by somebody else (D297).
pub(crate) fn measurements(
    title: &std::path::Path,
    plan: &[orbistoun_turn::turn::Step],
    taken: &[orbistoun_turn::turn::Taken],
) -> Vec<orbistoun_hle::learned::Measurement> {
    use orbistoun_hle::learned::{Evidence, Measurement};
    use orbistoun_turn::{patch, turn};

    let mut out = Vec::new();
    for (step, result) in plan.iter().zip(taken.iter()) {
        // Two shapes produce a measurement: a swept out-parameter contract, and the function
        // whose placeholder the guest was found to be dereferencing. The second is the only
        // one keepable on a moved wall, because it writes nothing (D296, D299).
        // The qualified name travels beside the patch, because the library half is the part
        // `from_finding` strips and the part a promotion needs (D328).
        let proposed = match (step, result) {
            (turn::Step::SweepArguments { target }, turn::Taken::Swept(finding)) => {
                patch::from_finding(target, finding).map(|patch| (target.as_str(), patch))
            }
            // **Whichever of the two answers reached further.** Both were run; recording the
            // rule's one regardless would make the comparison decorative (D300).
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
            // The containing directory, not the path: a path is a fact about one machine, and
            // an entry meant to travel should carry nothing about the sender's disk.
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
/// The containing directory, which is the same identifier the knowledge files already use
/// in `found_in` - and deliberately **not** the file name: a bare `eboot.bin` is identical
/// in every title and would have them all sharing one record.
pub(crate) fn title_id(path: &std::path::Path) -> Option<String> {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
}

/// A count with thousands separated, because one title makes ninety-nine million calls.
///
/// The hand-written table had these and the first generated one did not, which is a small
/// thing and exactly the kind of small thing that makes a generated table read as a
/// regression rather than a repair.
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
/// Keyed by the bare function name so it joins against the knowledge base, which does not
/// know about libraries the way a trace label does.
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
/// The subtype comes first because it is the closed vocabulary - it is what can be
/// counted and grepped - and the free text after it is the part only a person reads.
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
