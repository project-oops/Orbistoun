//! Talking to a live probe: `ask`, `session` and `probe`.

use anyhow::{Context, Result};
use orbistoun_service::Service;

/// Asks a probe one question.
///
/// The rawest surface onto the protocol: it prints what came back and does not grade it, file it or
/// judge it. `died`, `timeout` and `lost` print as themselves with no value, since the probe dying
/// is a normal outcome and not a result.
pub(crate) fn cmd_ask(
    address: &str,
    key: Option<&str>,
    command: &[String],
    budget: u64,
    as_knowledge: bool,
    origin: &orbistoun_probe::Origin,
) -> Result<()> {
    let address = if address.contains(':') {
        address.to_owned()
    } else {
        format!("{address}:{}", orbistoun_probe::client::DEFAULT_PORT)
    };
    let budget = std::time::Duration::from_secs(budget);

    let mut client = orbistoun_probe::client::connect(&address, budget)
        .with_context(|| format!("connecting to {address}"))?;
    let session = client
        .hello(orbistoun_probe::VERSION, key)
        .map_err(|e| anyhow::anyhow!("negotiating with {address}: {e}"))?;

    let (verb, arguments) = command.split_first().expect("clap requires one");
    let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let answer = client.command(verb, &borrowed);
    let _ = client.bye();

    // Only a `call` renders as knowledge: `read` and `report` establish other things, and filing
    // them would put a byte count where a return value belongs.
    if as_knowledge {
        return render_asked(&answer, verb, arguments, origin);
    }

    match answer {
        Ok(answer) => {
            // Records that arrived before the answer - `bytes` from a read, or a report's stream -
            // are often the point of the question.
            for record in &answer.records {
                println!("{record:?}");
            }
            println!("{}", answer.outcome);
            if !answer.detail.is_empty() {
                println!("  {}", answer.detail);
            }
            // A memory read is worth rendering as memory rather than as records.
            let transcript = orbistoun_probe::Transcript::read(&client.transcript().join("\n"))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let memory = transcript.memory();
            if !memory.bytes.is_empty() {
                println!("\n{} byte(s):", memory.bytes.len());
                for (offset, chunk) in memory.bytes.chunks(16).enumerate() {
                    let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
                    println!("  {:04x}  {}", offset * 16, hex.join(" "));
                }
            }
            if memory.undecodable > 0 {
                println!(
                    "  {} run(s) could not be decoded and were not guessed at",
                    memory.undecodable
                );
            }
        }
        Err(e) => println!("refused: {e}"),
    }
    println!("\nsession {session}");
    Ok(())
}

/// Renders a live answer as the knowledge entry it would become.
///
/// Only a `call`: the rule is about what a function returns. A `read` establishes memory contents
/// and a `report` a suite's results, so anything else says so instead of producing an entry.
fn render_asked(
    answer: &std::result::Result<
        orbistoun_probe::client::Answer,
        orbistoun_probe::client::ClientError,
    >,
    verb: &str,
    arguments: &[String],
    origin: &orbistoun_probe::Origin,
) -> Result<()> {
    if verb != "call" {
        println!("`{verb}` establishes something, but not what a function returns");
        println!("  only `call` produces a knowledge entry");
        return Ok(());
    }
    let Ok(answer) = answer else {
        println!("nothing to record: the command was refused before it ran");
        return Ok(());
    };

    let parsed: Vec<u64> = arguments
        .iter()
        .filter_map(|argument| {
            let body = argument.strip_prefix("0x").unwrap_or(argument);
            u64::from_str_radix(body, 16).ok()
        })
        .collect();
    let symbol = parsed
        .first()
        .map_or_else(|| "<unknown>".to_owned(), |address| format!("{address:#x}"));

    // The return kind decides whether the guest may use the value, and this command was given an
    // address, not a name. So it records only, the safe reading when the return kind is unknown.
    let asked = orbistoun_probe::Asked {
        symbol,
        arguments: parsed.iter().skip(1).copied().collect(),
        outcome: answer.outcome.clone(),
        usable: orbistoun_probe::usable(None),
    };

    let entry = asked.knowledge(origin);
    let file = orbistoun_hle::knowledge::KnowledgeFile {
        library: "<unknown - asked by address>".to_owned(),
        functions: vec![entry],
    };
    print!("{}", file.render().context("rendering knowledge")?);
    println!("# recorded only: this was asked by address, so the return kind is unknown");
    Ok(())
}

