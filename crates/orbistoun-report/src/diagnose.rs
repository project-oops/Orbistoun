//! Turning a run into a ranked list of things to do about it.
//!
//! # Why findings rather than output
//!
//! Everything below is already visible somewhere in a run's output - the ranked import
//! list, the fault, the call tail, the stack conformance line. Reading it takes a person
//! who knows what each shape means, and *that person is the bottleneck*.
//!
//! The eventual consumer of this is not a person. It is something that reads a run and
//! proposes a change - today that is a human with a language model, and later it may be
//! the emulator repairing its own gaps. Either way it needs the same thing: **what is
//! wrong, where, what evidence says so, and what would address it** - as data, ranked, so
//! nothing has to be re-derived from prose.
//!
//! # Confidence is the load-bearing field
//!
//! A confidently wrong suggestion is **worse than no suggestion**, because it gets acted
//! on. That is not a general worry - it is this project's own history: an entry convention
//! that looked right, a stub policy that looked wired, a name sweep whose vocabulary could
//! not contain the answer. Each of those would have produced a confident, wrong finding.
//!
//! So every finding says how much weight it deserves, and the rule is the one obSCEne
//! already uses: a certain finding is a defect, a possible one is a conversation. Nothing
//! here reports `Certain` unless the trace *shows* it rather than suggests it (D179).

use crate::trace::{CallTrace, TracedCall};

/// How much a finding should be trusted.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// The trace shows it directly. Acting on this needs no judgement.
    Certain,
    /// A strong pattern, but one that has a benign reading. Worth checking first.
    Likely,
    /// A guess worth someone's attention, and nothing more.
    Possible,
}

/// What kind of gap a finding describes.
///
/// The kind is what lets a consumer route a finding without parsing prose - implement a
/// function, name a hash, and fix a contract are three different jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Gap {
    /// A function the guest called that has no implementation.
    Unimplemented,
    /// An import whose name is still a bare hash.
    Unnamed,
    /// A placeholder error code being used by the guest as a pointer or handle.
    ///
    /// The single most productive signal this project has: it names the function that
    /// answered wrongly *and* proves the guest believed the answer.
    ErrorUsedAsPointer,
    /// The guest gave up deliberately.
    GuestGaveUp,
    /// The guest died touching an address, and the address says which kind of mistake.
    ///
    /// **The commonest outcome in this project produced no finding at all.** A run that
    /// faulted printed a region and an offset and stopped, so the calls leading in, the
    /// registers and the arguments all had to be read out of the trace by hand - which is
    /// the tool asking a person to do its job (D198).
    Faulted,
    /// One call dominating the run, which means the guest is not progressing.
    Spinning,
    /// Guest calls arriving on a stack the calling convention forbids.
    AbiViolation,
    /// A file read that delivered less than was asked for.
    ShortRead,
}

impl Gap {
    /// Which crate or file the work most likely lands in.
    ///
    /// A hint rather than a rule: it saves a consumer a search, and being wrong costs one.
    pub const fn where_to_look(self) -> &'static str {
        match self {
            Self::Unimplemented | Self::ErrorUsedAsPointer => {
                "the subsystem crate that declares the symbol"
            }
            Self::Unnamed => "crates/orbistoun-names/data/vendor.toml",
            Self::GuestGaveUp | Self::Spinning | Self::Faulted => "the calls immediately before it",
            Self::AbiViolation => "crates/orbistoun-thunk, and how the guest is entered",
            Self::ShortRead => "crates/orbistoun-fs",
        }
    }
}

/// How the call leading into a wall is marked in [`Finding::evidence`].
///
/// Declared here rather than matched on by eye downstream: the subject of a fault is the
/// *region the guest died in*, so anything wanting the call that led there has to read it
/// out of the evidence, and a second copy of this prefix elsewhere is one that drifts.
///
/// Found the hard way. A dispatcher took the subject as the call and swept `image`, which
/// planted nothing across every argument and would have read as a clean negative.
pub const PRECEDED_BY: &str = "just before: ";

/// One thing worth doing about a run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    /// What kind of gap this is.
    pub gap: Gap,
    /// How much to trust it.
    pub confidence: Confidence,
    /// The symbol or address this is about, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// One sentence stating the problem.
    pub what: String,
    /// Why the run says so. **Facts from the trace, never inference.**
    pub evidence: Vec<String>,
    /// What would address it, if that is knowable from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// How many calls this concerns, used for ranking.
    pub weight: u64,
}

/// Placeholder codes this project answers with.
///
/// Deliberately in a range no real firmware value occupies (principle 3), which is exactly
/// what makes them findable in a guest's arguments afterwards.
/// What each integer argument arrives in, so a finding can say **which** one carried the value.
///
/// Saying "its first argument" when the value was in `rdx` sends a reader to the wrong place, and
/// a finding that misdirects is worse than one that says less (D570).
const ARGUMENT_REGISTERS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];

const PLACEHOLDER_LOW: u64 = 0x7FFF_0000;
/// One past the placeholder range.
const PLACEHOLDER_HIGH: u64 = 0x7FFF_0010;

