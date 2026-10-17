//! The report printed after a run: findings, standing, threads and the wall.

use crate::run::Run;
/// Says whether this run got further than the last.
///
/// Presentation only: the verdict is `orbistoun_report::trace::compare`, so the GUI reaches the
/// same one from the same two traces (D034).
pub(crate) fn report_progress(
    before: Option<&orbistoun_report::trace::CallTrace>,
    after: &orbistoun_report::trace::CallTrace,
) {
    use orbistoun_report::trace::Verdict;

    let progress = orbistoun_report::trace::compare(before, after);
    println!();
    println!("progress");

    if progress.verdict == Verdict::FirstRun {
        println!("  {}", progress.verdict.summary());
        println!(
            "  reached {} distinct imports, died at {}",
            after.distinct, progress.fault
        );
        print_standing(after);
        print_leaned_on(after);
        print_decided(after);
        print_quiet(after);
        print_threads(after);
        print_waits(after);
        print_abi(after);
        print_link_plan(after);
        print_said(after);
        print_wall(after);
        return;
    }

    println!(
        "  imports  {} distinct ({:+}), {} calls ({:+})",
        after.distinct, progress.distinct_delta, after.total_calls, progress.calls_delta
    );
    for (sign, labels) in [("+", &progress.gained), ("-", &progress.lost)] {
        if !labels.is_empty() {
            println!("           {sign} {}", labels.join(", "));
        }
    }
    // Where the GPU queue stopped, against the run before (D849).
    let stop = |at: Option<orbistoun_report::trace::GpuStop>| {
        at.map_or_else(
            || "nowhere".to_owned(),
            |at| format!("submission {}, byte {:#x}", at.submission, at.byte),
        )
    };
    if progress.gpu_stop.is_some() || progress.previous_gpu_stop.is_some() {
        println!(
            "  gpu      stopped at {} (was {})",
            stop(progress.gpu_stop),
            stop(progress.previous_gpu_stop)
        );
    }
    print_standing(after);
    print_leaned_on(after);
    print_decided(after);
    print_quiet(after);
    print_threads(after);
    print_waits(after);
    println!(
        "  fault {}{}",
        progress.fault,
        match &progress.previous_fault {
            Some(previous) => format!("   (was {previous})"),
            None => String::new(),
        }
    );
    print_reads(after);
    println!(
        "  verdict  {:8} {}",
        progress.verdict.label(),
        progress.verdict.summary()
    );
    for change in &progress.conditions_changed {
        println!("           ! {change}, so this verdict measures a settings change");
    }
    // Before the verdict is read: a diagnostic that applied zero times leaves an ordinary run whose
    // unchanged result could be misread as an elimination (D227).
    for what in &after.conditions.did_nothing {
        println!("           !! {what} - this run measured nothing it was asked to measure");
    }
    if progress.bought_under_intervention {
        // Printed where the conclusion is drawn: under an intervention, getting further may mean
        // the guest accepted a wrong answer (D227).
        println!("           ! this run altered the program, so getting further may mean the");
        println!("             guest accepted a wrong answer. Check what it *wrote*, not only");
        println!("             that it moved - ORBISTOUN_WATCH is what answers that");
    }
    print_abi(after);
    print_link_plan(after);
    print_formats(after);
    print_fault_detail(after);
    print_unattached_dumps(after);
    print_said(after);
    print_findings(after);
    print_wall(after);
}

/// The link plan the run applied and how it stood against the stored one (D724).
fn print_link_plan(trace: &orbistoun_report::trace::CallTrace) {
    if let Some(line) = trace.conditions.describe_link_plan() {
        println!("  {line}");
    }
}

/// What the guest was doing when it faulted: the operation, the address and the registers the trace
/// holds.
fn print_fault_detail(trace: &orbistoun_report::trace::CallTrace) {
    let Some(f) = &trace.fault else {
        return;
    };
    println!("  faulted  {} {:#x}", f.kind, f.address);
    if let Some(r) = &f.registers {
        for line in r.lines() {
            println!("           {line}");
        }
    }
    if !f.frames.is_empty() {
        let path: Vec<String> = f
            .frames
            .iter()
            .take(4)
            .map(|x| format!("{:#x}", x.return_address))
            .collect();
        println!("           called from {}", path.join(" <- "));
    }
}