/// Drives a live session and writes the transcript out.
///
/// Everything downstream - grading, findings, knowledge entries - reads files, not sockets, so a
/// session that leaves nothing on disk produced nothing. The operator's assertion about the machine
/// is written into the file as a comment so the transcript can be graded later.
pub(crate) fn cmd_session(
    address: &str,
    key: Option<&str>,
    out: &std::path::Path,
    device: Option<&str>,
    firmware: Option<&str>,
    is_target: bool,
    budget: u64,
) -> Result<()> {
    use std::io::Write as _;

    let address = if address.contains(':') {
        address.to_owned()
    } else {
        format!("{address}:{}", orbistoun_probe::client::DEFAULT_PORT)
    };
    let budget = std::time::Duration::from_secs(budget);

    let mut client = orbistoun_probe::client::connect(&address, budget)
        .with_context(|| format!("connecting to {address}"))?;
    let session = client
        .hello(orbistoun_probe::VERSION, key)
        .map_err(|e| anyhow::anyhow!("negotiating with {address}: {e}"))?;
    println!("session {session}");

    // Only what the probe announced: a reserved verb it does not implement is not sent.
    if client.can(&orbistoun_probe::Capability::Report) {
        match client.report() {
            Ok(answer) => println!("report {}", answer.outcome),
            // A command that did not answer is the finding, not a client error. It is recorded and
            // the session closes cleanly.
            Err(e) => println!("report failed: {e}"),
        }
    } else {
        println!("report not announced by this probe");
    }
    let _ = client.bye();

    let mut file =
        std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?;
    writeln!(file, "# session {session} against {address}")?;
    writeln!(
        file,
        "# operator asserts: device={} firmware={} is-target={}",
        device.unwrap_or("unasserted"),
        firmware.unwrap_or(""),
        is_target
    )?;
    for line in client.transcript() {
        writeln!(file, "{line}")?;
    }
    println!("written {}", out.display());
    println!(
        "\nread it back with: orbistoun probe {} {}",
        out.display(),
        if is_target {
            "--device <name> --is-target"
        } else {
            "--device <name>"
        }
    );
    Ok(())
}

