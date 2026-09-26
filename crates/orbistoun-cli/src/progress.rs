//! The report printed after a run: findings, standing, threads and the wall.

use crate::run::Run;
/// Says whether this run got further than the last.
///
/// **Presentation only.** The measurement itself is `orbistoun_report::trace::compare`,
/// one layer down, because the GUI has to reach the same verdict from the same two
/// traces. Two shims computing "did this help?" separately is how they come to disagree
/// about the only number this project steers by (D160).
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
    // **Before the verdict is read, not after.** A diagnostic that applied zero times
    // makes the run an ordinary one wearing a label, and the whole risk is that its
    // unchanged result is recorded as an elimination. Two were (D229, D230, D241).
    for what in &after.conditions.did_nothing {
        println!("           !! {what} - this run measured nothing it was asked to measure");
    }
    if progress.bought_under_intervention {
        // **Printed where the conclusion gets drawn.** This exists because the mistake was
        // made here: a reservation moved a wall, the movement read as confirming the
        // hypothesis behind it, and watching what the guest *wrote* one run later said the
        // opposite (D224, D226, D227).
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

/// What the guest was doing when it died, beyond where.
///
/// **The trace held all of this and the report printed a region and an offset.** Reading
/// the operation, the address and the registers meant parsing the trace by hand - which is
/// the tool failing at its one job, and is how a person ends up writing a throwaway script
/// to answer a question the run already knew (D197).
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
/// A forced dump is a question somebody asked deliberately - usually about an
/// implementation they wrote and suspect. Showing it only underneath a finding meant the
/// answer was recorded and never displayed, because an implemented function has no finding
/// (D197).
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

/// One captured argument, as a line.
///
/// **One renderer, because there were two.** The findings list and the unattached-dump
/// list each had their own copy of this three-way branch, and the third case below had to
/// be added to both or the same dump would read differently depending on which list it
/// appeared in (D217).
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
        // A scalar: a size, a flag, a count. Evidence in its own right (D198).
        return format!("      arg{} = {:#x}", d.slot, d.value);
    }
    // Address-shaped, and nothing readable there. Said out loud, because otherwise it
    // renders exactly like the line above and the two mean opposite things - one is a
    // number the guest chose, the other is a pointer that is wrong or a region this run
    // never declared.
    format!("      arg{} = {:#x} -> {}", d.slot, d.value, d.at)
}

/// What the run says is worth doing, most actionable first.
///
/// **The output this project is ultimately for.** Everything above is a measurement; this
/// is the conclusion drawn from it - and it is rendered from the same structured findings
/// that go into the trace, so a person and a machine are reading the same thing rather
/// than one parsing the other's prose (D179).
/// How many findings a run prints when nothing says otherwise.
///
/// Six is what a person reading a run wants; `ORBISTOUN_FINDINGS` is for working one.
const DEFAULT_FINDINGS: usize = 6;

/// How many findings to print, given whatever the setting holds.
///
/// **A value that is not a number is the default, not zero.** `take(0)` prints nothing while
/// still printing the heading, which reads as "this run found nothing" - the opposite of what
/// a mistyped setting should say. Zero *asked for* is honoured, because suppressing the list
/// deliberately is a thing somebody may want (D527).
fn findings_to_show(setting: Option<String>) -> usize {
    setting.map_or(DEFAULT_FINDINGS, |raw| {
        raw.trim().parse::<usize>().unwrap_or(DEFAULT_FINDINGS)
    })
}