/// One past every placeholder, tagged ones included.
///
/// `ORBISTOUN_TAG_PLACEHOLDERS` gives each stub `0x7fff_0000 | (0x10 + its slot)`, so a tagged
/// value sits above [`PLACEHOLDER_HIGH`] and still inside the half-word this project reserves.
const PLACEHOLDER_TAGGED_HIGH: u64 = 0x8000_0000;

/// Which stub produced a tagged placeholder, if this value is one.
///
/// [`None`] for an untagged placeholder - the ordinary `0x7fff_0001` says only that *some*
/// unimplemented function answered, which is the whole reason tagging exists (D567).
fn tagged_stub(value: u64) -> Option<usize> {
    if !(PLACEHOLDER_HIGH..PLACEHOLDER_TAGGED_HIGH).contains(&value) {
        return None;
    }
    usize::try_from(value - PLACEHOLDER_HIGH).ok()
}

/// The import a tagged placeholder came from, named from the run's own call list.
///
/// The trace indexes every call by the stub it landed on, which is the same numbering the tag
/// carries - so no new plumbing is needed to turn a value back into a name.
fn source_of(trace: &CallTrace, value: u64) -> Option<&str> {
    let slot = tagged_stub(value)?;
    trace
        .calls
        .iter()
        .find(|c| c.index == slot)
        .map(|c| c.label.as_str())
}

/// Whether a value looks like one of our placeholders, at any small offset.
///
/// The offset matters: a guest that treats an error code as a struct pointer reads a field
/// through it, so the *faulting* address is the code plus or minus a little. Matching the
/// bare value alone would miss every case where the guest did anything with it (D125).
fn looks_like_placeholder(value: u64) -> bool {
    const NEAR: u64 = 0x1000;
    // The upper bound is the tagged range's, not the fixed one's: under
    // `ORBISTOUN_TAG_PLACEHOLDERS` a placeholder can be any `0x7fff_xxxx`, and a detector that
    // only knew the first sixteen would go blind exactly when asked to say more (D567).
    value >= PLACEHOLDER_LOW.saturating_sub(NEAR)
        && value < PLACEHOLDER_TAGGED_HIGH.saturating_add(NEAR)
}

/// A share of total calls, as a percentage, guarding against an empty run.
fn share(part: u64, whole: u64) -> u64 {
    part.saturating_mul(100).checked_div(whole).unwrap_or(0)
}

/// Everything a run says is worth doing, most actionable first.
///
/// Pure, so the rules are testable without running a guest - which matters more here than
/// usual, because a wrong rule produces a confident wrong instruction.
pub fn findings(trace: &CallTrace) -> Vec<Finding> {
    let mut out = Vec::new();
    out.extend(gave_up(trace));
    out.extend(error_used_as_pointer(trace));
    out.extend(spinning(trace));
    out.extend(abi_violation(trace));
    out.extend(short_reads(trace));
    out.extend(faulted(trace));
    out.extend(unnamed(trace));
    out.extend(unimplemented(trace));

    // Ranked by how much can be trusted, then by how much of the run it concerns. A
    // consumer taking the top item should be taking the one least likely to waste its
    // time.
    out.sort_by(|a, b| {
        a.confidence
            .cmp(&b.confidence)
            .then(b.weight.cmp(&a.weight))
    });
    out
}

/// The guest died touching memory, and what can be said about where.
///
/// Classification is deliberately mechanical - it reads the address and the regions the
/// run recorded, and says nothing it cannot support. "Not in any region this run mapped"
/// is a fact; "the allocator returned null" is a story, and stories are what a reader
/// should be forming rather than reading (D198).
fn faulted(trace: &CallTrace) -> Option<Finding> {
    let f = trace.fault.as_ref()?;
    // A guest that stopped itself is reported by `gave_up`; reporting both would rank one
    // outcome twice and put the less informative one above real gaps.
    if trace.stopped.is_some() {
        return None;
    }

    // Three shapes, distinguished only by arithmetic on the address. Each names a
    // different mistake, and the differences are what a reader would otherwise work out
    // by hand every time.
    let shape = if f.address == 0 {
        "a null pointer - something answered zero and the guest did not check".to_owned()
    } else if f.address < NEAR_NULL {
        "a null pointer plus an offset - a field read through a pointer that was zero".to_owned()
    } else if looks_like_placeholder(f.address) {
        "one of our own placeholder codes, used as an address".to_owned()
    } else if let Some(named) = marker(f.address) {
        named
    } else {
        "an address in no region this run mapped".to_owned()
    };

    let last: Vec<String> = trace.tail.iter().rev().take(4).map(traced_line).collect();

    let mut evidence = vec![format!("{} {:#x} is {shape}", f.kind, f.address)];
    if let Some(r) = &f.registers {
        // Name the null base before the raw dump, so the one register that mattered is not left
        // for the reader to find by matching sixteen values against the address.
        evidence.extend(null_base_registers(f.address, r));
        evidence.extend(r.lines());
    }
    // After the raw dump, because it is longer and a reader wants the values first. Empty
    // unless something the guest held pointed at memory this run had mapped (D522).
    evidence.extend(f.pointees.iter().cloned());
    evidence.extend(last.into_iter().map(|c| format!("{PRECEDED_BY}{c}")));

    Some(Finding {
        gap: Gap::Faulted,
        // Certain about *what happened*; the shape is described rather than diagnosed, so
        // nothing here rests on a guess about cause.
        confidence: Confidence::Certain,
        subject: f.region.clone(),
        what: format!(
            "the guest faulted at {}, {} {:#x}",
            describe_site(f),
            f.kind,
            f.address
        ),
        evidence,
        action: Some(
            concat!(
                "read the calls just before it and the arguments they were given - ",
                "the value that became this address was answered by one of them"
            )
            .to_owned(),
        ),
        // Weighted like `gave_up`, because they are the same class of statement: how the
        // run ended. Ranked below them it sat under findings about functions called twice,
        // which is the opposite of what a reader opening a failed run wants first.
        weight: trace.total_calls,
    })
}