/// Reads a probe transcript and reports what it establishes.
///
/// Read-only and file-based, so the gate needs no hardware attached (D207).
pub(crate) fn cmd_probe(
    path: &std::path::Path,
    device: Option<String>,
    firmware: Option<String>,
    is_target: bool,
    as_knowledge: bool,
    against: Option<&std::path::Path>,
    service: &Service,
) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let transcript = orbistoun_probe::Transcript::read(&text)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;

    // The operator's assertion, or its absence. A probe inside an emulator reports that emulator's
    // version as the platform's, so `target|console` on the wire is a claim, not evidence.
    let origin = if let Some(device) = device {
        {
            // A name known to be a stand-in stays one; anything else is the target only when
            // `--is-target` says so. The list names stand-ins rather than targets, so an unlisted
            // emulator is demoted: a wrong demotion is recoverable, a wrong promotion corrupts the
            // knowledge base.
            let target = is_target && !orbistoun_probe::Origin::is_known_stand_in(&device);
            if is_target && !target {
                println!("note      `{device}` is a known stand-in, so --is-target was ignored");
            }
            orbistoun_probe::Origin::asserted(device, firmware.unwrap_or_default(), target)
        }
    } else {
        {
            // Nothing asserted, nothing claimed: the session is recorded in full and every result
            // grades as an assumption.
            if is_target {
                println!("note      --is-target names nothing without --device, so it was ignored");
            }
            orbistoun_probe::Origin::unasserted()
        }
    };
    let origin = transcript.sessions.first().map_or_else(
        || origin.clone(),
        |session| origin.clone().with_claims(session),
    );

    print_origin(&transcript, &origin);
    print!("{}", transcript.established(&origin));

    if as_knowledge {
        return print_knowledge(&transcript, &origin);
    }

    // Facts first, and named: a function with a measured return value is the form this project can
    // act on.
    let findings = transcript.findings(&origin);
    let facts: Vec<_> = findings.iter().filter(|f| f.is_fact()).collect();
    if !facts.is_empty() {
        println!("\nestablished");
        for finding in facts {
            let grade = finding.known_by.map_or("?", |oracle| oracle.label());
            println!(
                "  {:<9} {}::{} = {} [{grade}]",
                format!("{:?}", finding.status).to_lowercase(),
                finding.library,
                finding.symbol,
                if finding.value.is_empty() {
                    "(no value)"
                } else {
                    &finding.value
                }
            );
        }
    }

    print_self_report(&transcript);
    print_sections(&transcript);
    print_measurements(&transcript, &origin, service);
    if let Some(reference) = against {
        print_divergences(&transcript, reference)?;
    }

    // Symbols, separately from results, graded the same way by the same origin: a name resolving on
    // a stand-in reflects the stand-in's name list, not the platform (D246).
    let symbols = transcript.symbols(&origin);
    if !symbols.is_empty() {
        let absent = symbols.iter().filter(|s| !s.present).count();
        println!(
            "\nsymbols {} resolved, {absent} absent",
            symbols.len() - absent
        );
        for symbol in &symbols {
            // Named for what the record carried: a `sym` record says how the symbol is reached, a
            // `resolve` record says where it landed.
            let detail = match (&symbol.availability, &symbol.address) {
                (Some(how), _) => format!("via {how}"),
                (None, Some(at)) => format!("at {at}"),
                (None, None) => "no detail recorded".to_owned(),
            };
            // Said on the line: a fact that may source a name and one that may not otherwise look
            // identical (D246).
            let source = if symbol.may_source_a_name() {
                ""
            } else {
                "  - not a naming source"
            };
            println!(
                "  {:<8} {}::{} ({detail}){source}",
                if symbol.present { "present" } else { "ABSENT" },
                symbol.library,
                symbol.symbol,
            );
        }
    }

    // Calls that were announced and never concluded. Listed separately and never counted as
    // failures: nothing was observed about them.
    let unfinished = transcript.attempted_without_result();
    if !unfinished.is_empty() {
        println!("\nannounced and never concluded - each one ended the probe");
        for (check, library, symbol) in unfinished {
            println!("  {library}::{symbol}  ({check})");
        }
    }
    Ok(())
}

/// Prints what the run measured, per section, and decodes the one section this project can act on
/// directly.
///
/// The tally says what each section measured and that this reader decoded nothing from it, so
/// unread `measure` records stay visible and name what to teach this command next.
fn print_measurements(
    transcript: &orbistoun_probe::Transcript,
    origin: &orbistoun_probe::Origin,
    service: &Service,
) {
    let sections = transcript.measured_sections();
    if sections.is_empty() {
        return;
    }
    let total: usize = sections.values().sum();
    println!("\nmeasurements  {total} in {} section(s)", sections.len());
    for (section, count) in &sections {
        let read = if section == orbistoun_probe::KEXPORT_SECTION {
            ""
        } else {
            "  - carried, not interpreted"
        };
        println!("  {count:>5}  {section}{read}");
    }

    let exports = transcript.kernel_exports(origin);
    if exports.is_empty() {
        return;
    }
    print_kernel_exports(&exports, service);
}

/// Prints the kernel export table the probe read, against the names this project holds.
///
/// It shows which hashes the platform exports whether or not anything imported them, a census no
/// collision search reaches, and which hashes share an address: two hashes at one address are one
/// function, so a name for either makes the other a testable variant.
fn print_kernel_exports(exports: &[orbistoun_probe::KernelExport], service: &Service) {
    let name_of = |export: &orbistoun_probe::KernelExport| -> Option<&str> {
        service.symbol_name(export.nid)
    };

    let named = exports.iter().filter(|e| name_of(e).is_some()).count();
    let addresses: std::collections::BTreeSet<u64> = exports.iter().map(|e| e.vaddr).collect();
    // The grade is the same for every entry, so it is stated once: a table read off a stand-in and
    // one read off the target are the same bytes and different evidence (D246).
    let grade = exports
        .first()
        .map_or("?", |export| export.known_by.label());
    println!(
        "\nkernel exports  {} at {} address(es), {named} named by this project, {} not [{grade}]",
        exports.len(),
        addresses.len(),
        exports.len() - named,
    );
    if service.symbol_db_len().is_none() {
        println!("  no symbol database loaded, so nothing could be named");
    }

    // The naming lead: an alias group with one side named makes the unnamed side a variant of a
    // name we hold, a hash to derive rather than guess.
    let aliases = orbistoun_probe::export_aliases(exports);
    let leads: Vec<_> = aliases
        .iter()
        .filter(|alias| {
            let named = alias
                .exports
                .iter()
                .filter(|e| name_of(e).is_some())
                .count();
            named > 0 && named < alias.exports.len()
        })
        .collect();
    println!(
        "  {} address(es) export more than one hash; {} of those have a named side",
        aliases.len(),
        leads.len(),
    );
    for alias in leads {
        let spelled: Vec<String> = alias
            .exports
            .iter()
            .map(|export| name_of(export).map_or_else(|| format!("{}", export.nid), str::to_owned))
            .collect();
        println!("    {:#x}  {}", alias.vaddr, spelled.join("  =  "));
    }
    print_wanted_exports(exports, service, &orbistoun_paths::Paths::resolve());
}

