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
        print_quiet(after);
        print_abi(after);
        print_said(after);
        print_wall(after);
        return;
    }

    println!(
        "  imports  {} distinct ({:+}), {} calls ({:+})",
        after.distinct, progress.distinct_delta, after.total_calls, progress.calls_delta
    );
    print_standing(after);
    print_quiet(after);
    print_threads(after);
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
    print_formats(after);
    print_fault_detail(after);
    print_unattached_dumps(after);
    print_said(after);
    print_findings(after);
    print_wall(after);
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
            "      arg{} = {:#x} -> {} = {}{}",
            d.slot, d.value, d.at, d.bytes, text
        );
    }
    if d.at.is_empty() {
        // A scalar: a size, a flag, a count.
        return format!("      arg{} = {:#x}", d.slot, d.value);
    }
    // Address-shaped with nothing readable there. Said explicitly, because otherwise it renders
    // like a scalar: a pointer that is wrong, or into a region this run never declared.
    format!("      arg{} = {:#x} -> {}", d.slot, d.value, d.at)
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
    use orbistoun_report::diagnose::Confidence;

    let findings = orbistoun_report::diagnose::findings(trace);
    if findings.is_empty() {
        return;
    }
    println!();
    println!("what to do about it");
    // Findings asked for with `ORBISTOUN_DUMP` (`Gap::Captured`) are always shown and never count
    // against the cap, which applies only to findings the tool volunteered.
    let (asked, volunteered): (Vec<_>, Vec<_>) = findings
        .iter()
        .partition(|f| f.gap == orbistoun_report::diagnose::Gap::Captured);
    let shown = findings_to_show(orbistoun_env::FINDINGS.get());
    for finding in asked
        .iter()
        .copied()
        .chain(volunteered.iter().copied().take(shown))
    {
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
    }
    if volunteered.len() > shown {
        println!("  ... and {} more", volunteered.len() - shown);
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
        let tail = match newest {
            Some(note) => format!(
                "last called {} at {}",
                note.last_call.as_deref().unwrap_or("?"),
                note.last_sequence.unwrap_or(0)
            ),
            None => format!(
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