/// Arguments captured for functions no finding mentions.
///
/// A forced dump is a deliberate question, often about an implemented function, which has no
/// finding to show it under.
fn print_unattached_dumps(trace: &orbistoun_report::trace::CallTrace) {
    let findings = orbistoun_report::diagnose::findings(trace);
    let shown: Vec<&str> = findings.iter().map(|f| f.what.as_str()).collect();
    let loose: Vec<&orbistoun_report::trace::ArgumentDump> = trace
        .dumps
        .iter()
        .filter(|d| !shown.iter().any(|w| w.contains(&d.label)))
        .collect();
    if loose.is_empty() {
        return;
    }
    println!();
    println!("captured arguments");
    let mut last = "";
    for d in loose {
        if d.label != last {
            println!("  {}", d.label);
            last = &d.label;
        }
        println!("{}", dump_line(d));
    }
}

/// One captured argument, as a line, shared by the findings list and the unattached-dump list so a
/// dump reads the same in both.
fn dump_line(d: &orbistoun_report::trace::ArgumentDump) -> String {
    if !d.bytes.is_empty() {
        let text = if d.text.is_empty() {
            String::new()
        } else {
            format!("  \"{}\"", d.text)
        };
        return format!(
            "      arg{} = {:#x} -> {} = {}{}{}",
            d.slot,
            d.value,
            d.at,
            d.bytes,
            text,
            d.moment()
        );
    }
    if d.at.is_empty() {
        // A scalar: a size, a flag, a count.
        return format!("      arg{} = {:#x}{}", d.slot, d.value, d.moment());
    }
    // Address-shaped with nothing readable there. Said explicitly, because otherwise it renders
    // like a scalar: a pointer that is wrong, or into a region this run never declared.
    format!(
        "      arg{} = {:#x} -> {}{}",
        d.slot,
        d.value,
        d.at,
        d.moment()
    )
}

/// How many findings a run prints when nothing says otherwise.
///
/// Six suits reading a run; `ORBISTOUN_FINDINGS` is for working one.
const DEFAULT_FINDINGS: usize = 6;

/// How many findings to print, given whatever the setting holds.
///
/// A value that is not a number gives the default, not zero: `take(0)` prints the heading and
/// nothing under it, which reads as "found nothing". An explicit zero is honoured.
fn findings_to_show(setting: Option<String>) -> usize {
    setting.map_or(DEFAULT_FINDINGS, |raw| {
        raw.trim().parse::<usize>().unwrap_or(DEFAULT_FINDINGS)
    })
}

/// What the run says is worth doing, most actionable first.
///
/// Rendered from the same structured findings the trace holds, so a person and a machine read the
/// same thing (D179).
fn print_findings(trace: &orbistoun_report::trace::CallTrace) {
    use orbistoun_report::diagnose::{Confidence, Gap};

    let findings = orbistoun_report::diagnose::findings(trace);
    if findings.is_empty() {
        return;
    }
    println!();
    println!("what to do about it");
    // Findings asked for with `ORBISTOUN_DUMP` (`Gap::Captured`) are always shown and never count
    // against the cap, which applies only to findings the tool volunteered. Host gaps are always
    // shown too, in a block of their own after the walls: they rank below every wall, and a cap
    // would hide the one place a silent title or an empty window is named.
    let (asked, rest): (Vec<_>, Vec<_>) = findings.iter().partition(|f| f.gap == Gap::Captured);
    let (host, volunteered): (Vec<_>, Vec<_>) =
        rest.into_iter().partition(|f| f.gap == Gap::HostCapability);
    let shown = findings_to_show(orbistoun_env::FINDINGS.get());
    let print = |finding: &orbistoun_report::diagnose::Finding| {
        let mark = match finding.confidence {
            Confidence::Certain => "!",
            Confidence::Likely => "?",
            Confidence::Possible => "~",
        };
        println!("  {mark} {}", finding.what);
        for line in &finding.evidence {
            println!("      {line}");
        }
        // What the guest was pointing at, beneath the finding that names the function (D194).
        for dump in trace
            .dumps
            .iter()
            .filter(|d| finding.what.contains(&d.label))
        {
            println!("{}", dump_line(dump));
        }
        if let Some(action) = &finding.action {
            println!("    -> {action}");
        }
    };
    for finding in asked
        .iter()
        .copied()
        .chain(volunteered.iter().copied().take(shown))
    {
        print(finding);
    }
    if volunteered.len() > shown {
        println!("  ... and {} more", volunteered.len() - shown);
    }
    if !host.is_empty() {
        println!();
        println!("on the host");
        for finding in host {
            print(finding);
        }
    }
}