/// Whether the kernel export table answers a hash the corpus could not name.
///
/// The useful set is the intersection: a hash a running guest calls, that this project cannot name,
/// and that the export table holds an address for. A count of unnamed exports cannot say whether a
/// given wanted hash is among them. Silent when the table answers none.
fn print_wanted_exports(
    exports: &[orbistoun_probe::KernelExport],
    service: &Service,
    paths: &orbistoun_paths::Paths,
) {
    let wanted = hashes_a_guest_wanted(paths);
    if wanted.is_empty() {
        return;
    }
    let held: std::collections::BTreeMap<u64, u64> =
        exports.iter().map(|e| (e.nid.as_raw(), e.vaddr)).collect();
    let found: Vec<(&str, u64, u64)> = wanted
        .iter()
        .filter_map(|(label, nid)| held.get(nid).map(|vaddr| (label.as_str(), *nid, *vaddr)))
        .collect();

    println!(
        "\n  of {} hash(es) a guest here calls and nothing can name, this table holds {}",
        wanted.len(),
        found.len()
    );
    for (label, nid, vaddr) in &found {
        // The address is what makes it a lead: another hash at the same address is an alias, and an
        // alias with a named side names this one.
        let alias = exports
            .iter()
            .filter(|e| e.vaddr == *vaddr && e.nid.as_raw() != *nid)
            .find_map(|e| service.symbol_name(e.nid));
        match alias {
            Some(name) => println!("    {label}  is {name}, aliased at {vaddr:#x}"),
            None => println!(
                "    {label}  at {vaddr:#x}, with no named hash at that address to derive from"
            ),
        }
    }
}

/// Hashes a guest here has called and nothing can name, from the persisted traces.
///
/// Read from the same traces `worklist` ranks, so the two agree on what "unnamed" means.
fn hashes_a_guest_wanted(paths: &orbistoun_paths::Paths) -> Vec<(String, u64)> {
    let Ok(entries) = std::fs::read_dir(paths.traces_dir()) else {
        return Vec::new();
    };
    let mut out: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
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
            let Some((_, hash)) = call.label.split_once("::0x") else {
                continue;
            };
            if let Ok(nid) = u64::from_str_radix(hash, 16) {
                out.insert(call.label.clone(), nid);
            }
        }
    }
    out.into_iter().collect()
}

