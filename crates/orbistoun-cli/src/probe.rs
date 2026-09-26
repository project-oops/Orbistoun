//! Talking to a live probe: `ask`, `session` and `probe`.

use anyhow::{Context, Result};
use orbistoun_service::Service;

/// Asks a probe one question.
///
/// # Why the answer is printed rather than interpreted
///
/// This is the rawest surface onto the protocol and it stays that way deliberately. It
/// prints what came back and does not decide what it means - no grading, no knowledge
/// entry, no judgement about whether the value is usable. Those are decisions with rules
/// attached, and a command whose whole job is "ask the console" should not quietly make
/// them.
///
/// What it will not do is flatter a non-answer. `died`, `timeout` and `lost` print as
/// themselves and carry no value, because the probe dying is the normal case here and a
/// tool that rendered it as a result would be the one thing this whole effort is against.
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

    // Rendering the answer as knowledge is the whole point of asking: a value that stays in
    // a terminal has to be asked for again tomorrow. Only a `call` produces one - `read` and
    // `report` establish other things, and pretending otherwise would file a byte count as a
    // function's return value.
    if as_knowledge {
        return render_asked(&answer, verb, arguments, origin);
    }

    match answer {
        Ok(answer) => {
            // Records that arrived before the answer - `bytes` from a read, or a report's
            // stream - are shown, because they are frequently the point of the question.
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
/// # Why only a `call`
///
/// The rule is about what a *function returns*. A `read` establishes what some memory holds
/// and a `report` establishes a suite's results; filing either as a return value would put a
/// byte count where a function's answer belongs. So anything else says so rather than
/// producing a plausible entry.
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

    // The return kind decides whether the guest may use it, and nothing here knows it: this
    // command was given an address, not a name. So it records only, which is the safe
    // reading - not knowing what a function returns is exactly when handing its value over
    // is most dangerous.
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
/// # Why the file is the point
///
/// The session is the interface; the corpus is the product. A run that answers questions
/// and leaves nothing on disk has produced nothing, and everything downstream - grading,
/// findings, knowledge entries - reads files rather than sockets. So this connects, runs
/// the probe's suite, and writes what it saw.
///
/// The operator's assertion about the machine is written into the file as a comment,
/// because a transcript that has to be joined against a memory of who ran it is a
/// transcript nobody can grade later.
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

    // Only what it announced. Sending a reserved verb and waiting to be refused puts a
    // command on the wire that this probe does not implement, and on a target that faults
    // easily that is not free.
    if client.can(&orbistoun_probe::Capability::Report) {
        match client.report() {
            Ok(answer) => println!("report {}", answer.outcome),
            // A command that did not answer is not an error in the client - it is the
            // finding. It is recorded and the session continues to a clean close.
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
/// Deliberately read-only and deliberately file-based. A gate that needs a console plugged
/// in is a gate that fails for everyone else, so the corpus is the interface and the socket
/// is somebody else's problem (D207).
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

    // The operator's assertion, or the absence of one. Nothing here reads the machine
    // identity off the records: a probe running inside an emulator reports that emulator's
    // version as the platform's, so `target|console` on the wire is a claim somebody typed
    // and not evidence of anything.
    let origin = if let Some(device) = device {
        {
            // One question, not two. A name this project knows to be a stand-in stays one
            // without the operator saying so twice; anything else is taken at its word only
            // when `--is-target` says so.
            //
            // The list is of stand-ins rather than targets on purpose: an unrecognised name
            // defaults to *not the target*, so an emulator nobody has listed is demoted
            // rather than promoted. A wrong demotion is recoverable; the other direction is
            // the one that corrupts a knowledge base.
            let target = is_target && !orbistoun_probe::Origin::is_known_stand_in(&device);
            if is_target && !target {
                println!("note      `{device}` is a known stand-in, so --is-target was ignored");
            }
            orbistoun_probe::Origin::asserted(device, firmware.unwrap_or_default(), target)
        }
    } else {
        {
            // Nothing typed, nothing claimed. This is the safe default and the common case:
            // the session is recorded in full and every result grades as an assumption,
            // which is exactly what "nobody said what this ran on" means.
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

    // Facts first, and named. A count says how much was learned; this says what, and a
    // function with a measured return value is the only form this project can act on.
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

    // Symbols, separately from results, but graded the same way and by the same origin.
    // They used to be printed ungraded on the reasoning that a name resolving on a stand-in
    // is still spelled correctly - which is the stand-in's mined name list speaking, not the
    // platform (D246).
    let symbols = transcript.symbols(&origin);
    if !symbols.is_empty() {
        let absent = symbols.iter().filter(|s| !s.present).count();
        println!(
            "\nsymbols {} resolved, {absent} absent",
            symbols.len() - absent
        );
        for symbol in &symbols {
            // Whichever the record carried, named for what it is. A `sym` record says how
            // the symbol is reached and a `resolve` record says where it landed; printing
            // either under one unlabelled bracket would make them look like one field with
            // inconsistent contents (D245).
            let detail = match (&symbol.availability, &symbol.address) {
                (Some(how), _) => format!("via {how}"),
                (None, Some(at)) => format!("at {at}"),
                (None, None) => "no detail recorded".to_owned(),
            };
            // Said on the line, not inferred from the header. A fact that may source a
            // name and one that may not look identical otherwise, and the whole naming
            // rule turns on the difference (D242, D246).
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

    // A call that was announced and never concluded. Listed separately and never counted
    // as a failure: the probe said what it was about to do and did not come back, so
    // nothing was concluded about it. Reporting that as a failing check would be recording
    // an outcome nobody observed.
    let unfinished = transcript.attempted_without_result();
    if !unfinished.is_empty() {
        println!("\nannounced and never concluded - each one ended the probe");
        for (check, library, symbol) in unfinished {
            println!("  {library}::{symbol}  ({check})");
        }
    }
    Ok(())
}

/// Prints what the run measured, per section, and decodes the one section this project
/// can act on directly.
///
/// # Why the tally comes before the detail
///
/// `measure` is the most numerous record kind in every report this project has been given,
/// and until D605 none of them were read at all: they parsed into `Record::Other` and were
/// dropped, while the reader printed a record count that was perfectly correct. Nothing was
/// wrong except that the largest thing in the file was invisible.
///
/// So the tally is the point. It says what a section measured *and* that this reader made
/// nothing of it, which is the honest report of a consumer that lags its source - and it
/// names the next thing worth teaching this command to read (principle 3).
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

/// Prints the console's own kernel export table against the names this project holds.
///
/// # The two things this table says that nothing else does
///
/// **Which hashes the platform exports at all.** A hash from an import table is one a title
/// asked for; a hash from here is one the platform offers whether or not anything has ever
/// imported it. That is the census a collision search can never reach (D245).
///
/// **Which of them are the same function.** Two hashes at one address are one function under
/// two names, and given a name for either the other is a *variant* of it - which is a
/// candidate a search can test, where a bare hash is not.
fn print_kernel_exports(exports: &[orbistoun_probe::KernelExport], service: &Service) {
    let name_of = |export: &orbistoun_probe::KernelExport| -> Option<&str> {
        service.symbol_name(export.nid)
    };

    let named = exports.iter().filter(|e| name_of(e).is_some()).count();
    let addresses: std::collections::BTreeSet<u64> = exports.iter().map(|e| e.vaddr).collect();
    // The grade is the same for every entry in the table, so it is said once rather than
    // repeated on 2,443 lines - but it is said, because a table read off a stand-in and one
    // read off the target are the same bytes and different evidence (D246).
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

    // **The naming lead, and the only part of this worth a person's attention.** An alias
    // group with one side named says the unnamed side is a variant of a name we hold, which
    // is the difference between a hash to guess at and a hash to derive.
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
/// # Why this is the question, and not "how many are unnamed"
///
/// The table carries 2,443 hashes and this project can name 2,231 of them. **The 212 it cannot are
/// not the interesting set** - they are names orbistoun has no vocabulary for and no guest has
/// asked about. The interesting set is the intersection: a hash a *running guest* calls, that this
/// project cannot name, and that the console's own export table holds an address for.
///
/// Four such hashes exist in this corpus. One is `libkernel::0x04df812afad225d7`, which PPSA28061
/// calls and then `abort`s seventy-seven bytes later - a check-and-give-up whose only blocker is
/// the name (D636). A summary line saying "212 not named" cannot answer whether that hash is one
/// of them, which is the whole reason the table was asked for (D642).
///
/// Silent when the table answers none of them, because that is the ordinary case and a line that
/// fires every run is one people learn to skip.
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
        // **The address, because that is what makes it a lead.** Another hash at the same address
        // is an alias, and an alias with a named side names this one.
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

/// Hashes a guest here has actually called and nothing can name, from the persisted traces.
///
/// **The intersection is the point.** A kernel export table this project cannot fully name is
/// ordinary; a hash a *running guest* called, that nothing can name, and that the table holds an
/// address for, is a lead. Read from the same traces `worklist` ranks, so the two agree about
/// what "unnamed" means (D642).
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
/// # What "worst" means here
///
/// A check that **passed on the reference and failed here** is a defect with a sentence attached.
/// One that passed there and merely partially passed here is a lead. One that disagrees in any
/// other direction is usually a difference in what was reachable, and is listed last.
///
/// A check only one side ran is not listed at all: that is a difference in reach, not in
/// behaviour, and reporting it as a defect buries the ones that are (D622).
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
    // The count of the one class that is unambiguously a defect, **and how many distinct
    // things it is saying**. Ninety-nine of one sweep's hundred and fifteen were the same
    // sentence about a different absent library, and "115 defects, each with its own words"
    // was true of sixteen of them. A count of items is not a count of findings (D624).
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
/// **Ordered smallest group first**, inside each transition class in the order the classes
/// first appear. A finding one check made is the specific one; a finding ninety-nine made is a
/// census of what this build does not have, and printing it first buries the other sixteen -
/// which is the same reason a check only one side ran is not listed at all (D622, D624).
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
/// A group of one reads exactly as it always did. A larger one leads with its size, because
/// the size is the finding - "ninety-nine libraries are absent" is one fact about the build,
/// not ninety-nine facts about ninety-nine libraries.
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
/// # Why the state is shown and not just the value
///
/// All three states can read `unknown` and they are three different findings: the platform
/// has no such query, the probe has not wired one up yet, or here is a real number. A
/// display that collapsed them would show one blank where there are three, and only one of
/// them is anybody's bug.
///
/// Marked as the target's own account throughout. Inside an emulator every field answers as
/// that emulator chooses, so none of this is machine identity - that is asserted by the
/// operator and appears above, separately and labelled.
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
        // `generation` carries two readings a display can get wrong, so both are named
        // here rather than left in a document the reader does not have open.
        //
        // `both` is a positive observation - two driver stacks present - and it
        // deliberately names no console, because presence is not implementation.
        //
        // The parenthetical is **evidence, not recency**: `agc` and `gnm` are the graphics
        // drivers the inference keyed on. It used to read `(current)` / `(previous)`, which
        // stops being true the day a sixth generation ships and cannot be corrected in an
        // archived report (obSCEne D147). Nothing here parsed those words - the value is
        // rendered verbatim - so the change needed no code; the note is so a reader does
        // not take a driver name for a version.
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

/// Prints each area of the platform and how much of it came out green.
///
/// A single total says how much was checked and nothing about what is *understood*. The
/// same count spread thinly across every area and concentrated in one are completely
/// different situations, and only the second means a subsystem can be relied on.
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
        // A skip is shown rather than folded into the total, because it is a check that
        // did not run - the section did not establish what it claims to, and rounding a
        // skip up is how a subsystem gets relied on for something nobody tested.
        let counts = [
            ("pass", section.pass),
            ("partial", section.partial),
            ("fail", section.fail),
            ("skip", section.skip),
        ]
        .into_iter()
        // `pass` is shown even at zero: a section reporting no passes is the interesting
        // case, and omitting the number would leave it looking like a section with no
        // checks rather than one where nothing worked.
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

/// Prints what produced the answers, before any of the answers.
///
/// First and not as a footnote. A number read without knowing which machine produced it is
/// the failure this project has already paid for once.
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
        // What produced the answers, printed first and not as a footnote. A number read
        // without knowing which device produced it is the failure this project has already
        // paid for once.
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
        // What the session claimed, printed as a claim. It is worth seeing next to the
        // operator's assertion precisely when the two disagree - an emulator announcing
        // `console` under an operator who said otherwise is the case this whole
        // distinction exists for.
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
    // Grouped by library, because that is how the knowledge base is filed, and
    // rendered through its own serialiser so what is printed is what would be written.
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