/// Whether formatted writes produced anything.
///
/// A refused format still counts as a call reaching an implementation, so standing rises while the
/// guest receives an empty string (D183).
fn print_formats(trace: &orbistoun_report::trace::CallTrace) {
    let f = &trace.formats;
    if f.calls == 0 {
        return;
    }
    if f.refused == 0 && f.truncated == 0 {
        println!("  formats  {} writes, all honoured", f.calls);
        return;
    }
    println!(
        "  formats  {} writes, {} refused, {} truncated",
        f.calls, f.refused, f.truncated
    );
    if !f.first_fault.is_empty() {
        println!("           ! {}", f.first_fault);
    }
}

/// How much of the run rested on something real.
///
/// A call count is progress only to the extent calls were answered by an implementation rather than
/// a placeholder. Answering `ok` everywhere is a legitimate bisection technique; this prints the
/// label that keeps it from being mistaken for progress (D181).
fn print_standing(trace: &orbistoun_report::trace::CallTrace) {
    if trace.total_calls == 0 {
        return;
    }
    let stubbed = trace.stubbed_calls();
    // The count as well as the share: a handful of stubbed calls in hundreds of thousands rounds to
    // `0%`, which reads as nothing stubbed.
    println!(
        "  standing {} of {} calls answered by an implementation ({} on stubs, {}%)",
        trace.total_calls - stubbed,
        trace.total_calls,
        stubbed,
        trace.stubbed_share()
    );
    if trace.conditions.answers_blindly() {
        println!(
            concat!(
                "           ! stubs are answering {} rather than reporting unimplemented, ",
                "so reaching further this run means less, not more"
            ),
            trace.conditions.default_return
        );
    }
}

/// The implemented calls in `calls` whose answer `partial` says is assumed or partly modelled, by
/// label, with their call counts and why, most-called first.
fn leaned_on(
    calls: &[orbistoun_report::trace::CalledImport],
    partial: impl Fn(&str) -> Option<String>,
) -> Vec<(String, u64, String)> {
    let mut leaned: Vec<(String, u64, String)> = calls
        .iter()
        .filter(|row| row.implemented)
        .filter_map(|row| {
            let name = row.label.rsplit("::").next().unwrap_or(&row.label);
            partial(name).map(|why| (row.label.clone(), row.calls, why))
        })
        .collect();
    leaned.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    leaned
}

/// The most partial answers [`print_leaned_on`] lists.
const LEANED_ON_SHOWN: usize = 8;

/// Names the implemented answers this run leaned on most that its knowledge records as assumed or
/// partly modelled: the standing line counts them as answered, and a title whose picture is wrong
/// is often reading one of them.
fn print_leaned_on(trace: &orbistoun_report::trace::CallTrace) {
    let knowledge = orbistoun_hle::knowledge::Knowledge::builtin();
    let leaned = leaned_on(&trace.calls, |name| {
        knowledge
            .get(name)
            .and_then(orbistoun_hle::knowledge::FunctionKnowledge::partial)
    });
    if leaned.is_empty() {
        return;
    }
    println!(
        "  leans on {} answer(s) recorded as assumed or partly modelled, most-called first:",
        leaned.len()
    );
    for (label, calls, why) in leaned.iter().take(LEANED_ON_SHOWN) {
        println!("           {calls:>9} calls  {label} - {why}");
    }
}

/// One line per placeholder answer the title branches on: the import, where it was first called,
/// and the test and jump there.
fn decided_lines(tested: &[orbistoun_report::trace::TestedAnswer]) -> Vec<String> {
    tested
        .iter()
        .map(|t| format!("{} at {}: {}", t.label, t.site, t.test))
        .collect()
}