/// Prints the checks two transcripts disagree about, worst first.
///
/// A check that passed on the reference and failed here is a defect; one that passed there and
/// partially passed here is a lead; any other disagreement is usually a difference in reach and is
/// listed last. A check only one side ran is not listed: that is reach, not behaviour.
fn print_divergences(
    subject: &orbistoun_probe::Transcript,
    reference: &std::path::Path,
) -> Result<()> {
    use orbistoun_probe::Status;

    let text = std::fs::read_to_string(reference)
        .with_context(|| format!("reading {}", reference.display()))?;
    let other = orbistoun_probe::Transcript::read(&text)
        .map_err(|e| anyhow::anyhow!("{}: {e}", reference.display()))?;

    let diverged = subject.diverges_from(&other);
    let both_ran = subject
        .verdicts()
        .keys()
        .filter(|c| other.verdicts().contains_key(*c))
        .count();
    println!(
        "\nagainst {}: {} of {both_ran} check(s) both ran concluded differently",
        reference.display(),
        diverged.len()
    );
    if diverged.is_empty() {
        return Ok(());
    }
    let groups = group_divergences(&diverged);
    // The count of defects, and how many distinct sentences they carry: many are often the same
    // sentence about different absent libraries.
    let defects = diverged
        .iter()
        .filter(|d| d.reference == Status::Pass && d.subject == Status::Fail)
        .count();
    let findings = groups
        .iter()
        .filter(|g| *g.reference == Status::Pass && *g.subject == Status::Fail)
        .count();
    println!(
        concat!(
            "  {} passed there and failed here, saying {} distinct thing(s) - ",
            "a sentence repeated across a family is one finding"
        ),
        defects, findings
    );
    for group in &groups {
        print_divergence_group(group);
    }
    Ok(())
}

/// One transition, one sentence, and every check that concluded it.
struct DivergenceGroup<'a> {
    reference: &'a orbistoun_probe::Status,
    subject: &'a orbistoun_probe::Status,
    detail: &'a str,
    checks: Vec<&'a str>,
}

/// Collects divergences that differ only in which check said them.
///
/// Ordered smallest group first within each transition class, in the order classes first appear: a
/// finding one check made is specific, and a large group is a census of what this build lacks.
fn group_divergences<'a>(diverged: &'a [orbistoun_probe::Divergence]) -> Vec<DivergenceGroup<'a>> {
    let mut order: Vec<(&orbistoun_probe::Status, &orbistoun_probe::Status)> = Vec::new();
    let mut groups: Vec<DivergenceGroup<'a>> = Vec::new();
    for d in diverged {
        if !order.contains(&(&d.reference, &d.subject)) {
            order.push((&d.reference, &d.subject));
        }
        if let Some(existing) = groups.iter_mut().find(|g| {
            *g.reference == d.reference && *g.subject == d.subject && g.detail == d.detail
        }) {
            existing.checks.push(&d.check);
        } else {
            groups.push(DivergenceGroup {
                reference: &d.reference,
                subject: &d.subject,
                detail: &d.detail,
                checks: vec![&d.check],
            });
        }
    }
    groups.sort_by_key(|g| {
        let class = order
            .iter()
            .position(|c| c.0 == g.reference && c.1 == g.subject)
            .unwrap_or(usize::MAX);
        (class, g.checks.len())
    });
    groups
}

/// Prints one group: the transition, the sentence, and who said it.
///
/// A larger group leads with its size, because the size is the finding: one fact about the build,
/// not one per library.
fn print_divergence_group(group: &DivergenceGroup<'_>) {
    /// How many members of a family to name before summarising the rest.
    const NAMED: usize = 3;

    let transition = format!(
        "{:<8} -> {:<8}",
        format!("{:?}", group.reference).to_lowercase(),
        format!("{:?}", group.subject).to_lowercase(),
    );
    let detail = if group.detail.is_empty() {
        String::new()
    } else {
        format!("  {}", group.detail)
    };
    if group.checks.len() == 1 {
        println!("  {transition} {}{detail}", group.checks[0]);
        return;
    }
    println!("  {transition} x{}{detail}", group.checks.len());
    let named = group.checks.iter().take(NAMED).copied().collect::<Vec<_>>();
    let rest = group.checks.len().saturating_sub(named.len());
    let tail = if rest == 0 {
        String::new()
    } else {
        format!(", and {rest} more")
    };
    println!("                        {}{tail}", named.join(", "));
}