/// Where the fault was, named against a region when the run knew one.
fn describe_site(f: &crate::trace::FaultSite) -> String {
    match (&f.region, f.offset) {
        (Some(region), Some(offset)) => format!("{region}+{offset:#x}"),
        _ => format!("{:#x}", f.instruction_pointer),
    }
}

/// Whether a faulting address is one of this project's own markers, and which.
///
/// **The arithmetic nobody should do by hand.** A run under a marker block faults on an
/// address like `0x5e2700002000`, and reading that as *field two* means dividing by a stride
/// a reader has to go and look up. The decoders live in `orbistoun-abi`, which is where the
/// markers are made; this only asks them (D369).
///
/// Two depths, because there are two. A **field** marker is what a handoff structure's
/// unestablished slot holds, so faulting on one means the guest used that field. A
/// **content** marker is what sits *behind* such a field, so faulting on one means the guest
/// read through the field and then used what it found - which names an offset as well.
fn marker(address: u64) -> Option<String> {
    use orbistoun_abi::enter::{content_slot, sentinel_slot};

    // The firmware skeleton, named before the handoff markers because a guest that reached into
    // it did so deliberately - it computed the address from a base and an offset - and "firmware
    // plus 0x2885e00" is the phrase that makes that arithmetic legible where a bare address hides
    // it (see orbistoun-firmware, and the sibling D404).
    if let Some(offset) = orbistoun_firmware::firmware_slot(address) {
        return Some(format!(
            "firmware+{offset:#x} - the guest reached into the firmware image, which holds a \
             zeroed skeleton and not the console's real memory"
        ));
    }

    if let Some((field, offset)) = content_slot(address) {
        return Some(if offset == 0 {
            format!("what handoff field {field} points at - the guest read through it")
        } else {
            format!(
                "handoff field {field} plus {offset:#x} - the guest read through the field and \
                 then used a value from that offset"
            )
        });
    }
    let (field, offset) = sentinel_slot(address)?;
    Some(if offset == 0 {
        format!("handoff field {field} itself, which nothing has established")
    } else {
        format!("handoff field {field} plus {offset:#x}, which nothing has established")
    })
}

/// Below this, an address is a small offset from null rather than a pointer.
///
/// A page. A field read through a null pointer lands within one; anything further is a
/// number that was never a pointer at all.
const NEAR_NULL: u64 = 0x1000;

/// The register(s) that look like the null base of a null-ish fault.
///
/// A fault at or just above zero is a dereference of a pointer that was zero, plus a struct
/// field offset. The dump already has all sixteen registers, but *which one was the pointer* is
/// left for a reader to work out by matching values against the address by hand - the exact step
/// the report exists to spare them, and the one that turns a null-write into an afternoon. This
/// does it: any register at or below the null page, where the fault address is that register
/// plus a field-sized offset, is named as the likely culprit. More than one may qualify when
/// several registers are zero; all are listed rather than a guess picked between them.
fn null_base_registers(fault_address: u64, r: &crate::trace::Registers) -> Vec<String> {
    let candidates: Vec<(&str, u64)> = [
        ("rax", r.rax),
        ("rbx", r.rbx),
        ("rcx", r.rcx),
        ("rdx", r.rdx),
        ("rsi", r.rsi),
        ("rdi", r.rdi),
        ("rbp", r.rbp),
        ("rsp", r.rsp),
        ("r8", r.r8),
        ("r9", r.r9),
        ("r10", r.r10),
        ("r11", r.r11),
        ("r12", r.r12),
        ("r13", r.r13),
        ("r14", r.r14),
        ("r15", r.r15),
    ]
    .into_iter()
    .filter(|(_, value)| {
        *value < NEAR_NULL && fault_address >= *value && fault_address - *value < NEAR_NULL
    })
    .collect();
    // A textbook null dereference has a base of *exactly* zero; when any register is zero those
    // are the culprits, and a small-but-nonzero register - often the value being stored, which
    // matched only by coincidence - is noise to be dropped.
    let any_zero = candidates.iter().any(|(_, value)| *value == 0);
    candidates
        .into_iter()
        .filter(|(_, value)| !any_zero || *value == 0)
        .map(|(name, value)| {
            format!(
                ">> the null base is likely {name} (={value:#x}) - the access is {name} + {:#x}, so find where {name} was set to zero",
                fault_address - value
            )
        })
        .collect()
}