/// Names every placeholder answer the title's own code branches on, however rarely it was called:
/// the findings rank stubs by calls, and a one-call stub that sends a start-up down its failure path
/// would otherwise go unprinted.
fn print_decided(trace: &orbistoun_report::trace::CallTrace) {
    let lines = decided_lines(&trace.tested_answers);
    if lines.is_empty() {
        return;
    }
    println!(
        "  decides  {} placeholder answer(s) the title branches on where it first called them:",
        lines.len()
    );
    for line in &lines {
        println!("           {line}");
    }
}

/// When the guest last asked the host for anything, for a run the clock ended.
///
/// Separates stuck from slow: "ran to the time limit" alone reads as still working, when the guest
/// may have gone quiet long before. Silent when nothing measured it - a fault, a self-stop or a
/// spent call budget.
fn print_quiet(trace: &orbistoun_report::trace::CallTrace) {
    let Some(quiet) = trace.quiet else {
        return;
    };
    println!("  quiet    it {}", quiet.describe());
    if quiet.is_notable() {
        // What was counted, then what it could mean: no call crossing into the host is the
        // measurement; blocked and busy in guest code are both readings, and this branch cannot
        // tell them apart.
        println!(concat!(
            "           ! it stopped asking the host for anything well before the clock ran ",
            "out - either it is waiting on something that has not arrived, or it is working ",
            "inside its own code"
        ));
        println!(
            "           try ORBISTOUN_LIMIT={} - an identical call count there means it is blocked",
            (quiet.run_ms / 1000).saturating_mul(4).max(2)
        );
    }
}

/// Which guest threads exist, and what each last did.
///
/// Printed only when the guest went quiet: a faulted run has a fault site and a tail, a quiet one
/// has neither. Grouped by name, because engines spawn many identically named helpers that would
/// otherwise push `main` off the report.
fn print_threads(trace: &orbistoun_report::trace::CallTrace) {
    if trace.quiet.is_none() || trace.threads.is_empty() {
        return;
    }
    let mut groups: Vec<(&str, Vec<&orbistoun_report::trace::ThreadNote>)> = Vec::new();
    for note in &trace.threads {
        if let Some(group) = groups.iter_mut().find(|(name, _)| *name == note.name) {
            group.1.push(note);
        } else {
            groups.push((note.name.as_str(), vec![note]));
        }
    }
    // The busiest group last, so a reader ends on the thread still working.
    groups.sort_by_key(|(_, notes)| notes.iter().filter_map(|n| n.last_sequence).max());
    let silent = trace
        .threads
        .iter()
        .filter(|t| t.last_call.is_none())
        .count();
    println!(
        "  threads  {} guest thread(s), {silent} with no call in the recorded window",
        trace.threads.len()
    );
    for (name, notes) in &groups {
        // An unnamed thread still gets a label: a blank reads as a continuation of the line above.
        let name = if name.is_empty() { "(unnamed)" } else { *name };
        let newest = notes
            .iter()
            .filter(|n| n.last_call.is_some())
            .max_by_key(|n| n.last_sequence);
        let ended = notes.iter().filter(|n| n.finished).count();
        let count = if notes.len() == 1 {
            format!("{:#x}", notes[0].handle)
        } else {
            format!("x{}", notes.len())
        };
        let blocked = notes.iter().find_map(|n| n.inside.as_deref());
        let tail = match (newest, blocked) {
            (Some(note), _) => format!(
                "last called {} at {}",
                note.last_call.as_deref().unwrap_or("?"),
                note.last_sequence.unwrap_or(0)
            ),
            (None, Some(call)) => format!(
                "inside {call}, from before the last {} calls of the run",
                orbistoun_report::trace::TAIL_CALLS
            ),
            (None, None) => format!(
                "no call in the last {} of the run",
                orbistoun_report::trace::TAIL_CALLS
            ),
        };
        let ended = if ended == 0 {
            String::new()
        } else {
            format!(", {ended} ended")
        };
        println!("           {name:<30} {count:<16} {tail}{ended}");
    }
}