/// Prints what the target says about itself, marked as self-reported.
///
/// Three states can all read `unknown` - the platform has no such query, the probe has not wired
/// one, or a real value - and only one is anybody's defect. Inside an emulator every field answers
/// as the emulator chooses, so this is not machine identity; that is the operator's assertion,
/// printed separately.
fn print_self_report(transcript: &orbistoun_probe::Transcript) {
    use orbistoun_probe::Confidence;

    let report = transcript.self_report();
    if report.is_empty() {
        return;
    }
    println!(
        "
self-reported by the target (not evidence of what it is)"
    );
    for field in &report {
        let state = match &field.confidence {
            Confidence::Known => "",
            Confidence::Unconfirmed => "  [the probe cannot read this yet]",
            Confidence::Absent => "  [this platform has no such query]",
            Confidence::Unrecognised(other) => &format!("  [state {other:?} - unrecognised]"),
        };
        // `generation` has two readings a display can get wrong. `both` means two driver stacks are
        // present and names no hardware, because presence is not implementation. The parenthetical
        // names the graphics drivers the inference keyed on (`agc`, `gnm`) - evidence, not recency.
        let note = if field.field == "generation" {
            match field.value.as_str() {
                "both" => "  [two driver stacks present; this names no console]",
                v if v.contains("(agc)") || v.contains("(gnm)") => {
                    "  [the parenthetical is the driver this was inferred from, not a version]"
                }
                _ => state,
            }
        } else {
            state
        };
        println!("  {:<11} {}{note}", field.field, field.value);
    }
}

/// Prints each area of the platform and how much of it passed.
///
/// A single total says how much was checked, not what is understood; the same count spread thin or
/// concentrated in one area are different situations.
fn print_sections(transcript: &orbistoun_probe::Transcript) {
    let sections = transcript.sections();
    if sections.is_empty() {
        return;
    }
    let green = sections
        .iter()
        .filter(|section| section.is_wholly_green())
        .count();
    println!("\nareas {green} of {} wholly green", sections.len());
    for section in &sections {
        // A skip is shown, not folded into the total: it is a check that did not run.
        let counts = [
            ("pass", section.pass),
            ("partial", section.partial),
            ("fail", section.fail),
            ("skip", section.skip),
        ]
        .into_iter()
        // `pass` is shown even at zero, so a section where nothing passed does not look like a
        // section with no checks.
        .filter(|(label, count)| *count > 0 || *label == "pass")
        .map(|(label, count)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(", ");
        let mark = if section.is_wholly_green() { "+" } else { " " };
        println!(
            "  {mark} {:<20} {:<34} {counts}",
            section.id,
            if section.title.is_empty() {
                "(no section record)"
            } else {
                &section.title
            }
        );
    }
}

/// Prints what produced the answers, before any of the answers: a number is meaningless without the
/// machine that produced it.
fn print_origin(transcript: &orbistoun_probe::Transcript, origin: &orbistoun_probe::Origin) {
    println!("machine {} (operator-asserted)", origin.describe());
    if origin.is_target {
        println!("          asserted as the target platform - results may grade as measured");
    } else {
        println!("          not asserted as the real target - nothing grades above `assumed`");
    }
    if let Some((build, kind)) = transcript.build() {
        println!("build {build} ({kind})");
    }
    println!();

    for session in &transcript.sessions {
        println!("session {}", session.session);
        // What produced the answers, printed first.
        for (key, value) in &session.parts {
            println!("  {key:<9} {value}");
        }
        let mut capabilities: Vec<String> = session
            .capabilities
            .iter()
            .map(|capability| format!("{capability:?}").to_lowercase())
            .collect();
        capabilities.sort();
        println!("  can {}", capabilities.join(", "));
        // What the session claimed, printed as a claim: it matters most when it disagrees with the
        // operator's assertion.
        if let Some(claimed) = session.claimed_target() {
            println!("  claimed {claimed} (the probe's own word, not evidence)");
        }
        println!();
    }
}

/// Renders what was established as knowledge entries.
fn print_knowledge(
    transcript: &orbistoun_probe::Transcript,
    origin: &orbistoun_probe::Origin,
) -> Result<()> {
    let findings = transcript.findings(origin);
    // Grouped by library, as the knowledge base is filed, and rendered through its own serialiser
    // so what is printed is what would be written.
    let mut by_library: std::collections::BTreeMap<
        String,
        orbistoun_hle::knowledge::KnowledgeFile,
    > = std::collections::BTreeMap::new();
    for finding in &findings {
        let file = by_library
            .entry(finding.library.clone())
            .or_insert_with(|| orbistoun_hle::knowledge::KnowledgeFile {
                library: finding.library.clone(),
                functions: Vec::new(),
            });
        file.functions.push(finding.knowledge(origin));
    }
    for (library, file) in by_library {
        println!(
            "
# {library}"
        );
        print!("{}", file.render().context("rendering knowledge")?);
    }
    Ok(())
}