/// The guest stopped itself.
fn gave_up(trace: &CallTrace) -> Option<Finding> {
    let stopped = trace.stopped.as_ref()?;
    // What it called immediately before deciding, which is the closest thing to a reason
    // the guest offers.
    let last: Vec<String> = trace.tail.iter().rev().take(4).map(traced_line).collect();
    Some(Finding {
        gap: Gap::GuestGaveUp,
        confidence: Confidence::Certain,
        subject: None,
        what: format!("{stopped} - it decided to stop rather than failing"),
        evidence: {
            let mut e = vec![format!("after {} calls", trace.total_calls)];
            e.extend(last.into_iter().map(|c| format!("{PRECEDED_BY}{c}")));
            e
        },
        action: Some(
            "read the calls immediately before it - a guest that gives up usually reports \
             why first, and that call is the gap"
                .to_owned(),
        ),
        weight: trace.total_calls,
    })
}

/// One traced call as an evidence line: what was called, its first argument, what it
/// **answered** where that is known, and the call site.
///
/// The return is shown only when it was recorded. The faulting call's own frame, and any
/// call still running, has none - and `-> ?` there would read as an answer of "unknown"
/// where saying nothing is the honest thing (D459).
fn traced_line(c: &TracedCall) -> String {
    match c.returned {
        Some(ret) => format!(
            "{}({:#x}) -> {ret:#x} from {:#x}",
            c.label, c.args[0], c.from
        ),
        None => format!("{}({:#x}) from {:#x}", c.label, c.args[0], c.from),
    }
}

/// The last few calls a run made, as evidence lines.
///
/// Shared by the findings whose action tells a reader to look at them, so the list a person is
/// sent to and the list a dispatcher sweeps are the same list (D299).
fn preceding(trace: &CallTrace) -> Vec<String> {
    trace
        .tail
        .iter()
        .rev()
        .take(4)
        .map(|c| format!("{PRECEDED_BY}{}", traced_line(c)))
        .collect()
}

/// A placeholder code being used by the guest as a pointer.
fn error_used_as_pointer(trace: &CallTrace) -> Vec<Finding> {
    let mut out = Vec::new();

    // The guest passing one of our codes *into* a later call. Whatever answered it is the
    // function to fix, and the call that received it names the moment.
    // **Every argument, not only the first.** A placeholder handed on as a *size* is visible only
    // if the size happens to be argument zero - `malloc`'s is, which is the sole reason D564's four
    // gigabytes were ever seen. The same value in `rdx` left no trace at all until now (D570).
    for (call, register) in trace
        .tail
        .iter()
        .flat_map(|c| (0..c.args.len()).map(move |r| (c, r)))
    {
        let value = call.args[register];
        if !looks_like_placeholder(value) {
            continue;
        }
        let which = ARGUMENT_REGISTERS.get(register).copied().unwrap_or("?");
        out.push(Finding {
            gap: Gap::ErrorUsedAsPointer,
            confidence: Confidence::Certain,
            subject: Some(call.label.clone()),
            what: match source_of(trace, value) {
                // Tagged: the value names its own source, so the finding says it outright.
                Some(source) => format!(
                    "{} was passed {value:#x} in {which} - the placeholder {source} answered",
                    call.label
                ),
                None => format!(
                    "{} was passed {value:#x} in {which} - one of our own placeholder codes",
                    call.label
                ),
            },
            evidence: {
                // **The calls this finding's own action points at.** It says "find what
                // answered with that code just before", and a finding whose action sends a
                // reader looking must carry what they are to look at - otherwise the search
                // is a person's by construction rather than by choice (D299).
                let mut e = vec![
                    format!("call #{} from {:#x}", call.sequence, call.from),
                    "the guest is treating an unimplemented answer as data".to_owned(),
                ];
                e.extend(preceding(trace));
                e
            },
            action: Some(match source_of(trace, value) {
                // D299: a finding that sends a reader looking must carry what they are to
                // look at. With a tag there is nothing to look for - the value is the answer.
                Some(source) => format!(
                    "give {source} a real return - a function whose answer is read as data \
                     must never answer an error code (D125)"
                ),
                None => "find what answered with that code just before, and give it a real \
                         return - a pointer-returning function must never answer an error \
                         code (D125). Re-run under ORBISTOUN_TAG_PLACEHOLDERS to be told \
                         which (D567)"
                    .to_owned(),
            }),
            weight: 1,
        });
    }

    // And the same code arriving as a faulting address, which is the guest having
    // dereferenced it.
    if let Some(fault) = &trace.fault {
        if looks_like_placeholder(fault.address) {
            out.push(Finding {
                gap: Gap::ErrorUsedAsPointer,
                confidence: Confidence::Certain,
                subject: fault.inside_import.clone(),
                what: format!(
                    "the run died dereferencing {:#x}, which is one of our placeholder codes",
                    fault.address
                ),
                evidence: {
                    let mut e = vec![format!(
                        "{} {:#x} at {}",
                        fault.kind,
                        fault.address,
                        fault
                            .region
                            .as_deref()
                            .unwrap_or("an address outside every placed region")
                    )];
                    e.extend(preceding(trace));
                    e
                },
                action: Some(
                    "the function that returned it must answer a real value; if it returns \
                     a pointer or handle, it needs an implementation rather than a policy \
                     change"
                        .to_owned(),
                ),
                weight: trace.total_calls,
            });
        }
    }
    out
}