fn print_findings(trace: &orbistoun_report::trace::CallTrace) {
    use orbistoun_report::diagnose::Confidence;

    let findings = orbistoun_report::diagnose::findings(trace);
    if findings.is_empty() {
        return;
    }
    println!();
    println!("what to do about it");
    // **Six by default, and the rest summarised as a count.** That is right for reading a run
    // and wrong for working one: a finding past the sixth keeps its arguments, and those
    // arguments are what name a call. This has now hidden the one that mattered twice - once
    // read as "three stubs left", once blocking a wall investigation outright (D527).
    // **What was asked for is never one of the six.** A `Gap::Captured` finding exists only
    // because somebody named an import with `ORBISTOUN_DUMP`, so ranking it against findings
    // the tool volunteered - and cutting it at six - answers a different question from the one
    // that was put (D625).
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
        // What the guest was pointing at, beneath the finding that names the function.
        // A finding says "implement this"; the dump says what it was handed, which is the
        // difference between knowing the job and being able to do it (D194).
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

/// Whether formatted writes actually produced anything.
///
/// **"Implemented" and "answered correctly" are different claims.** A refused format still
/// counts as a call reaching an implementation, so the standing figure rises while the
/// guest receives an empty string - which is exactly the sort of improvement-shaped
/// non-improvement this project keeps having to guard against (D183).
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
/// **The discount on the headline.** A call count reads as progress, and it is progress
/// exactly to the extent the calls were answered by an implementation rather than a
/// placeholder. Reporting the total alone lets the two be confused, and the confusion has
/// a direction: the cheapest way to raise a call count is to make every unimplemented
/// function claim success, at which point the guest runs much further on nothing at all.
///
/// Not a prohibition. Answering `ok` everywhere is a legitimate bisection technique and the
/// loop depends on being able to try it - what makes it a hack is doing it unlabelled, so
/// the label is what this prints (D181).
fn print_standing(trace: &orbistoun_report::trace::CallTrace) {
    if trace.total_calls == 0 {
        return;
    }
    let stubbed = trace.stubbed_calls();
    // **The count, not only the share.** Twelve stubbed calls out of four hundred and sixty-seven
    // thousand is `0%`, and `0% on stubs` reads as *nothing is stubbed* - while those twelve are
    // the only calls in the run worth looking at. The same shape as `0 KiB` for four hundred and
    // two bytes, found by auditing for it after that one cost four days (D595, D597).
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
/// **The line that separates stuck from slow**, and it did not exist. A run that hit the limit
/// printed `ran to the time limit` and nothing else, which reads as "it was still working" - and
/// for the title that gets furthest in this project it was the opposite: 310,987 calls, then
/// nothing, for seventeen of twenty seconds. Establishing that took two runs at different limits
/// and a hand comparison; this is the same fact from one run (D645).
///
/// Silent when nothing measured it - a fault, a self-stop, or a spent call budget all end a run
/// with the guest still going, and a `0.0s` there would be a measurement nobody made.
fn print_quiet(trace: &orbistoun_report::trace::CallTrace) {
    let Some(quiet) = trace.quiet else {
        return;
    };
    println!("  quiet    it {}", quiet.describe());
    if quiet.is_notable() {
        // **What was counted, then what it could mean, in that order.** No call crossing into
        // the host is the measurement; blocked is one reading of it and busy guest code is
        // another, and this branch cannot tell them apart - so it names both and says what
        // would (principle 3).
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
/// **Printed only when the guest went quiet**, because that is the question it answers. A run that
/// faulted has a fault site and a tail; a run that stopped calling has neither, and until this
/// existed the report could say only *that* it stopped. Reaching "an asset collector raised a
/// signal on main and then waited on a semaphore" took three rounds of hand instrumentation, and
/// every round re-derived what these two tables already held (D651).
///
/// # Grouped by name, and that is the whole of its usefulness
///
/// Unity spawns eleven identically-named helpers. Listed one per line they fill the report and
/// push `main` - the thread the signal was aimed at - off the end of it, which the first version
/// of this function did. Grouped, the same run says it in three lines: eleven helpers of which
/// one is still calling, and a `main` that is not.
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
    // **The busiest group last.** A reader scanning down ends on the thread that was still
    // working, which is where the story of a hang continues.
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
        // A thread the guest never named still needs a column: blank reads as a continuation of
        // the line above it, which is how the collector first looked like part of `main`.
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
/// **The tail, because a guest describes its problem immediately before it stops.** The first
/// lines of a boot log are the same every run and the last one is the finding, so a head would
/// show the part nobody needs and cut the part everybody does.
///
/// Printed verbatim and **not classified**. It would be easy to hunt the word "error" and put
/// those lines first; that is a heuristic dressed as a diagnosis, and this project's rule is that
/// a message naming a cause comes from the branch that determined it. Nothing here determined
/// anything - the guest did, and its own words are shown in its own order (D658).
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
        // Truncated per line rather than wrapped: a log line is one thing the guest said, and a
        // wrapped one reads as two.
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
/// Printed whenever anything was read, including when it is clean - a line that only
/// appears on failure cannot be told apart from one nobody wired up (D175).
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
/// **`0 KiB` is what four hundred and two bytes looked like**, and this line was read as *one
/// read of zero bytes* for four days - into a decision entry, into three worklogs, and into a
/// whole line of investigation about a guest that was in fact reading its boot configuration
/// completely and successfully.
///
/// Integer division by 1024 is not wrong, and it is the shape principle 3 warns about: output
/// that is true and reads as something else. So anything under a mebibyte says bytes (D595).
fn amount(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        return format!("{bytes} bytes");
    }
    format!("{} KiB", bytes / 1024)
}

/// Whether the guest called us the way the calling convention says it must.
///
/// Printed on every run, including when it is clean - a line that only appears on failure
/// cannot be distinguished from a line nobody wired up, and this measures something that
/// was silently untested for the whole life of the project (D159).
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
        // The remainder is the diagnosis: 0 means control arrived by a jump where a call
        // was expected, anything odd means the stack was already wrong upstream.
        println!(
            "           first at #{sequence} {import}, rsp {rsp:#x} (rsp % 16 = {})",
            rsp % 16
        );
    }
}

/// The last calls before the guest died, in order.
///
/// Only shown when there *was* a fault: for a run that ended cleanly the ranked list is
/// the right view, and this would be noise. At a wall it is the opposite - "what did it
/// call last" is the only question worth asking, and a list ranked by frequency cannot
/// answer it at any length (D154).
///
/// Consecutive repeats are collapsed. A guest clearing memory calls `memset` three
/// hundred times in a row, and printing them individually buries the two calls either
/// side that actually matter.
fn print_wall(trace: &orbistoun_report::trace::CallTrace) {
    if trace.fault.is_none() || trace.tail.is_empty() {
        return;
    }
    println!();
    // **Only the last calls if the recording reached the end**, and for most titles it does
    // not: the ring keeps the first `MAX_RECORDED_CALLS` and stops, so a run of four hundred
    // thousand calls leaves a "tail" sitting at call eight thousand. Presenting that as what
    // the guest called last is a report claiming more than its measurement supports, which is
    // the one thing principle 3 forbids the tools as well as the emulator (D568).
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

    // Keyed on the answer as well as the label, so a run of calls that returned *different*
    // values does not collapse into one line that hides the very thing worth seeing - a
    // function that answered a pointer once and zero the next time (D459).
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
        // A collapsed run shows its first call's first argument, and only that. When the calls
        // in it did not share one, say so: D566 read a run of forty-eight as "one address" off
        // exactly this line, and the addresses were thirteen (D573).
        let repeat = match (count, firsts.len()) {
            (1, _) => String::new(),
            (n, 1) => format!(" x{n}"),
            (n, distinct) => format!(" x{n} on {distinct} distinct first arguments"),
        };
        // The call site is shown beside the call because it is the same address space the
        // fault's frame walk reports - so a frame in the stack trace stops being a bare
        // number the moment an import was called from it (D173).
        let site = if from == 0 {
            String::new()
        } else {
            format!("   from {from:#x}")
        };
        // What it answered, when the call had returned before the trace was read. Shown
        // because at these walls the wrong value is usually the one we handed back, not the
        // one the guest passed in - and a tail without it could not point at that (D459).
        let answered = match returned {
            Some(ret) => format!(" -> {ret:#x}"),
            None => String::new(),
        };
        // The first argument is shown because at a wall it is often the whole answer: a
        // guest passing back an address it was handed a moment earlier makes the chain
        // visible with no other tooling.
        println!("  {label}({arg0:#x}){answered}{repeat}{site}");
    }
}

#[cfg(test)]
mod tests {
    /// **A mistyped setting prints the default, not nothing.**
    ///
    /// `take(0)` still prints the "what to do about it" heading and then no findings, which
    /// reads as "this run found nothing" - the opposite of what a typo should say. Zero asked
    /// for explicitly is honoured, because suppressing the list deliberately is reasonable.
    ///
    /// **What this cannot check:** that the number chosen is enough. It is a cap, and a cap is
    /// only ever right for the question being asked - which is why it is a setting now and not
    /// a constant (D527).
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