/// Each thread waiting in a loop around one call, with what its callee-saved registers held and
/// pointed at: where the word it waits on is (D832).
///
/// Printed however the run ended, since a thread spinning on a yield keeps a run busy and never
/// quiet.
fn print_waits(trace: &orbistoun_report::trace::CallTrace) {
    for note in &trace.threads {
        let Some(wait) = &note.waits else {
            continue;
        };
        println!(
            "  waits    {} loops on {} ({} of its recorded calls)",
            note.name,
            note.last_call.as_deref().unwrap_or("?"),
            wait.calls
        );
        for register in &wait.registers {
            let bytes: Vec<String> = register.bytes.iter().map(|b| format!("{b:02x}")).collect();
            let pointee = if bytes.is_empty() {
                String::new()
            } else {
                format!(" -> {}", bytes.join(" "))
            };
            println!(
                "           {:<3} = {:#018x}{pointee}",
                register.name, register.value
            );
        }
        if let Some(word) = &wait.word {
            let from = word.held_write.as_ref().map_or_else(
                || format!("{}+{:#x}", word.register, word.offset),
                |held| format!("the GPU's write: {held}"),
            );
            println!(
                "           reads {:#x} ({from}), the access before {:#x}",
                word.address, word.read_after
            );
            if word.writers.is_empty() {
                println!(
                    "           ! nothing wrote it while it was watched: a completion that never came"
                );
            } else {
                let writers: Vec<String> = word.writers.iter().map(|w| format!("{w:#x}")).collect();
                println!("           written before {}", writers.join(", "));
            }
            if word.handed_to.is_empty() {
                println!("           no call whose arguments were dumped was handed its object");
            }
            for call in &word.handed_to {
                println!("           handed to {call}");
            }
        }
    }
}

/// What the guest said, ending with the last thing it said.
///
/// The tail, because a guest describes its problem just before it stops and the first lines of a
/// boot log are the same every run. Printed verbatim and not classified: nothing here determined a
/// cause, so nothing ranks the lines.
fn print_said(trace: &orbistoun_report::trace::CallTrace) {
    if trace.said.is_empty() {
        return;
    }
    let shown = trace.said.len().min(SAID_LINES);
    println!(
        "\nwhat the guest said  ({} line(s), last {shown})",
        trace.said.len()
    );
    for line in trace.said.iter().skip(trace.said.len() - shown) {
        // Truncated per line rather than wrapped, so one line reads as one message.
        let trimmed: String = line.chars().take(SAID_WIDTH).collect();
        let cut = if line.chars().count() > SAID_WIDTH {
            " …"
        } else {
            ""
        };
        println!("  {trimmed}{cut}");
    }
    // The lines it marked as errors itself, wherever in what is kept they were said.
    let errors = guest_errors(&trace.said);
    if !errors.is_empty() {
        println!(
            "\nwhat the guest called an error  ({} distinct line(s))",
            errors.len()
        );
        for (line, times) in errors.iter().take(SAID_ERRORS) {
            let trimmed: String = line.chars().take(SAID_WIDTH).collect();
            println!("  {times}x {trimmed}");
        }
    }
}

/// How many of the lines a guest called errors a report lists.
const SAID_ERRORS: usize = 8;

/// The lines of `said` the guest marked as errors itself - the word "error", in any case - each
/// once with how many times it was said, in the order first said. Selected by the guest's own word,
/// not ranked: nothing here says which caused what.
fn guest_errors(said: &[String]) -> Vec<(String, usize)> {
    let mut errors: Vec<(String, usize)> = Vec::new();
    for line in said
        .iter()
        .filter(|line| line.to_ascii_lowercase().contains("error"))
    {
        match errors.iter_mut().find(|(seen, _)| seen == line) {
            Some((_, times)) => *times += 1,
            None => errors.push((line.clone(), 1)),
        }
    }
    errors
}

/// How many of the guest's own lines a report ends with.
const SAID_LINES: usize = 24;

/// How much of one line is shown before it is cut.
const SAID_WIDTH: usize = 160;
/// Whether the guest received every byte it asked for.
///
/// Printed whenever anything was read, clean or not, so a missing line means nothing was read.
fn print_reads(trace: &orbistoun_report::trace::CallTrace) {
    let reads = &trace.reads;
    if reads.reads == 0 {
        return;
    }
    if reads.short == 0 {
        println!(
            "  files {} reads, {}, none cut short",
            reads.reads,
            amount(reads.bytes)
        );
    } else {
        println!(
            "  files {} of {} reads were CUT SHORT ({} delivered)",
            reads.short,
            reads.reads,
            amount(reads.bytes)
        );
    }
}