/// One call dominating the run.
fn spinning(trace: &CallTrace) -> Option<Finding> {
    /// Below this the guest is merely busy, not stuck.
    const DOMINANT: u64 = 90;
    /// And a run has to be long enough for a share to mean anything.
    const ENOUGH: u64 = 10_000;

    let top = trace.calls.first()?;
    let share = share(top.calls, trace.total_calls);
    if share < DOMINANT || trace.total_calls < ENOUGH {
        return None;
    }
    Some(Finding {
        gap: Gap::Spinning,
        confidence: Confidence::Certain,
        subject: Some(top.label.clone()),
        what: format!(
            "{} is {share}% of {} calls - the guest is repeating it rather than progressing",
            top.label, trace.total_calls
        ),
        evidence: vec![
            format!("{} calls to one function", top.calls),
            "a guest that keeps asking the same question has not accepted the answer".to_owned(),
        ],
        action: Some(
            "the answer this returns is being rejected. Vary it and watch whether the call \
             pattern changes - the shape of the loop says more than the return code"
                .to_owned(),
        ),
        weight: top.calls,
    })
}

/// Calls arriving on a stack the convention forbids.
fn abi_violation(trace: &CallTrace) -> Option<Finding> {
    if trace.abi.misaligned_calls == 0 {
        return None;
    }
    Some(Finding {
        gap: Gap::AbiViolation,
        confidence: Confidence::Certain,
        subject: trace.abi.first_misaligned_import.clone(),
        what: format!(
            "{} of {} calls arrived on a misaligned stack",
            trace.abi.misaligned_calls, trace.total_calls
        ),
        evidence: {
            let mut e =
                vec!["System V requires rsp % 16 == 8 at a callee's first instruction".to_owned()];
            if let Some(rsp) = trace.abi.first_misaligned_rsp {
                e.push(format!(
                    "first offender arrived with rsp {rsp:#x} (% 16 = {})",
                    rsp % 16
                ));
            }
            e
        },
        action: Some(
            "this is almost never the guest's fault - check how it is entered. A remainder \
             of 0 means control arrived by a jump where a call was expected"
                .to_owned(),
        ),
        weight: trace.abi.misaligned_calls,
    })
}

/// Reads that delivered less than was asked for.
fn short_reads(trace: &CallTrace) -> Option<Finding> {
    if trace.reads.short == 0 {
        return None;
    }
    Some(Finding {
        gap: Gap::ShortRead,
        confidence: Confidence::Likely,
        subject: None,
        what: format!(
            "{} of {} file reads were cut short before the end of their file",
            trace.reads.short, trace.reads.reads
        ),
        evidence: vec![format!("{} bytes delivered in total", trace.reads.bytes)],
        action: Some(
            "a guest that receives a truncated asset faults inside its own parser, far \
             from here - check the length arithmetic before looking anywhere else"
                .to_owned(),
        ),
        weight: trace.reads.short,
    })
}

/// Named functions the guest used that nothing implements.
///
/// **The most directly actionable category.** It names a function, says how much the guest
/// leaned on it, and the work is unambiguous - unlike a fault, which says where something
/// went wrong without saying what would fix it.
///
/// Unnamed hashes are excluded: they are already reported as a naming gap, and "implement
/// `libkernel::0xcedb06001fd4c617`" is not an instruction anyone can follow.
fn unimplemented(trace: &CallTrace) -> Vec<Finding> {
    trace
        .calls
        .iter()
        .filter(|c| !c.implemented && !c.label.contains("::0x"))
        .map(|c| Finding {
            gap: Gap::Unimplemented,
            confidence: Confidence::Certain,
            subject: Some(c.label.clone()),
            what: format!(
                "{} was called {} times and nothing implements it",
                c.label, c.calls
            ),
            evidence: vec![
                "the call landed on a stub, which answered a placeholder".to_owned(),
            ],
            action: Some(format!(
                "implement it in the crate declaring {}; record what it returns in its knowledge file first, because a function answering a pointer must never answer an error code",
                c.label.split("::").next().unwrap_or("that library")
            )),
            weight: c.calls,
        })
        .collect()
}