/// How many bytes, in a unit that cannot round a real number down to nothing.
///
/// Integer division by 1024 prints `0 KiB` for hundreds of bytes, which reads as an empty read, so
/// anything under a mebibyte is in bytes.
fn amount(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        return format!("{bytes} bytes");
    }
    format!("{} KiB", bytes / 1024)
}

/// Whether the guest called us the way the calling convention requires.
///
/// Printed on every run, including when clean, so a missing line is never mistaken for a pass
/// (D159).
fn print_abi(trace: &orbistoun_report::trace::CallTrace) {
    let abi = &trace.abi;
    if abi.misaligned_calls == 0 {
        println!(
            "  abi {} calls, all on a conforming stack",
            trace.total_calls
        );
        return;
    }
    println!(
        "  abi {} of {} calls arrived on a MISALIGNED stack",
        abi.misaligned_calls, trace.total_calls
    );
    if let (Some(sequence), Some(rsp)) = (abi.first_misaligned_sequence, abi.first_misaligned_rsp) {
        let import = abi.first_misaligned_import.as_deref().unwrap_or("unknown");
        // The remainder is the diagnosis: 0 means control arrived by a jump where a call was
        // expected; anything odd means the stack was already wrong upstream.
        println!(
            "           first at #{sequence} {import}, rsp {rsp:#x} (rsp % 16 = {})",
            rsp % 16
        );
    }
}

/// The last calls before the guest faulted, in order.
///
/// Only shown for a fault: at a wall "what did it call last" is the question, and a frequency
/// ranking cannot answer it. Consecutive repeats are collapsed so a long `memset` loop does not
/// bury the calls around it.
fn print_wall(trace: &orbistoun_report::trace::CallTrace) {
    if trace.fault.is_none() || trace.tail.is_empty() {
        return;
    }
    println!();
    // The tail is labelled as the last calls only when it reaches the run's final call; otherwise
    // its sequence range is printed so it is not read as what preceded the fault.
    let reaches_end = trace
        .tail
        .last()
        .is_some_and(|c| c.sequence + 1 >= trace.total_calls);
    if reaches_end {
        println!("last calls before the fault");
    } else {
        let first = trace.tail.first().map_or(0, |c| c.sequence);
        let last = trace.tail.last().map_or(0, |c| c.sequence);
        println!(
            "calls #{first}-#{last} of {} - NOT the ones before the fault: the recorder fills once and stops, and this run made more",
            trace.total_calls
        );
    }

    // Keyed on the answer as well as the label, so calls that returned different values stay on
    // separate lines (D459).
    let mut runs: Vec<Run<'_>> = Vec::new();
    for call in &trace.tail {
        match runs.last_mut() {
            Some(run) if run.label == call.label && run.returned == call.returned => {
                run.count += 1;
                run.firsts.insert(call.args[0]);
            }
            _ => runs.push(Run {
                label: &call.label,
                arg0: call.args[0],
                returned: call.returned,
                from: call.from,
                count: 1,
                firsts: std::iter::once(call.args[0]).collect(),
            }),
        }
    }
    for Run {
        label,
        arg0,
        returned,
        from,
        count,
        firsts,
    } in runs
    {
        // A collapsed run shows its first call's first argument; when the calls did not share one,
        // the line says so.
        let repeat = match (count, firsts.len()) {
            (1, _) => String::new(),
            (n, 1) => format!(" x{n}"),
            (n, distinct) => format!(" x{n} on {distinct} distinct first arguments"),
        };
        // The call site shares the address space of the fault's frame walk, so a stack frame can be
        // matched to the import called from it (D018).
        let site = if from == 0 {
            String::new()
        } else {
            format!("   from {from:#x}")
        };
        // What it answered, when the call returned before the trace was read: at a wall the wrong
        // value is often the one handed back (D459).
        let answered = match returned {
            Some(ret) => format!(" -> {ret:#x}"),
            None => String::new(),
        };
        // The first argument is often the whole answer at a wall: a guest passing back an address
        // it was just handed makes the chain visible.
        println!("  {label}({arg0:#x}){answered}{repeat}{site}");
    }
}