/// Imports still known only by hash.
fn unnamed(trace: &CallTrace) -> Vec<Finding> {
    trace
        .calls
        .iter()
        .filter(|c| c.label.contains("::0x"))
        .map(|c| Finding {
            gap: Gap::Unnamed,
            confidence: Confidence::Certain,
            subject: Some(c.label.clone()),
            what: format!("{} was called {} times and has no name", c.label, c.calls),
            evidence: vec!["the hash resolved to no name in the symbol database".to_owned()],
            // Names the commands, because "extend the vocabulary" is advice and a command
            // is an action. `suggest` is mentioned rather than run: it is slow, optional,
            // and nothing on this path should ever wait on a model.
            action: Some(
                concat!(
                    "extend the candidate vocabulary and re-run the name search - a name ",
                    "is confirmed by the hash agreeing, never by consulting a table. ",
                    "`./bin/orbistoun names` re-runs it; `./bin/orbistoun suggest` asks a ",
                    "local model for words first, for when the vocabulary is what is short"
                )
                .to_owned(),
            ),
            weight: c.calls,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        Confidence, Gap, findings, looks_like_placeholder, marker, null_base_registers, source_of,
        tagged_stub,
    };
    use crate::trace::{
        AbiReport, CallTrace, CalledImport, Conditions, FaultSite, FormatReport, ReadReport,
        Registers, TracedCall,
    };

    /// Registers with every field a distinct value well above the null page, so a test can zero
    /// exactly the ones it means to and nothing matches the null-base check by accident. (The
    /// real hazard the default `0` would hide: every unset field looking like a null base.)
    fn well_placed_registers() -> Registers {
        Registers {
            rax: 0x4001,
            rbx: 0x4002,
            rcx: 0x4003,
            rdx: 0x4004,
            rsi: 0x4005,
            rdi: 0x4006,
            rbp: 0x4007,
            rsp: 0x4008,
            r8: 0x4009,
            r9: 0x400a,
            r10: 0x400b,
            r11: 0x400c,
            r12: 0x400d,
            r13: 0x400e,
            r14: 0x400f,
            r15: 0x4010,
        }
    }

    /// **The register that was the null pointer is named, and the value that merely looked like
    /// one is not.** A null-plus-offset fault has a base of exactly zero and a small offset; a
    /// register holding the stored value can match the same arithmetic by coincidence, and
    /// listing it would send a reader after the wrong register - the manual step this removes.
    #[test]
    fn the_null_base_register_is_named_and_a_coincidence_is_not() {
        // `mov [r12+0x10], r14d` with r12 zero: the fault is at 0x10, r12 is the base, and r14
        // holds the stored value 0x10 - which matches the address with a zero offset by chance.
        let mut regs = well_placed_registers();
        regs.r12 = 0;
        regs.r14 = 0x10;
        let named = null_base_registers(0x10, &regs);
        assert_eq!(
            named.len(),
            1,
            "exactly one culprit, not the coincidence: {named:?}"
        );
        assert!(
            named[0].contains("r12 + 0x10"),
            "the zero base and its offset: {named:?}"
        );
        assert!(
            !named[0].contains("r14"),
            "the stored value is not mistaken for the base"
        );
    }

    /// Two genuinely-zero registers are both offered rather than one guessed between.
    #[test]
    fn several_zero_bases_are_all_offered() {
        let mut regs = well_placed_registers();
        regs.rax = 0;
        regs.rcx = 0;
        let named = null_base_registers(0, &regs);
        assert_eq!(named.len(), 2, "both zero registers: {named:?}");
    }

    /// A fault far from zero names nothing - the check is for null bases, not any register that
    /// happens to sit below an address.
    #[test]
    fn a_fault_that_is_not_null_ish_names_no_base() {
        let mut regs = well_placed_registers();
        regs.rax = 0;
        assert!(
            null_base_registers(0x4000_0000, &regs).is_empty(),
            "0x0 is not within a field's reach of 0x40000000"
        );
    }

    /// **The arithmetic a reader should not be doing** (D369).
    ///
    /// A run under a marker block faults on an address like `0x5e2700002000`, and reading
    /// that as *field two* means dividing by a stride you have to go and look up. It came up
    /// three times in one session before this existed.
    #[test]
    fn a_marker_address_is_named_rather_than_left_as_arithmetic() {
        use orbistoun_abi::enter::{CONTENT_BASE, CONTENT_STRIDE, SENTINEL_BASE, SENTINEL_STRIDE};

        let field = marker(SENTINEL_BASE + 2 * SENTINEL_STRIDE).expect("a field marker");
        assert!(field.contains("field 2"), "{field}");

        let through = marker(CONTENT_BASE + 2 * CONTENT_STRIDE).expect("a content marker");
        assert!(through.contains("read through"), "{through}");

        let deeper = marker(CONTENT_BASE + 2 * CONTENT_STRIDE + 0x18).expect("with an offset");
        assert!(deeper.contains("0x18"), "{deeper}");
    }

    /// An address that is not one of ours is not described as one.
    #[test]
    fn an_ordinary_address_is_not_mistaken_for_a_marker() {
        assert_eq!(marker(0x4000_0000_1234), None);
        assert_eq!(marker(0), None);
    }

    fn empty() -> CallTrace {
        CallTrace {
            module: "m".to_owned(),
            reached: "Entered".to_owned(),
            total_calls: 0,
            distinct: 0,
            frames: 0,
            calls: Vec::new(),
            syscalls: Vec::new(),
            tail: Vec::new(),
            abi: AbiReport::default(),
            reads: ReadReport::default(),
            dumps: Vec::new(),
            conditions: Conditions::default(),
            formats: FormatReport::default(),
            stopped: None,
            fault: None,
        }
    }

    fn call(label: &str, arg0: u64) -> TracedCall {
        TracedCall {
            sequence: 1,
            label: label.to_owned(),
            args: [arg0, 0, 0, 0, 0, 0],
            from: 0x1000,
            returned: None,
        }
    }

    #[test]
    fn a_called_function_nothing_implements_is_the_clearest_instruction_there_is() {
        // It names a function, says how much the guest leaned on it, and the work is
        // unambiguous - unlike a fault, which says where something broke without saying
        // what would fix it.
        let mut trace = empty();
        trace.total_calls = 40;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libSceAgc::sceAgcCreateShader".to_owned(),
            calls: 40,
            implemented: false,
        }];
        let found = findings(&trace);
        assert_eq!(found[0].gap, Gap::Unimplemented);
        assert!(
            found[0]
                .action
                .as_ref()
                .expect("has one")
                .contains("libSceAgc")
        );
    }

    #[test]
    fn an_unnamed_hash_is_a_naming_gap_and_not_an_implementation_one() {
        // "Implement libkernel::0xcedb06001fd4c617" is not an instruction anyone can
        // follow - it has to be named before it can be written.
        let mut trace = empty();
        trace.total_calls = 3;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libkernel::0xcedb06001fd4c617".to_owned(),
            calls: 3,
            implemented: false,
        }];
        let kinds: Vec<Gap> = findings(&trace).iter().map(|f| f.gap).collect();
        assert_eq!(kinds, vec![Gap::Unnamed]);
    }

    #[test]
    fn a_clean_run_produces_nothing_to_do() {
        // The list must be empty when there is nothing wrong, or every run reports work
        // and the ranking stops meaning anything.
        assert!(findings(&empty()).is_empty());
    }

    #[test]
    fn a_placeholder_passed_as_an_argument_is_reported_with_certainty() {
        // The most productive signal this project has: it names the call that received a
        // bad answer and proves the guest believed it.
        let mut trace = empty();
        trace.tail = vec![call(
            "libSceVideoOut::sceVideoOutRegisterBuffers2",
            0x7FFF_0001,
        )];
        let found = findings(&trace);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].gap, Gap::ErrorUsedAsPointer);
        assert_eq!(found[0].confidence, Confidence::Certain);
        assert!(
            found[0].action.is_some(),
            "a finding without an action is just news"
        );
    }

    #[test]
    fn a_placeholder_is_recognised_at_an_offset_from_itself() {
        // A guest treating a code as a struct pointer reads a *field* through it, so the
        // address that faults is the code plus or minus a little. Matching the bare value
        // would miss every case where the guest did anything with it.
        assert!(looks_like_placeholder(0x7FFF_0001));
        assert!(looks_like_placeholder(0x7FFF_0019), "code plus 0x18");
        assert!(looks_like_placeholder(0x7FFE_FFF9), "code minus 8");
        assert!(!looks_like_placeholder(0));
        assert!(!looks_like_placeholder(0x4000_0000_0000));
    }

    #[test]
    fn one_call_dominating_a_long_run_is_a_spin() {
        let mut trace = empty();
        trace.total_calls = 1_000_000;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libkernel::sceKernelDirectMemoryQuery".to_owned(),
            calls: 999_000,
            implemented: true,
        }];
        let found = findings(&trace);
        assert_eq!(found[0].gap, Gap::Spinning);
    }

    #[test]
    fn a_short_run_dominated_by_one_call_is_not_a_spin() {
        // A guest that calls `memset` four times out of five during startup is busy, not
        // stuck, and reporting that as a spin would bury the real ones.
        let mut trace = empty();
        trace.total_calls = 5;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libc::memset".to_owned(),
            calls: 4,
            implemented: true,
        }];
        assert!(findings(&trace).is_empty());
    }

    #[test]
    fn giving_up_outranks_everything_else_and_carries_its_last_calls() {
        // A guest that stopped deliberately said the most useful thing in the run, and it
        // said it in the calls immediately before.
        let mut trace = empty();
        trace.total_calls = 53;
        trace.stopped = Some("the guest called abort".to_owned());
        trace.tail = vec![call("libkernel::0x48a758b2e731cfd7", 0x6000_0080_0ef0)];
        let found = findings(&trace);
        assert_eq!(found[0].gap, Gap::GuestGaveUp);
        assert!(
            found[0].evidence.iter().any(|e| e.contains("just before")),
            "the reason is in what it called last"
        );
    }

    #[test]
    fn findings_are_ranked_by_confidence_before_weight() {
        // A consumer taking the top item must be taking the one least likely to waste its
        // time - a heavy guess must not outrank a light certainty.
        let mut trace = empty();
        trace.reads = ReadReport {
            reads: 100,
            short: 90,
            bytes: 10,
        };
        trace.tail = vec![call("libc::something", 0x7FFF_0001)];
        let found = findings(&trace);
        assert_eq!(found[0].confidence, Confidence::Certain);
        assert_eq!(found[0].gap, Gap::ErrorUsedAsPointer);
        assert_eq!(found.last().expect("two findings").gap, Gap::ShortRead);
    }

    #[test]
    fn every_finding_says_where_to_look() {
        // The point of a classification is that a consumer can route it without reading
        // the prose.
        let mut trace = empty();
        trace.fault = Some(FaultSite {
            pointees: Vec::new(),
            kind: "read of".to_owned(),
            address: 0x7FFF_0001,
            instruction_pointer: 0x1234,
            region: Some("image".to_owned()),
            offset: Some(0x1234),
            inside_import: None,
            registers: None,
            frames: Vec::new(),
        });
        for finding in findings(&trace) {
            assert!(!finding.gap.where_to_look().is_empty());
            assert!(!finding.evidence.is_empty(), "a claim needs its evidence");
        }
    }

    /// A trace whose call list places two imports at known stub slots.
    fn trace_with_slots() -> CallTrace {
        CallTrace {
            module: "m".to_owned(),
            reached: "Entered".to_owned(),
            total_calls: 2,
            distinct: 2,
            frames: 0,
            calls: vec![
                CalledImport {
                    index: 0x81,
                    label: "libSceUlt::sceUltUlthreadRuntimeGetWorkAreaSize".to_owned(),
                    calls: 1,
                    implemented: false,
                },
                CalledImport {
                    index: 0x215,
                    label: "libSceAgc::sceAgcCreateShader".to_owned(),
                    calls: 1,
                    implemented: false,
                },
            ],
            syscalls: Vec::new(),
            tail: Vec::new(),
            abi: AbiReport::default(),
            reads: ReadReport::default(),
            dumps: Vec::new(),
            conditions: Conditions::default(),
            formats: FormatReport::default(),
            fault: None,
            stopped: None,
        }
    }

    /// **A tagged placeholder names the function that answered it.**
    ///
    /// The whole point of D567. Untagged, every stub answers `0x7fff_0001`, so a placeholder in a
    /// guest's argument says *some* unimplemented function produced it - and the finding could
    /// only tell a reader to go looking, which D299 says a finding must not do.
    ///
    /// The tag is `0x7fff_0000 | (0x10 + slot)`, and the trace indexes calls by the same slot, so
    /// the value resolves to a name with no new plumbing.
    ///
    /// # What this cannot assert
    ///
    /// That the slot numbering the service tags with is the one the trace records. They are the
    /// same global stub index today; nothing here would notice if one of them started counting
    /// differently, and the symptom would be a confident finding naming the wrong function.
    #[test]
    fn a_tagged_placeholder_names_its_source() {
        let trace = trace_with_slots();
        assert_eq!(
            source_of(&trace, 0x7fff_0091),
            Some("libSceUlt::sceUltUlthreadRuntimeGetWorkAreaSize"),
            "0x91 is slot 0x81 plus the 0x10 floor"
        );
        assert_eq!(
            source_of(&trace, 0x7fff_0225),
            Some("libSceAgc::sceAgcCreateShader")
        );
        // **A tag for a slot this run never called resolves to nothing**, and this is the
        // load-bearing half rather than hygiene. Guest registers routinely hold stale values
        // that fall inside the tag range: PPSA28061's tail carries `0x7fff0201` and
        // `0x7fffbe01` in argument slots, which decode to stubs 497 and 48,625 - neither of
        // which that run ever called. Without this check both would have been reported as
        // confident attributions built from garbage, which is worse than the vague finding
        // tagging replaced (D570).
        assert_eq!(source_of(&trace, 0x7fff_0999), None);
        assert_eq!(
            source_of(&trace, 0x7fff_0201),
            None,
            "PPSA28061's real stale register"
        );
        assert_eq!(source_of(&trace, 0x7fff_be01), None, "and the other one");
    }

    /// **An untagged placeholder names nothing, and must not pretend to.**
    ///
    /// `0x7fff_0001` is what every stub answers when tagging is off, and the fixed `GuestError`
    /// codes live below `0x7fff_0010`. Reading one of those as a slot would attribute a finding to
    /// whichever import happened to be at index 0 - a confident, wrong answer, which is worse than
    /// the vague one it replaced.
    #[test]
    fn an_untagged_placeholder_attributes_nothing() {
        let trace = trace_with_slots();
        for fixed in [0x7fff_0000_u64, 0x7fff_0001, 0x7fff_000f] {
            assert_eq!(
                tagged_stub(fixed),
                None,
                "{fixed:#x} is a fixed placeholder code, not a tag"
            );
            assert_eq!(source_of(&trace, fixed), None);
        }
    }

    /// **The detector still recognises a tagged placeholder as one of ours.**
    ///
    /// `looks_like_placeholder` bounded itself at `0x7fff_0010` - the fixed codes. A tagged run
    /// answers far above that, so the detector would have gone blind **exactly when it was asked
    /// to say more**, and the diagnostic would have silently reported nothing.
    #[test]
    fn the_detector_sees_tagged_placeholders_too() {
        for tagged in [0x7fff_0010_u64, 0x7fff_0091, 0x7fff_0225, 0x7fff_beac] {
            assert!(
                looks_like_placeholder(tagged),
                "{tagged:#x} is one of ours and the detector missed it"
            );
        }
        // And still recognises the untagged one, and still rejects an ordinary address.
        assert!(looks_like_placeholder(0x7fff_0001));
        assert!(!looks_like_placeholder(0x4000_0000_0000));
    }
}