#[cfg(test)]
mod tests {
    /// Every placeholder answer the title branches on is listed whatever its call count: GTA V's
    /// online start-up dies on two one-call stubs that the call-ranked findings never printed.
    #[test]
    fn every_placeholder_the_title_branches_on_is_listed() {
        let tested = |label: &str, site: &str| orbistoun_report::trace::TestedAnswer {
            label: label.to_owned(),
            site: site.to_owned(),
            test: "test eax,eax; js 0000400002D4EB5Fh".to_owned(),
        };
        let lines = super::decided_lines(&[
            tested(
                "libSceNpEntitlementAccess::sceNpEntitlementAccessInitialize",
                "image+0x2d4e906",
            ),
            tested(
                "libSceNpManager::sceNpRegisterStateCallbackA",
                "image+0x2606636",
            ),
        ]);
        assert_eq!(
            lines,
            [
                "libSceNpEntitlementAccess::sceNpEntitlementAccessInitialize at image+0x2d4e906: test eax,eax; js 0000400002D4EB5Fh",
                "libSceNpManager::sceNpRegisterStateCallbackA at image+0x2606636: test eax,eax; js 0000400002D4EB5Fh",
            ]
        );
        assert!(super::decided_lines(&[]).is_empty());
    }

    /// The implemented calls a run leans on whose answer is assumed or partly modelled are named,
    /// most-called first: GTA V called an interpolant builder 45584 times whose remap by the
    /// shaders' semantics is "not modelled", and the standing line counted it as answered.
    #[test]
    fn a_run_names_the_partial_answers_it_leans_on() {
        let row =
            |label: &str, calls: u64, implemented: bool| orbistoun_report::trace::CalledImport {
                index: 0,
                label: label.to_owned(),
                calls,
                implemented,
                shape: String::new(),
            };
        let calls = vec![
            row("libSceAgc::0x71040c4df8235e1d", 45_584, true),
            row("libc::memcpy", 3_000_000, true),
            row("libSceX::sceAssumed", 12, true),
            row("libSceX::sceStub", 99, false),
        ];
        let partial = |name: &str| match name {
            "0x71040c4df8235e1d" => {
                Some("not modelled: a remap by the shaders' interpolants".to_owned())
            }
            "sceAssumed" | "sceStub" => Some("assumed".to_owned()),
            _ => None,
        };
        assert_eq!(
            super::leaned_on(&calls, partial),
            vec![
                (
                    "libSceAgc::0x71040c4df8235e1d".to_owned(),
                    45_584,
                    "not modelled: a remap by the shaders' interpolants".to_owned()
                ),
                ("libSceX::sceAssumed".to_owned(), 12, "assumed".to_owned()),
            ]
        );
    }

    /// The lines a guest marked as errors itself are listed apart, each once with how often it was
    /// said, in the order first said: a guest's own error buried mid-log is not lost to the tail.
    #[test]
    fn the_guest_s_own_errors_are_listed_once_each() {
        let said: Vec<String> = [
            "boot",
            "[Video] ERROR: PostInit failed",
            "frame",
            "[Video] ERROR: PostInit failed",
            "load error: missing.bin",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        assert_eq!(
            super::guest_errors(&said),
            vec![
                ("[Video] ERROR: PostInit failed".to_owned(), 2),
                ("load error: missing.bin".to_owned(), 1),
            ]
        );
        assert!(super::guest_errors(&said[..1]).is_empty());
    }

    /// A mistyped setting prints the default, not nothing; an explicit zero is honoured.
    #[test]
    fn a_mistyped_findings_setting_falls_back_rather_than_printing_none() {
        use super::{DEFAULT_FINDINGS, findings_to_show};
        assert_eq!(
            findings_to_show(None),
            DEFAULT_FINDINGS,
            "unset is the default"
        );
        assert_eq!(findings_to_show(Some("40".to_owned())), 40);
        assert_eq!(findings_to_show(Some(" 40 ".to_owned())), 40, "trimmed");
        assert_eq!(
            findings_to_show(Some("lots".to_owned())),
            DEFAULT_FINDINGS,
            concat!(
                "a typo must not silence the list - an empty list under a heading reads ",
                "as a run that found nothing"
            )
        );
        assert_eq!(
            findings_to_show(Some("0".to_owned())),
            0,
            "but zero asked for is honoured"
        );
    }
}
