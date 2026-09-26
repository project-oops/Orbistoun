//! Turning a run into a ranked list of things to do about it.
//!
//! Everything here is visible somewhere in a run's output (the ranked import list, the fault, the
//! call tail, the stack conformance line); findings state it as data - what is wrong, where, what
//! evidence says so and what would address it - so a consumer, person or tool, re-derives nothing
//! from prose.
//!
//! Confidence is the load-bearing field, because a confidently wrong suggestion gets acted on. A
//! certain finding is a defect and a possible one is a conversation; nothing reports `Certain`
//! unless the trace shows it (D179).

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
/// The kind lets a consumer route a finding without parsing prose: implementing a function, naming
/// a hash and fixing a contract are different jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Gap {
    /// A function the guest called that has no implementation.
    Unimplemented,
    /// An import whose name is still a bare hash.
    Unnamed,
    /// A placeholder error code being used by the guest as a pointer or handle.
    ///
    /// It names the function that answered wrongly and shows that the guest believed the answer.
    ErrorUsedAsPointer,
    /// The guest gave up deliberately.
    GuestGaveUp,
    /// The guest died touching an address, and the address says which kind of mistake.
    ///
    /// The finding carries the calls leading in, the registers and the arguments, so none of them
    /// has to be read out of the trace by hand.
    Faulted,
    /// The guest entered the kernel through an instruction orbistoun has no handler for: a
    /// `syscall` or `hlt`, or an `int` on an unmeasured vector.
    ///
    /// Distinct from [`Self::Faulted`] because the job differs: characterise this kernel entry and
    /// add its handler, rather than find a bad pointer. `int 0x41` is not in this class: it is
    /// fatal on hardware with no return, so reaching it is a [`Self::Faulted`] caused by an
    /// upstream wrong value.
    KernelEntryUnimplemented,
    /// One call dominating the run, which means the guest is not progressing.
    Spinning,
    /// Guest calls arriving on a stack the calling convention forbids.
    AbiViolation,
    /// A file read that delivered less than was asked for.
    ShortRead,
    /// Arguments captured because somebody asked for them by name.
    ///
    /// An answer rather than a gap: naming an implemented import with `ORBISTOUN_DUMP` shows its
    /// arguments even though no other finding is about that import.
    Captured,
    /// The guest handed a command buffer to the graphics driver.
    ///
    /// Progress, not a wall. Like [`Self::Captured`] it is an answer rather than a gap: the finding
    /// says what the command stream holds, so the next work (translating the shaders its registers
    /// name, then a backend to run them) is ranked rather than guessed at.
    Submitted,
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
            Self::KernelEntryUnimplemented => {
                "orbistoun's interrupt/syscall handling, and an obSCEne measurement of the vector"
            }
            Self::GuestGaveUp | Self::Spinning | Self::Faulted => "the calls immediately before it",
            Self::AbiViolation => "crates/orbistoun-thunk, and how the guest is entered",
            Self::ShortRead => "crates/orbistoun-fs",
            Self::Captured => "the implementation you asked about",
            Self::Submitted => "crates/orbistoun-gpu and its pipeline, then a backend crate",
        }
    }
}

/// How the call leading into a wall is marked in [`Finding::evidence`].
///
/// The subject of a fault is the region the guest died in, so anything wanting the call that led
/// there reads it out of the evidence by this one prefix.
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
    /// Why the run says so: facts from the trace, never inference.
    pub evidence: Vec<String>,
    /// What would address it, if that is knowable from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// How many calls this concerns, used for ranking.
    pub weight: u64,
}

/// What each integer argument arrives in, so a finding can say which one carried the value.
const ARGUMENT_REGISTERS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];

/// The base of the placeholder block, from the core's single definition.
///
/// Placeholders sit in a range no real firmware value occupies, which makes them findable in a
/// guest's arguments. The high bit is set so a guest's own `rc < 0` check reads them as negative
/// (D670).
const PLACEHOLDER_LOW: u64 = orbistoun_core::PLACEHOLDER_BASE as u64;
/// One past the fixed `GuestError` codes (`PLACEHOLDER_BASE | 0x1..=0x4`); tagged placeholders
/// begin here.
const PLACEHOLDER_HIGH: u64 = PLACEHOLDER_LOW + 0x10;

/// One past every placeholder, tagged ones included.
///
/// `ORBISTOUN_TAG_PLACEHOLDERS` gives each stub `PLACEHOLDER_BASE | (0x10 + its slot)`, so a tagged
/// value sits above [`PLACEHOLDER_HIGH`] and inside the reserved half-word, with the same high bit
/// as the untagged code.
const PLACEHOLDER_TAGGED_HIGH: u64 = PLACEHOLDER_LOW + 0x1_0000;

/// Folds a placeholder widened from `int` to `long` back to 32 bits.
///
/// A negative code sign-extends (`0xF7FF_0001` becomes `0xFFFF_FFFF_F7FF_0001`), so a value whose
/// top half is all ones is tested by its low half, the same normalisation
/// `orbistoun_core::placeholder_named` does (D670). Anything else above 32 bits is not one of ours.
fn low32_if_sign_extended(value: u64) -> u64 {
    if value >> 32 == 0xFFFF_FFFF {
        value & 0xFFFF_FFFF
    } else {
        value
    }
}

/// Which stub produced a tagged placeholder, if this value is one.
///
/// [`None`] for an untagged placeholder: `PLACEHOLDER_BASE | 0x1` says only that some unimplemented
/// function answered (D567).
fn tagged_stub(value: u64) -> Option<usize> {
    let value = low32_if_sign_extended(value);
    if !(PLACEHOLDER_HIGH..PLACEHOLDER_TAGGED_HIGH).contains(&value) {
        return None;
    }
    usize::try_from(value - PLACEHOLDER_HIGH).ok()
}

/// The import a tagged placeholder came from, named from the run's own call list.
///
/// The trace indexes every call by the stub it landed on, the same numbering the tag carries.
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
/// A guest that treats an error code as a struct pointer reads a field through it, so the faulting
/// address is the code plus or minus a little.
fn looks_like_placeholder(value: u64) -> bool {
    const NEAR: u64 = 0x1000;
    let value = low32_if_sign_extended(value);
    // The upper bound is the tagged range's, so the detector also sees every `PLACEHOLDER_BASE |
    // 0xxxxx` under `ORBISTOUN_TAG_PLACEHOLDERS` (D567).
    value >= PLACEHOLDER_LOW.saturating_sub(NEAR)
        && value < PLACEHOLDER_TAGGED_HIGH.saturating_add(NEAR)
}

/// A share of total calls, as a percentage, guarding against an empty run.
fn share(part: u64, whole: u64) -> u64 {
    part.saturating_mul(100).checked_div(whole).unwrap_or(0)
}

/// Everything a run says is worth doing, most actionable first.
///
/// Pure, so the rules are testable without running a guest.
pub fn findings(trace: &CallTrace) -> Vec<Finding> {
    let mut out = Vec::new();
    out.extend(gave_up(trace));
    out.extend(error_used_as_pointer(trace));
    out.extend(spinning(trace));
    out.extend(abi_violation(trace));
    out.extend(short_reads(trace));
    out.extend(submitted(trace));
    out.extend(faulted(trace));
    out.extend(unnamed(trace));
    out.extend(unimplemented(trace));
    // Last, because it asks what the others already claim: an unimplemented import keeps its dump
    // where it is shown.
    let claimed = captured(trace, &out);
    out.extend(claimed);

    // Ranked by confidence, then by how much of the run it concerns, so the top item is the one
    // least likely to waste a consumer's time.
    out.sort_by(|a, b| {
        a.confidence
            .cmp(&b.confidence)
            .then(b.weight.cmp(&a.weight))
    });
    out
}

/// The guest died touching memory, and what can be said about where.
///
/// Classification is mechanical: it reads the address and the regions the run recorded and says
/// nothing it cannot support. "Not in any region this run mapped" is a fact; "the allocator
/// returned null" is a diagnosis, which is the reader's (D179).
fn faulted(trace: &CallTrace) -> Option<Finding> {
    let f = trace.fault.as_ref()?;
    // A guest that stopped itself is reported by `gave_up`; reporting both would rank one outcome
    // twice.
    if trace.stopped.is_some() {
        return None;
    }

    // The instruction is checked before the address. A trap instruction (`int 0x41`, `syscall`,
    // `hlt`) raises a general-protection fault the host reports as a read of an arbitrary address,
    // often -1, which the address shapes below would misread as a bad pointer.
    if let Some(kind) = crate::trace::classify_trap(&f.instruction) {
        return Some(kernel_entry_finding(trace, f, kind));
    }

    // Three shapes, distinguished only by arithmetic on the address; each names a different
    // mistake.
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

    // This thread's calls, then a few of other threads', labelled. The tail is every thread's and
    // the fault is one thread's, so they are not read as one sequence (D621).
    let last: Vec<String> = on_this_thread(trace, f.host_thread)
        .into_iter()
        .chain(on_other_threads(trace, f.host_thread))
        .collect();

    // The call that supplied the bad pointer: the most recent one whose return matches the
    // dereferenced base.
    let source = pointer_source(trace, f.address, f.host_thread);

    // The copy that faulted reading its source, read from the recorded argument rather than the
    // register dump. `None` for any fault that is not a byte copy reading its source.
    let copy = copy_reading_fault(trace, f.address, f.host_thread);

    let mut evidence = vec![format!("{} {:#x} is {shape}", f.kind, f.address)];
    if let Some(c) = source {
        // Stated as an observation only: this call's answer is the value the guest dereferenced.
        // Whether this call or an earlier one is the gap is left open, because a call can answer
        // zero correctly.
        evidence.push(format!(
            concat!(
                ">> {} answered {} immediately before, and the guest dereferenced that value ",
                "here without checking it"
            ),
            c.label,
            c.returned
                .map_or_else(|| "that".to_owned(), |r| format!("{r:#x}")),
        ));
    }
    // The lead when a copy is the fault, from its recorded source, or nothing when no copy read
    // there.
    evidence.extend(copy_source_lead(copy, f.address));
    if let Some(r) = &f.registers {
        // Names the null base before the raw dump, so the register that mattered is not left to be
        // matched by hand.
        evidence.extend(null_base_registers(f.address, r));
        evidence.extend(r.lines());
    }
    // After the raw dump, which a reader wants first. Empty unless something the guest held pointed
    // at memory this run had mapped.
    evidence.extend(f.pointees.iter().cloned());
    evidence.extend(last);

    Some(Finding {
        gap: Gap::Faulted,
        // Certain about what happened; the shape is described rather than diagnosed.
        confidence: Confidence::Certain,
        subject: f.region.clone(),
        what: format!(
            "the guest faulted at {}, {} {:#x}{}",
            describe_site(f),
            f.kind,
            f.address,
            // Which thread, when there is one: the calls listed underneath are every thread's
            // (D621).
            f.thread
                .map_or_else(String::new, |t| format!(", on guest thread {t:#x}"))
        ),
        evidence,
        // Routed to the supplying call when the trace shows one and to the general search when it
        // does not, stated as a lead rather than a verdict: an implemented function answering zero
        // is usually correct.
        action: Some(fault_action(copy, source)),
        // Weighted like `gave_up`: both state how the run ended, which a reader opening a failed
        // run wants first.
        weight: trace.total_calls,
    })
}

/// What to do about a memory fault: follow a copy's source, or the call that supplied the pointer.
fn fault_action(copy: Option<&TracedCall>, source: Option<&TracedCall>) -> String {
    if let Some(c) = copy {
        // A copy that faulted reading its source is a faithful primitive, not the gap: route to
        // whatever produced the source pointer, never to the copy itself.
        format!(
            concat!(
                "{} is a faithful byte copy - it moved what it was given. The gap is whatever ",
                "produced its source pointer {:#x}; read the calls on this thread before it for ",
                "the lookup or allocation that should have answered a valid pointer there, not ",
                "the copy itself"
            ),
            c.label, c.args[1],
        )
    } else {
        source.map_or_else(
            || {
                concat!(
                    "read the calls just before it and the arguments they were given - ",
                    "the value that became this address was answered by one of them"
                )
                .to_owned()
            },
            |c| {
                format!(
                    concat!(
                        "the guest used {}'s answer as a pointer without checking it. If {} ",
                        "should answer a pointer here, it is the gap - implement or fix it; if ",
                        "its answer is correct, the guest reached this path from an earlier ",
                        "wrong value, so read the calls further back"
                    ),
                    c.label, c.label
                )
            },
        )
    }
}

/// The finding for a fault whose instruction is a trap: a kernel entry, or a guest-raised abort.
///
/// A kernel entry is orbistoun's to implement and needs the vector characterised; a `ud2` is the
/// guest aborting on a check it failed, and points upstream. Both keep the fault's evidence
/// (registers, pointees, the calls leading in).
fn kernel_entry_finding(
    trace: &CallTrace,
    f: &crate::trace::FaultSite,
    kind: crate::trace::TrapKind,
) -> Finding {
    use crate::trace::TrapKind;

    let last: Vec<String> = on_this_thread(trace, f.host_thread)
        .into_iter()
        .chain(on_other_threads(trace, f.host_thread))
        .collect();
    let mut evidence = Vec::new();
    if let Some(r) = &f.registers {
        evidence.extend(r.lines());
    }
    evidence.extend(f.pointees.iter().cloned());
    evidence.extend(last);

    let (gap, what, action) = match kind {
        // `int 0x41` from userspace raises a signal on hardware and never returns (selectors 0x0
        // and 0x1 both fault), so it is not a kernel service to add a handler for. A guest reaching
        // it took a path that faults on hardware too, so it routes as a trap with an upstream
        // cause, like `ud2`. Every other vector is unmeasured and may be a service.
        TrapKind::KernelEntry { vector: Some(0x41) } => (
            Gap::Faulted,
            format!(
                concat!(
                    "the guest executed int 0x41 at {} - measured a fatal trap on retail (it ",
                    "raises a signal and does not return), not a kernel service"
                ),
                describe_site(f)
            ),
            concat!(
                "int 0x41 is not orbistoun's to implement: obSCEne measured it fatal on ",
                "hardware too (REQ-...b3c2), so the guest reached it via an upstream wrong ",
                "value. Read the calls just before it - that answer is the gap, not the trap"
            )
            .to_owned(),
        ),
        TrapKind::KernelEntry { vector } => {
            let entry = match vector {
                Some(v) => format!("int {v:#x}"),
                None => "a syscall/trap instruction (syscall, sysenter or hlt)".to_owned(),
            };
            (
                Gap::KernelEntryUnimplemented,
                format!(
                    "the guest entered the kernel via {entry} at {}, which orbistoun does not implement",
                    describe_site(f)
                ),
                format!(
                    concat!(
                        "characterise what {} reads and returns - an obSCEne measurement, since ",
                        "it is below the NID/library layer - then add the handler. A retail ",
                        "title reaching this runs on real hardware, so the wall is orbistoun's, ",
                        "not the guest's"
                    ),
                    entry
                ),
            )
        }
        TrapKind::GuestTrap => (
            Gap::Faulted,
            format!(
                "the guest raised a ud2 trap at {} - it aborted itself on a check it failed",
                describe_site(f)
            ),
            concat!(
                "read the calls just before it: the guest decided to abort, so the cause is ",
                "what it was told, not the trap"
            )
            .to_owned(),
        ),
    };

    Finding {
        gap,
        confidence: Confidence::Certain,
        subject: f.region.clone(),
        what,
        evidence,
        action: Some(action),
        weight: trace.total_calls,
    }
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
/// Decoding a marker address such as `0x5e2700002000` into a field means dividing by a stride; the
/// decoders live in `orbistoun-abi`, where the markers are made (D365). A field marker is what an
/// unestablished handoff slot holds, so faulting on one means the guest used that field. A content
/// marker sits behind such a field, so faulting on one means the guest read through the field and
/// used what it found, which names an offset as well.
fn marker(address: u64) -> Option<String> {
    use orbistoun_abi::enter::{content_slot, sentinel_slot};

    // The firmware skeleton, named before the handoff markers because a guest that reached into it
    // computed the address from a base and an offset, and "firmware plus 0x2885e00" makes that
    // arithmetic legible (see `orbistoun-firmware`).
    if let Some(offset) = orbistoun_firmware::firmware_slot(address) {
        return Some(format!(
            concat!(
                "firmware+{:#x} - the guest reached into the firmware image, which holds a ",
                "zeroed skeleton and not the console's real memory"
            ),
            offset
        ));
    }

    if let Some((field, offset)) = content_slot(address) {
        return Some(if offset == 0 {
            format!("what handoff field {field} points at - the guest read through it")
        } else {
            format!(
                concat!(
                    "handoff field {} plus {:#x} - the guest read through the field and ",
                    "then used a value from that offset"
                ),
                field, offset
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
/// One page: a field read through a null pointer lands within it.
const NEAR_NULL: u64 = 0x1000;

/// The registers that look like the null base of a null-ish fault.
///
/// A fault at or just above zero is a dereference of a zero pointer plus a field offset. Any
/// register at or below the null page, where the fault address is that register plus a field-sized
/// offset, is named. When several registers qualify, all are listed rather than one guessed.
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
    // A null dereference has a base of exactly zero; when any register is zero those are the
    // culprits, and a small nonzero register (often the stored value, matching by coincidence) is
    // dropped.
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

/// The call whose return became the bad pointer the guest dereferenced, if the trace shows one.
///
/// A function's return lands in `rax`, so the value the guest carries to `[rax + offset]` is the
/// return of the immediately preceding call, and only that one. An earlier call that happens to
/// return the same value is not evidence (`__cxa_guard_release` answers zero correctly), so only
/// the last call is matched.
///
/// Returned only when its answer is at or just below the faulting base (zero for a null
/// dereference, the address itself for a wild pointer). Otherwise the null came from further back
/// and the answer is the general search, not a name.
fn pointer_source(
    trace: &CallTrace,
    fault_address: u64,
    host_thread: Option<u64>,
) -> Option<&TracedCall> {
    // The pointer's base: zero for a null-ish fault, the address itself for a wild one. The offset
    // is small either way, and the window below absorbs it.
    let base = if fault_address < NEAR_NULL {
        0
    } else {
        fault_address
    };
    let last = trace
        .tail
        .iter()
        .rev()
        .find(|c| host_thread.is_none_or(|t| c.thread == t))?;
    last.returned
        .is_some_and(|r| base >= r && base - r < NEAR_NULL)
        .then_some(last)
}

/// The call the giving-up code made itself, just before it stopped, and how far back.
///
/// A deliberate stop (`abort`, `exit`, a `ud2`) is usually gated on one call's answer: the guest
/// calls something, tests the result and stops a few instructions later. That call is the closest
/// one below where the stop was decided, on the same thread. The distance is returned with it so
/// the finding shows it rather than asserting a cause: a few dozen bytes reads as the same
/// function, a gigabyte as another module.
///
/// [`None`] when nothing was called from below the stop on its thread.
fn gave_up_gate(trace: &CallTrace) -> Option<(&TracedCall, u64)> {
    let decided = trace.tail.last()?;
    let mut gate: Option<&TracedCall> = None;
    for c in trace.tail.iter().rev().skip(1) {
        // Same thread, and a call site below where it stopped. `rev` visits most recent first and
        // the update is strict, so of two calls from one site the more recent is kept.
        if c.thread != decided.thread || c.from >= decided.from {
            continue;
        }
        if gate.is_none_or(|g| c.from > g.from) {
            gate = Some(c);
        }
    }
    gate.map(|g| (g, decided.from - g.from))
}

/// The guest stopped itself.
fn gave_up(trace: &CallTrace) -> Option<Finding> {
    let stopped = trace.stopped.as_ref()?;
    // What it called immediately before deciding, the closest thing to a reason the guest offers.
    let last: Vec<String> = trace.tail.iter().rev().take(4).map(traced_line).collect();
    // The one call the giving-up code made itself, singled out so it is not read as equal to the
    // far calls behind it.
    let gate = gave_up_gate(trace);
    Some(Finding {
        gap: Gap::GuestGaveUp,
        confidence: Confidence::Certain,
        subject: gate.map(|(g, _)| g.label.clone()),
        what: format!("{stopped} - it decided to stop rather than failing"),
        evidence: {
            let mut e = vec![format!("after {} calls", trace.total_calls)];
            if let Some((g, delta)) = gate {
                e.push(format!(
                    "its own last call before stopping: {} ({delta:#x} before it stopped)",
                    traced_line(g)
                ));
            }
            e.extend(last.into_iter().map(|c| format!("{PRECEDED_BY}{c}")));
            e
        },
        action: Some(match gate {
            Some(_) => concat!(
                "read that call first - the closest one below where it stopped is the one the ",
                "giving-up code made itself, and a deliberate stop is usually gated on its answer. ",
                "Calls much further back are from other modules, not this decision"
            )
            .to_owned(),
            None => concat!(
                "read the calls immediately before it - a guest that gives up usually reports ",
                "why first, and that call is the gap"
            )
            .to_owned(),
        }),
        weight: trace.total_calls,
    })
}

/// One traced call as an evidence line: what was called, its first argument, what it answered where
/// that is known, and the call site.
///
/// The return is shown only when recorded: the faulting call's own frame, and any call still
/// running, has none (D459).
fn traced_line(c: &TracedCall) -> String {
    let operands = copy_operands(c);
    match c.returned {
        Some(ret) => format!(
            "{}({:#x}){operands} -> {ret:#x} from {:#x}",
            c.label, c.args[0], c.from
        ),
        None => format!("{}({:#x}){operands} from {:#x}", c.label, c.args[0], c.from),
    }
}

/// The source and length a byte-copy call carries in `arg1`/`arg2`.
///
/// A copy from a null or near-null source is the common shape of a producer that left a buffer
/// pointer zero. `arg0` is the destination, valid by the time the copy runs; the fault is in
/// `arg1`, so the operands are shown to make a null source legible without a second run. Values
/// only, never a cause.
fn copy_operands(c: &TracedCall) -> String {
    let label = c.label.as_str();
    if is_source_copy(label) {
        // arg1 = source (rsi), arg2 = length (rdx), in System V order (`ARGUMENT_REGISTERS`).
        format!(" src {:#x} n {:#x}", c.args[1], c.args[2])
    } else if label.contains("memset") {
        // arg2 = length (rdx); arg1 is the fill byte, not a pointer, so it is not shown as one.
        format!(" n {:#x}", c.args[2])
    } else {
        String::new()
    }
}

/// Whether a call is a byte copy that reads through a source pointer in `arg1`.
///
/// `memcpy`/`memmove` do; `memset` does not, since its `arg1` is a fill byte. One predicate, so the
/// operands line and the faulting-copy finder agree.
fn is_source_copy(label: &str) -> bool {
    label.contains("memcpy") || label.contains("memmove")
}

/// The byte copy whose source range covers the faulting address: the copy that faulted reading its
/// own source.
///
/// The source pointer was captured at the call in `arg1`; the mid-copy register dump is not a
/// reliable source, since there `rdx` is the remaining count. Matching the recorded range `[src,
/// src+n)` against the fault names which call read there, not why the pointer was wrong. Most
/// recent first on the faulting thread, so the copy still running (recorded with no return) is the
/// one found.
fn copy_reading_fault(
    trace: &CallTrace,
    fault_address: u64,
    host_thread: Option<u64>,
) -> Option<&TracedCall> {
    trace.tail.iter().rev().find(|c| {
        host_thread.is_none_or(|t| c.thread == t)
            && is_source_copy(&c.label)
            // `src <= fault` then `fault - src < n`: the fault lies in this copy's source span,
            // written to avoid the overflow `src + n` risks near the top of the range.
            && c.args[1] <= fault_address
            && fault_address - c.args[1] < c.args[2]
    })
}

/// The lead evidence line for a copy that faulted reading its source, or nothing when none did.
///
/// Beside the finder it reads from: where in the source the read landed, and whether the base is in
/// the null page. Values and arithmetic only; the cause is the finding's action.
fn copy_source_lead(copy: Option<&TracedCall>, fault_address: u64) -> Option<String> {
    let c = copy?;
    let (src, n) = (c.args[1], c.args[2]);
    let base = if src < NEAR_NULL {
        format!(", whose base is in the null page (0x0 + {src:#x})")
    } else {
        String::new()
    };
    Some(format!(
        concat!(
            ">> {label} faulted reading its source: given src {src:#x} n {n:#x}{base}; the ",
            "faulting address is byte {byte:#x} into that source",
        ),
        label = c.label,
        src = src,
        n = n,
        base = base,
        byte = fault_address - src,
    ))
}

/// The last few calls made on the thread a fault happened on.
///
/// With no thread recorded, the last few calls of the run.
fn on_this_thread(trace: &CallTrace, faulted_on: Option<u64>) -> Vec<String> {
    trace
        .tail
        .iter()
        .rev()
        .filter(|c| faulted_on.is_none_or(|t| c.thread == t))
        .take(4)
        .map(|c| format!("{PRECEDED_BY}{}", traced_line(c)))
        .collect()
}

/// A few calls from other threads, labelled as another thread's.
///
/// Empty when no thread was recorded, since there is nothing to contrast with.
fn on_other_threads(trace: &CallTrace, faulted_on: Option<u64>) -> Vec<String> {
    trace
        .tail
        .iter()
        .rev()
        .filter(|c| faulted_on.is_some_and(|t| c.thread != t))
        .take(2)
        .map(|c| format!("{PRECEDED_BY}{}  [another thread]", traced_line(c)))
        .collect()
}
/// The last few calls a run made, as evidence lines.
///
/// Shared by the findings whose action tells a reader to look at them, so the list a person is sent
/// to and the list a dispatcher sweeps are the same list.
fn preceding(trace: &CallTrace) -> Vec<String> {
    // The faulting thread's calls first, then the rest labelled as other threads' (D621).
    let faulted_on = trace.fault.as_ref().and_then(|f| f.host_thread);
    let mine: Vec<&TracedCall> = trace
        .tail
        .iter()
        .rev()
        .filter(|c| faulted_on.is_none_or(|t| c.thread == t))
        .take(4)
        .collect();
    let others: Vec<&TracedCall> = trace
        .tail
        .iter()
        .rev()
        .filter(|c| faulted_on.is_some_and(|t| c.thread != t))
        .take(2)
        .collect();
    mine.into_iter()
        .map(|c| format!("{PRECEDED_BY}{}", traced_line(c)))
        .chain(
            others
                .into_iter()
                .map(|c| format!("{PRECEDED_BY}{}  [another thread]", traced_line(c))),
        )
        .collect()
}

/// A placeholder code being used by the guest as a pointer.
fn error_used_as_pointer(trace: &CallTrace) -> Vec<Finding> {
    let mut out = Vec::new();

    // The guest passing one of our codes into a later call. Whatever answered it is the function to
    // fix, and the call that received it names the moment. Every argument is checked, since a
    // placeholder handed on as a size can arrive in any register.
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
                // The calls this finding's action points at, carried with it so the reader is not
                // sent searching.
                let mut e = vec![
                    format!("call #{} from {:#x}", call.sequence, call.from),
                    "the guest is treating an unimplemented answer as data".to_owned(),
                ];
                e.extend(preceding(trace));
                e
            },
            action: Some(match source_of(trace, value) {
                // With a tag there is nothing to look for: the value names its source.
                Some(source) => format!(
                    concat!(
                        "give {} a real return - a function whose answer is read as data ",
                        "must never answer an error code (D125)"
                    ),
                    source
                ),
                None => concat!(
                    "find what answered with that code just before, and give it a real ",
                    "return - a pointer-returning function must never answer an error ",
                    "code (D125). Re-run under ORBISTOUN_TAG_PLACEHOLDERS to be told ",
                    "which (D567)"
                )
                .to_owned(),
            }),
            weight: 1,
        });
    }

    // The same code arriving as a faulting address: the guest dereferenced it.
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
                    concat!(
                        "the function that returned it must answer a real value; if it returns ",
                        "a pointer or handle, it needs an implementation rather than a policy ",
                        "change"
                    )
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
            concat!(
                "the answer this returns is being rejected. Vary it and watch whether the call ",
                "pattern changes - the shape of the loop says more than the return code"
            )
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
            concat!(
                "this is almost never the guest's fault - check how it is entered. A remainder ",
                "of 0 means control arrived by a jump where a call was expected"
            )
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
            concat!(
                "a guest that receives a truncated asset faults inside its own parser, far ",
                "from here - check the length arithmetic before looking anywhere else"
            )
            .to_owned(),
        ),
        weight: trace.reads.short,
    })
}

/// The first command buffer the guest handed to the graphics driver.
///
/// Progress rather than a wall: this says what the command stream holds so the next work is ranked.
/// `None` for a run that did not reach a submission.
fn submitted(trace: &CallTrace) -> Option<Finding> {
    let submission = trace.submission.as_ref()?;
    let mut evidence = vec![
        format!(
            "{} register writes extracted from the stream",
            submission.register_writes
        ),
        format!(
            "{} of the addresses named resolved to a region the guest was given, {} did not (D101)",
            submission.addresses_resolved, submission.addresses_unresolved
        ),
        format!(
            "{} of {} shader candidates translated to a module a backend can bind",
            submission.shaders_translated, submission.shaders_found
        ),
    ];
    // Why each one did not: the reason a draw reaches the backend with nothing bound.
    evidence.extend(
        submission
            .shader_failures
            .iter()
            .map(|failure| format!("did not translate: {failure}")),
    );
    Some(Finding {
        gap: Gap::Submitted,
        confidence: Confidence::Certain,
        subject: Some("sceAgcDriverSubmitDcb".to_owned()),
        what: format!(
            "the guest submitted a command buffer: {} packets, {} draws, {} shader candidates",
            submission.packets, submission.draws, submission.shaders_found
        ),
        evidence,
        action: Some(
            concat!(
                "the first real graphics work: translate the shaders its registers name, then ",
                "attach a backend to the run path so the stream can be rendered"
            )
            .to_owned(),
        ),
        weight: u64::try_from(submission.packets).unwrap_or(u64::MAX),
    })
}

/// Named functions the guest used that nothing implements.
///
/// The most directly actionable category: it names a function and how much the guest leaned on it.
/// Unnamed hashes are excluded; they are reported as a naming gap.
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
            evidence: {
                let mut evidence =
                    vec!["the call landed on a stub, which answered a placeholder".to_owned()];
                // The signature the guest's own calls imply, so the finding says what to implement.
                // Empty for a function whose arguments were never sampled, and then not claimed.
                if !c.shape.is_empty() {
                    evidence.push(format!("the guest called it as {}", c.shape));
                }
                // The pointers it was handed belong to the finding, so a programmatic reader such
                // as the dispatcher sees that the call was given a structure.
                evidence.extend(pointer_arguments(trace, &c.label));
                evidence
            },
            action: Some(format!(
                "implement it in the crate declaring {}; record what it returns in its knowledge file first, because a function answering a pointer must never answer an error code",
                c.label.split("::").next().unwrap_or("that library")
            )),
            weight: c.calls,
        })
        .collect()
}

/// Every argument of `label` that pointed somewhere the run could read.
///
/// Only arguments with bytes: the dump reads a value only from a span published as readable, so a
/// scalar such as a size, flag or count has none. Rendered as `value -> region+offset`, the shape a
/// reader recognises and `orbistoun-turn` parses an address out of.
fn pointer_arguments(trace: &CallTrace, label: &str) -> Vec<String> {
    trace
        .dumps
        .iter()
        .filter(|d| d.label == label && !d.bytes.is_empty())
        .map(|d| format!("arg{} = {:#x} -> {} = {}", d.slot, d.value, d.at, d.bytes))
        .collect()
}

/// Arguments captured for imports nothing else in this report speaks for.
///
/// Dumps otherwise reach a reader only through [`pointer_arguments`], consulted by findings about
/// unimplemented imports, so a dump forced on an implemented import would print nothing. `already`
/// is every subject another finding covers, so an unimplemented import's dump is not repeated here.
fn captured(trace: &CallTrace, already: &[Finding]) -> Vec<Finding> {
    // Only imports somebody named. A dump is also taken for every unimplemented import, and a
    // function answering in `xmm0` has no integer handler yet is not unimplemented, so without this
    // it would be reported unasked.
    let mut labels: Vec<&str> = trace
        .dumps
        .iter()
        .filter(|d| !d.bytes.is_empty())
        .map(|d| d.label.as_str())
        .filter(|label| trace.forced_dumps.iter().any(|f| f == label))
        .collect();
    labels.sort_unstable();
    labels.dedup();
    labels
        .into_iter()
        .filter(|label| !already.iter().any(|f| f.subject.as_deref() == Some(*label)))
        .map(|label| {
            // No evidence lines of its own: the printer already renders every dump whose label
            // appears in `what`, and repeating them would read as two calls.
            let calls = trace.dumps.iter().filter(|d| d.label == label).count();
            Finding {
                gap: Gap::Captured,
                // A recording, not an inference: these are the bytes that were there.
                confidence: Confidence::Certain,
                subject: Some(label.to_owned()),
                what: format!("{label} was asked about, and here is what it was passed"),
                evidence: vec![format!("{calls} captured argument value(s), listed below")],
                action: Some(
                    concat!(
                        "compare each against what the implementation expects - a dump is ",
                        "evidence about the caller, not a verdict on the callee"
                    )
                    .to_owned(),
                ),
                // Below every gap: an answer to a question somebody asked never outranks a fault.
                weight: 0,
            }
        })
        .collect()
}

/// Imports still known only by hash.
///
/// The advice depends on who wrote the symbol. A vendor library's unnamed hash is a vocabulary gap
/// and the name search answers it. A hash from a module the title ships is the title's own symbol,
/// which no vendor word list holds, so the advice differs (D640).
fn unnamed(trace: &CallTrace) -> Vec<Finding> {
    trace
        .calls
        .iter()
        .filter(|c| c.label.contains("::0x"))
        .map(|c| {
            let library = c.label.split("::").next().unwrap_or_default();
            let shipped = trace.title_modules.iter().any(|m| m == library);
            Finding {
                gap: Gap::Unnamed,
                confidence: Confidence::Certain,
                subject: Some(c.label.clone()),
                what: if c.implemented {
                    format!(
                        "{} was called {} times and has no name, though a handler answers it by NID",
                        c.label, c.calls
                    )
                } else {
                    format!("{} was called {} times and has no name", c.label, c.calls)
                },
                evidence: {
                    let mut evidence = vec![if shipped {
                        format!("{library} is a module this title ships, so the hash is the game's own symbol")
                    } else {
                        "the hash resolved to no name in the symbol database".to_owned()
                    }];
                    // The inferred signature narrows a bare hash: a name candidate whose arity
                    // disagrees with how the guest called it is wrong before the hash is computed.
                    if !c.shape.is_empty() {
                        evidence.push(format!("the guest called it as {}", c.shape));
                    }
                    evidence
                },
                // Names the commands, so the advice is an action. `suggest` is mentioned rather
                // than run: it is slow and optional, and nothing on this path waits on a model.
                action: Some(if c.implemented {
                    concat!(
                        "nothing here blocks reach - orbistoun answers this by its NID and a ",
                        "handler runs, so the missing name is documentation, not a gap, and the ",
                        "vocabulary search is the wrong tool for it. A hash that never resolves to ",
                        "a name is a title-private symbol or a vendor alias no derivation reaches, ",
                        "and its knowledge entry is where it is recorded instead of the name table"
                    )
                    .to_owned()
                } else if shipped {
                    concat!(
                        "do not search a vendor vocabulary for this - it is the title's own ",
                        "code, and the module exporting it is already placed and started, so ",
                        "what the guest wants is its export rather than a name"
                    )
                    .to_owned()
                } else {
                    concat!(
                        "extend the candidate vocabulary and re-run the name search - a name ",
                        "is confirmed by the hash agreeing, never by consulting a table. ",
                        "`./bin/orbistoun names` re-runs it; `./bin/orbistoun suggest` asks a ",
                        "local model for words first, for when the vocabulary is what is short"
                    )
                    .to_owned()
                }),
                weight: c.calls,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        Confidence, Gap, findings, looks_like_placeholder, marker, null_base_registers, source_of,
        tagged_stub, traced_line,
    };
    use crate::trace::{
        AbiReport, ArgumentDump, CallTrace, CalledImport, Conditions, FaultSite, FormatReport,
        ReadReport, Registers, SubmissionSummary, TracedCall,
    };
    use orbistoun_core::GuestError;

    /// The untagged placeholder every unimplemented call answers (`0xF7FF_0001`), derived from the
    /// core so the tests follow it.
    fn placeholder_code() -> u64 {
        u64::from(GuestError::Unimplemented.as_raw())
    }

    /// Registers with every field a distinct value well above the null page, so a test zeroes
    /// exactly the ones it means to and no unset field matches the null-base check.
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

    /// A submitted command buffer is a finding, naming its packet, draw and shader-candidate
    /// counts, and a run that never reached one produces none.
    #[test]
    fn a_submitted_command_buffer_is_a_finding_and_an_ordinary_run_is_not() {
        // No submission: no finding.
        assert!(
            !findings(&empty()).iter().any(|f| f.gap == Gap::Submitted),
            "a run that reached no submission produces no Submitted finding"
        );

        let mut trace = empty();
        trace.submission = Some(SubmissionSummary {
            packets: 40,
            register_writes: 31,
            draws: 2,
            shaders_found: 3,
            addresses_resolved: 2,
            addresses_unresolved: 1,
            shaders_translated: 1,
            shader_failures: vec![
                "pixel at 0x1000: an opcode the translator has no rule for".to_owned(),
            ],
        });
        let found = findings(&trace);
        let submission = found
            .iter()
            .find(|f| f.gap == Gap::Submitted)
            .expect("a submission produces a finding");
        assert_eq!(submission.subject.as_deref(), Some("sceAgcDriverSubmitDcb"));
        // Why a draw has nothing bound is named, not left as a count.
        assert!(
            submission
                .evidence
                .iter()
                .any(|e| e.contains("1 of 3 shader candidates translated")),
            "{:?}",
            submission.evidence
        );
        assert!(
            submission.evidence.iter().any(|e| e
                == "did not translate: pixel at 0x1000: an opcode the translator has no rule for"),
            "{:?}",
            submission.evidence
        );
        assert!(
            submission.what.contains("40 packets")
                && submission.what.contains("2 draws")
                && submission.what.contains("3 shader candidates"),
            "the finding names the counts: {}",
            submission.what
        );
        assert_eq!(submission.weight, 40, "ranked by packet count");
    }

    /// The register that was the null pointer is named, and a register whose value merely matches
    /// the address arithmetic is not.
    #[test]
    fn the_null_base_register_is_named_and_a_coincidence_is_not() {
        // `mov [r12+0x10], r14d` with r12 zero: the fault is at 0x10, r12 is the base, and r14
        // holds the stored value 0x10, which matches the address with a zero offset by chance.
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

    /// A fault far from zero names nothing: the check is for null bases.
    #[test]
    fn a_fault_that_is_not_null_ish_names_no_base() {
        let mut regs = well_placed_registers();
        regs.rax = 0;
        assert!(
            null_base_registers(0x4000_0000, &regs).is_empty(),
            "0x0 is not within a field's reach of 0x40000000"
        );
    }

    /// A marker-block fault address is decoded into the field it names (D365).
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
            forced_dumps: Vec::new(),
            ended_by: None,
            threads: Vec::new(),
            said: Vec::new(),
            quiet: None,
            title_modules: Vec::new(),
            module: "m".to_owned(),
            reached: "Entered".to_owned(),
            total_calls: 0,
            distinct: 0,
            frames: 0,
            frame_written: false,
            submission: None,
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
            thread: 0,
            sequence: 1,
            label: label.to_owned(),
            args: [arg0, 0, 0, 0, 0, 0],
            from: 0x1000,
            returned: None,
        }
    }

    /// A byte copy's line shows its source and length, and an ordinary call's line does not.
    #[test]
    fn a_byte_copy_line_shows_its_source_and_length_so_a_copy_from_null_is_legible() {
        let mut memcpy = call("libc::memcpy", 0x7400_0218_f070);
        memcpy.args[1] = 0x0; // the null source
        memcpy.args[2] = 0xa8; // the length
        let line = traced_line(&memcpy);
        assert!(
            line.contains("src 0x0") && line.contains("n 0xa8"),
            "a memcpy line must show its null source and length: {line}"
        );

        // An ordinary call keeps the short form: operands appear only where they are evidence.
        let ordinary = call("libSceAgc::sceAgcDcbDrawIndexAuto", 0x7400_0218_7868);
        assert!(
            !traced_line(&ordinary).contains(" src "),
            "a non-copy call must not grow copy operands: {}",
            traced_line(&ordinary)
        );

        // memset carries a fill byte in arg1, not a pointer, so it shows the length and no source.
        let mut memset = call("libc::memset", 0x6000_007f_bfe4);
        memset.args[2] = 0x20;
        let set = traced_line(&memset);
        assert!(
            set.contains("n 0x20") && !set.contains(" src "),
            "memset shows its length but not a source pointer: {set}"
        );
    }

    /// A captured-arguments finding appears only for an import somebody named, and does appear for
    /// one.
    #[test]
    fn arguments_are_reported_only_for_imports_somebody_named() {
        let dump = |label: &str| ArgumentDump {
            label: label.to_owned(),
            slot: 0,
            at: "stack+0x10".to_owned(),
            value: 0x6000_0000_0010,
            bytes: "00 01 02 03".to_owned(),
            text: String::new(),
        };

        let mut unasked = empty();
        unasked.dumps = vec![dump("libc::acos")];
        assert!(
            !findings(&unasked).iter().any(|f| f.gap == Gap::Captured),
            "a dump taken by the default rule is not an answer to anybody"
        );

        let mut asked = empty();
        asked.dumps = vec![dump("libc::acos")];
        asked.forced_dumps = vec!["libc::acos".to_owned()];
        assert!(
            findings(&asked)
                .iter()
                .any(|f| f.gap == Gap::Captured && f.subject.as_deref() == Some("libc::acos")),
            "and an import somebody named must still be answered, or the finding is inert"
        );
    }

    /// A hash from a module the title ships gets different advice from a vendor library's hash, and
    /// each branch keeps its own.
    #[test]
    fn an_unnamed_hash_from_the_titles_own_module_is_not_sent_to_a_vendor_word_list() {
        let mut trace = empty();
        trace.calls = vec![
            CalledImport {
                index: 0,
                label: "PS5Util::0xf948d02a4f9f5ace".to_owned(),
                calls: 19_689_015,
                implemented: false,
                shape: String::new(),
            },
            CalledImport {
                index: 1,
                label: "libSceAgc::0x53bbd82b51d172db".to_owned(),
                calls: 1,
                implemented: false,
                shape: String::new(),
            },
            // An unnamed hash orbistoun already answers by NID: not a reach gap, so not sent to the
            // search.
            CalledImport {
                index: 2,
                label: "libSceAgc::0x7d86501b8094ef57".to_owned(),
                calls: 2,
                implemented: true,
                shape: String::new(),
            },
        ];
        trace.title_modules = vec!["PS5Util".to_owned()];

        let found = findings(&trace);
        let shipped = found
            .iter()
            .find(|f| f.subject.as_deref() == Some("PS5Util::0xf948d02a4f9f5ace"))
            .expect("the busiest unnamed import produces a finding");
        assert!(
            shipped
                .action
                .as_deref()
                .is_some_and(|a| a.contains("the title's own code")),
            "a symbol the game exports is not a gap in anybody's vocabulary"
        );

        let vendor = found
            .iter()
            .find(|f| f.subject.as_deref() == Some("libSceAgc::0x53bbd82b51d172db"))
            .expect("a vendor hash produces a finding too");
        assert!(
            vendor
                .action
                .as_deref()
                .is_some_and(|a| a.contains("extend the candidate vocabulary")),
            "and a vendor library's unnamed hash still gets the search that can find it"
        );

        // The implemented one is answered by NID, so it is not told to search a vocabulary.
        let handled = found
            .iter()
            .find(|f| f.subject.as_deref() == Some("libSceAgc::0x7d86501b8094ef57"))
            .expect("an implemented unnamed hash still produces a finding");
        assert!(
            handled
                .action
                .as_deref()
                .is_some_and(|a| !a.contains("extend the candidate vocabulary")
                    && a.contains("answers this by its NID")),
            "a call orbistoun already answers is not a vocabulary gap"
        );
    }

    #[test]
    fn a_called_function_nothing_implements_is_the_clearest_instruction_there_is() {
        // An unimplemented named function is reported with how much the guest leaned on it.
        let mut trace = empty();
        trace.total_calls = 40;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libSceAgc::sceAgcCreateShader".to_owned(),
            calls: 40,
            implemented: false,
            shape: String::new(),
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

    /// The inferred signature reaches the unimplemented finding, and an import with no shape gets
    /// none.
    #[test]
    fn an_inferred_signature_is_carried_into_the_unimplemented_finding() {
        let mut trace = empty();
        trace.total_calls = 40;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libSceAgc::sceAgcCreateShader".to_owned(),
            calls: 40,
            implemented: false,
            shape: "(ptr, u32, ptr?)".to_owned(),
        }];
        let found = findings(&trace);
        assert_eq!(found[0].gap, Gap::Unimplemented);
        assert!(
            found[0]
                .evidence
                .iter()
                .any(|e| e.contains("(ptr, u32, ptr?)")),
            "the signature the guest implied must be evidence on the finding: {:?}",
            found[0].evidence
        );

        // No shape, no claim.
        trace.calls[0].shape.clear();
        let bare = findings(&trace);
        assert!(
            !bare[0].evidence.iter().any(|e| e.contains("called it as")),
            "an unsampled import must not be given an invented signature: {:?}",
            bare[0].evidence
        );
    }

    #[test]
    fn an_unnamed_hash_is_a_naming_gap_and_not_an_implementation_one() {
        // An unnamed hash must be named before it can be implemented.
        let mut trace = empty();
        trace.total_calls = 3;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libkernel::0xcedb06001fd4c617".to_owned(),
            calls: 3,
            implemented: false,
            shape: String::new(),
        }];
        let kinds: Vec<Gap> = findings(&trace).iter().map(|f| f.gap).collect();
        assert_eq!(kinds, vec![Gap::Unnamed]);
    }

    #[test]
    fn a_clean_run_produces_nothing_to_do() {
        // Nothing wrong means no findings, or the ranking stops meaning anything.
        assert!(findings(&empty()).is_empty());
    }

    #[test]
    fn a_placeholder_passed_as_an_argument_is_reported_with_certainty() {
        // It names the call that received a bad answer and shows the guest believed it.
        let mut trace = empty();
        trace.tail = vec![call(
            "libSceVideoOut::sceVideoOutRegisterBuffers2",
            placeholder_code(),
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

    /// The negative placeholder is recognised in `rdx` and as a faulting address (D670).
    #[test]
    fn the_post_d670_placeholder_is_recognised_in_a_register_and_as_a_fault() {
        let code = placeholder_code();

        // Handed on in rdx (argument index 2), not rdi.
        let mut passed = empty();
        let mut handed = call("libSceGnmDriver::sceGnmSubmitCommandBuffers", 0);
        handed.args[2] = code;
        passed.tail = vec![handed];
        assert!(
            findings(&passed)
                .iter()
                .any(|f| f.gap == Gap::ErrorUsedAsPointer),
            "a placeholder handed on in rdx is a placeholder used as a pointer"
        );

        // Dereferenced: the run died reading the code as an address.
        let mut faulted = empty();
        faulted.fault = Some(FaultSite {
            instruction: Vec::new(),
            thread: None,
            host_thread: None,
            pointees: Vec::new(),
            kind: "read of".to_owned(),
            address: code,
            instruction_pointer: 0x1234,
            region: None,
            offset: None,
            inside_import: None,
            registers: None,
            frames: Vec::new(),
        });
        assert!(
            findings(&faulted)
                .iter()
                .any(|f| f.gap == Gap::ErrorUsedAsPointer),
            "a fault at the placeholder code is the guest dereferencing our refusal"
        );
    }

    #[test]
    fn a_placeholder_is_recognised_at_an_offset_from_itself() {
        // A guest treating a code as a struct pointer reads a field through it, so the faulting
        // address is the code plus or minus a little.
        assert!(looks_like_placeholder(placeholder_code()));
        assert!(looks_like_placeholder(0xF7FF_0019), "code plus 0x18");
        assert!(looks_like_placeholder(0xF7FE_FFF9), "code minus 8");
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
            shape: String::new(),
        }];
        let found = findings(&trace);
        assert_eq!(found[0].gap, Gap::Spinning);
    }

    #[test]
    fn a_short_run_dominated_by_one_call_is_not_a_spin() {
        // A guest that calls `memset` four times out of five during startup is busy, not stuck.
        let mut trace = empty();
        trace.total_calls = 5;
        trace.calls = vec![CalledImport {
            index: 0,
            label: "libc::memset".to_owned(),
            calls: 4,
            implemented: true,
            shape: String::new(),
        }];
        assert!(findings(&trace).is_empty());
    }

    #[test]
    fn giving_up_outranks_everything_else_and_carries_its_last_calls() {
        // A guest that stopped deliberately said the most useful thing in the calls immediately
        // before.
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
    fn the_gate_named_is_the_giving_up_codes_own_last_call_not_a_far_one() {
        // The guest calls a mapper in its executable (0x4800...), tests the answer and aborts 0x4d
        // bytes later. Its abort path opens an error dialog in a different module (0x4000...), the
        // most recent call before the abort. The gate is the call closest below where it stopped,
        // the near mapper, not the most recent call.
        let near_mapper = TracedCall {
            thread: 7,
            sequence: 1,
            label: "libkernel::sceKernelMapperGetParam".to_owned(),
            args: [0x6000_0080_0e20, 0, 0, 0, 0, 0],
            from: 0x4800_00a1_c760,
            returned: Some(0x8002_0006),
        };
        let error_dialog = TracedCall {
            thread: 7,
            sequence: 2,
            label: "libSceErrorDialog::sceErrorDialogInitialize".to_owned(),
            args: [0x81, 0, 0, 0, 0, 0],
            from: 0x4000_0012_741e,
            returned: Some(0x0),
        };
        let abort = TracedCall {
            thread: 7,
            sequence: 3,
            label: "libc::abort".to_owned(),
            args: [0xbe9c, 0, 0, 0, 0, 0],
            from: 0x4800_00a1_c7ad,
            returned: None,
        };
        let mut trace = empty();
        trace.total_calls = 391;
        trace.stopped = Some("the guest called abort".to_owned());
        // Order: gate, then the far dialog the abort path made, then abort. Most recent picks the
        // dialog; closest below picks the mapper.
        trace.tail = vec![near_mapper, error_dialog, abort];

        let found = findings(&trace);
        assert_eq!(found[0].gap, Gap::GuestGaveUp);
        let gate_line = found[0]
            .evidence
            .iter()
            .find(|e| e.contains("before it stopped"))
            .expect("the gate line is present");
        assert!(
            gate_line.contains("sceKernelMapperGetParam"),
            "the near mapper is named the gate, not the more recent cross-module call: {gate_line}"
        );
        assert!(
            gate_line.contains("0x4d"),
            "its distance back is the measured 0x4d: {gate_line}"
        );
        assert!(
            !gate_line.contains("sceErrorDialogInitialize"),
            "the far call the abort path made on its way down is not named the gate: {gate_line}"
        );
        assert_eq!(
            found[0].subject.as_deref(),
            Some("libkernel::sceKernelMapperGetParam"),
            "the gate is the finding's subject, so a consumer routes to it without parsing prose"
        );
    }

    #[test]
    fn a_give_up_with_nothing_called_below_it_names_no_gate() {
        // The near-call rule must not invent a gate: a stop whose only preceding call is on another
        // thread falls back to the plain finding rather than naming an unrelated call.
        let other_thread = TracedCall {
            thread: 9,
            sequence: 1,
            label: "libkernel::sceKernelUsleep".to_owned(),
            args: [0, 0, 0, 0, 0, 0],
            from: 0x4800_00a1_c700,
            returned: Some(0),
        };
        let abort = TracedCall {
            thread: 7,
            sequence: 2,
            label: "libc::abort".to_owned(),
            args: [0xbe9c, 0, 0, 0, 0, 0],
            from: 0x4800_00a1_c7ad,
            returned: None,
        };
        let mut trace = empty();
        trace.total_calls = 10;
        trace.stopped = Some("the guest called abort".to_owned());
        trace.tail = vec![other_thread, abort];

        let found = findings(&trace);
        assert_eq!(found[0].gap, Gap::GuestGaveUp);
        assert!(
            !found[0]
                .evidence
                .iter()
                .any(|e| e.contains("before it stopped")),
            "a call on another thread is not the giving-up code's own, so no gate is named"
        );
        assert!(
            found[0]
                .action
                .as_deref()
                .is_some_and(|a| a.contains("usually reports")),
            "with no gate it falls back to the original advice"
        );
    }

    #[test]
    fn findings_are_ranked_by_confidence_before_weight() {
        // A heavy guess must not outrank a light certainty.
        let mut trace = empty();
        trace.reads = ReadReport {
            reads: 100,
            short: 90,
            bytes: 10,
        };
        trace.tail = vec![call("libc::something", placeholder_code())];
        let found = findings(&trace);
        assert_eq!(found[0].confidence, Confidence::Certain);
        assert_eq!(found[0].gap, Gap::ErrorUsedAsPointer);
        assert_eq!(found.last().expect("two findings").gap, Gap::ShortRead);
    }

    #[test]
    fn every_finding_says_where_to_look() {
        // A consumer can route a finding by its classification without reading the prose.
        let mut trace = empty();
        trace.fault = Some(FaultSite {
            instruction: Vec::new(),
            thread: None,
            host_thread: None,
            pointees: Vec::new(),
            kind: "read of".to_owned(),
            address: placeholder_code(),
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

    /// A trap-instruction fault is classified in the ranked finding, not only in the crash print.
    ///
    /// `int 0x41` is fatal on hardware, so it is a [`Gap::Faulted`] routed to the call before it;
    /// any other unmeasured vector is a [`Gap::KernelEntryUnimplemented`] to characterise and then
    /// handle.
    #[test]
    fn int_0x41_is_a_measured_fatal_trap_and_an_unmeasured_vector_still_awaits_a_handler() {
        let int_41 = |instruction: Vec<u8>| {
            let mut trace = empty();
            trace.total_calls = 100;
            trace.fault = Some(FaultSite {
                instruction,
                thread: None,
                host_thread: None,
                pointees: Vec::new(),
                kind: "read of".to_owned(),
                address: u64::MAX, // the general-protection read a trap raises
                instruction_pointer: 0x4000_0019_6b91_u64,
                region: Some("image".to_owned()),
                offset: Some(0x0196_b91a),
                inside_import: None,
                registers: None,
                frames: Vec::new(),
            });
            findings(&trace)
                .into_iter()
                .find(|f| matches!(f.gap, Gap::KernelEntryUnimplemented | Gap::Faulted))
                .expect("a fault finding")
        };

        // int 0x41: fatal on hardware, so a guest trap pointing upstream.
        let fatal = int_41(vec![0xcd, 0x41]);
        assert_eq!(
            fatal.gap,
            Gap::Faulted,
            "int 0x41 is measured fatal on hardware, so it is a guest trap, not an entry to add: {}",
            fatal.what
        );
        assert!(
            fatal.what.contains("int 0x41") && fatal.what.contains("fatal"),
            "it must name the vector and say it is measured fatal, not awaiting a handler: {}",
            fatal.what
        );
        let fatal_action = fatal.action.as_deref().unwrap_or("");
        assert!(
            !fatal_action.contains("characterise") && fatal_action.contains("upstream"),
            concat!(
                "the measurement is in: the action points upstream, not at another ",
                "measurement: {}"
            ),
            fatal_action
        );

        // int 0x42: unmeasured, so still a kernel entry to characterise, then handle.
        let unmeasured = int_41(vec![0xcd, 0x42]);
        assert_eq!(
            unmeasured.gap,
            Gap::KernelEntryUnimplemented,
            "an unmeasured vector is still an entry awaiting a handler: {}",
            unmeasured.what
        );
        assert!(
            unmeasured
                .action
                .as_deref()
                .is_some_and(|a| a.contains("characterise")),
            "an unmeasured vector still routes to a device measurement"
        );

        // An ordinary instruction at the same address is still a generic fault, not a kernel entry.
        let ordinary = int_41(vec![0x48, 0x8b, 0x00]); // mov rax, [rax]
        assert_eq!(
            ordinary.gap,
            Gap::Faulted,
            "a mov through a null pointer is a bad pointer, not a kernel entry"
        );
        assert!(
            !ordinary.what.contains("int 0x41"),
            "a plain fault must not borrow the trap's wording: {}",
            ordinary.what
        );
    }

    /// A null dereference routes itself to the call that answered zero, and falls back to the
    /// general search when nothing in the trace answered the base.
    #[test]
    fn a_null_dereference_names_the_call_that_answered_zero() {
        let mut trace = empty();
        trace.total_calls = 100;
        // A call that answered zero, then a field read through that zero at +0x38.
        let mut supplier = call("libkernel::sceKernelGetDirectMemoryType", 0);
        supplier.returned = Some(0);
        trace.tail = vec![supplier];
        trace.fault = Some(FaultSite {
            instruction: vec![0x48, 0x8b, 0x40, 0x38], // mov rax, [rax+0x38]
            thread: Some(1),
            host_thread: Some(0),
            pointees: Vec::new(),
            kind: "read of".to_owned(),
            address: 0x38,
            instruction_pointer: 0x4000_0000_1234,
            region: Some("image".to_owned()),
            offset: Some(0x1234),
            inside_import: None,
            registers: None,
            frames: Vec::new(),
        });

        let f = findings(&trace)
            .into_iter()
            .find(|f| f.gap == Gap::Faulted)
            .expect("a fault finding");
        assert!(
            f.action
                .as_deref()
                .unwrap_or("")
                .contains("sceKernelGetDirectMemoryType"),
            "the action must route to the call that answered zero, not a general search: {:?}",
            f.action
        );
        assert!(
            f.evidence
                .iter()
                .any(|e| e.contains("sceKernelGetDirectMemoryType") && e.contains("0x0")),
            "the evidence must name the supplying call and its answer"
        );

        // Nothing answered the base: fall back to the general search, blame no one.
        let mut orphan = empty();
        orphan.total_calls = 100;
        let mut unrelated = call("libc::strlen", 6);
        unrelated.returned = Some(6); // not the null base
        orphan.tail = vec![unrelated];
        orphan.fault = Some(FaultSite {
            instruction: vec![0x48, 0x8b, 0x00],
            thread: Some(1),
            host_thread: Some(0),
            pointees: Vec::new(),
            kind: "read of".to_owned(),
            address: 0x38,
            instruction_pointer: 0x4000_0000_1234,
            region: Some("image".to_owned()),
            offset: Some(0x1234),
            inside_import: None,
            registers: None,
            frames: Vec::new(),
        });
        let orphan_action = findings(&orphan)
            .into_iter()
            .find(|f| f.gap == Gap::Faulted)
            .and_then(|f| f.action)
            .unwrap_or_default();
        assert!(
            orphan_action.contains("read the calls just before it")
                && !orphan_action.contains("strlen"),
            "with no supplier the finding searches, and blames no unrelated call: {orphan_action}"
        );
    }

    /// A copy that faults reading its source is named from its recorded argument, with the
    /// null-page base marked and the action routed to the pointer's producer rather than the copy;
    /// a copy whose source range does not cover the fault is not named.
    #[test]
    fn a_copy_that_faults_reading_a_null_source_is_named_not_the_memcpy() {
        let mut trace = empty();
        trace.total_calls = 100;
        let mut memcpy = call("libc::memcpy", 0x7400_0218_f070); // arg0 = destination, valid
        memcpy.args[1] = 0xa8; // the null-page source
        memcpy.args[2] = 0x50; // the length
        trace.tail = vec![memcpy];
        trace.fault = Some(FaultSite {
            instruction: vec![0x48, 0x8b, 0x00], // an ordinary read, not a trap
            thread: Some(1),
            host_thread: Some(0),
            pointees: Vec::new(),
            kind: "read of".to_owned(),
            address: 0xa8,
            instruction_pointer: 0x4000_0000_42eb,
            region: Some("VCRUNTIME140.dll".to_owned()),
            offset: Some(0x1dc8d),
            inside_import: None,
            registers: None,
            frames: Vec::new(),
        });

        let f = findings(&trace)
            .into_iter()
            .find(|f| f.gap == Gap::Faulted)
            .expect("a fault finding");
        assert!(
            f.evidence
                .iter()
                .any(|e| e.contains("memcpy") && e.contains("src 0xa8") && e.contains("null page")),
            "the copy must be named as reading a null-page source: {:?}",
            f.evidence
        );
        assert!(
            f.evidence
                .iter()
                .any(|e| e.contains("byte 0x0 into that source")),
            "the read offset into the source is computed: {:?}",
            f.evidence
        );
        let action = f.action.as_deref().unwrap_or("");
        assert!(
            action.contains("faithful byte copy") && action.contains("produced its source pointer"),
            "the action routes to the source's producer, not the copy: {action}"
        );

        // A copy whose recorded source does not cover the fault is not named as reading it.
        let mut valid = empty();
        valid.total_calls = 100;
        let mut good = call("libc::memcpy", 0x7400_0218_f068);
        good.args[1] = 0x6000_007f_c2f8; // a valid stack source, far from 0xa8
        good.args[2] = 0x8;
        valid.tail = vec![good];
        valid.fault = Some(FaultSite {
            instruction: vec![0x48, 0x8b, 0x00],
            thread: Some(1),
            host_thread: Some(0),
            pointees: Vec::new(),
            kind: "read of".to_owned(),
            address: 0xa8,
            instruction_pointer: 0x4000_0000_42eb,
            region: Some("image".to_owned()),
            offset: Some(0x10),
            inside_import: None,
            registers: None,
            frames: Vec::new(),
        });
        let g = findings(&valid)
            .into_iter()
            .find(|f| f.gap == Gap::Faulted)
            .expect("a fault finding");
        assert!(
            !g.evidence
                .iter()
                .any(|e| e.contains("faulted reading its source")),
            "a copy whose source range misses the fault is not named as reading it: {:?}",
            g.evidence
        );
    }

    /// A trace whose call list places two imports at known stub slots.
    fn trace_with_slots() -> CallTrace {
        CallTrace {
            forced_dumps: Vec::new(),
            ended_by: None,
            threads: Vec::new(),
            said: Vec::new(),
            quiet: None,
            title_modules: Vec::new(),
            module: "m".to_owned(),
            reached: "Entered".to_owned(),
            total_calls: 2,
            distinct: 2,
            frames: 0,
            frame_written: false,
            submission: None,
            calls: vec![
                CalledImport {
                    index: 0x81,
                    label: "libSceUlt::sceUltUlthreadRuntimeGetWorkAreaSize".to_owned(),
                    calls: 1,
                    implemented: false,
                    shape: String::new(),
                },
                CalledImport {
                    index: 0x215,
                    label: "libSceAgc::sceAgcCreateShader".to_owned(),
                    calls: 1,
                    implemented: false,
                    shape: String::new(),
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

    /// A tagged placeholder names the function that answered it (D567).
    ///
    /// The tag is `PLACEHOLDER_BASE | (0x10 + slot)` and the trace indexes calls by the same global
    /// stub index; this test cannot detect the two numberings diverging.
    #[test]
    fn a_tagged_placeholder_names_its_source() {
        let base = u64::from(orbistoun_core::PLACEHOLDER_BASE);
        let trace = trace_with_slots();
        assert_eq!(
            source_of(&trace, base | 0x91),
            Some("libSceUlt::sceUltUlthreadRuntimeGetWorkAreaSize"),
            "0x91 is slot 0x81 plus the 0x10 floor"
        );
        assert_eq!(
            source_of(&trace, base | 0x225),
            Some("libSceAgc::sceAgcCreateShader")
        );
        // A tag for a slot this run never called resolves to nothing, so a stray value in the tag
        // range is not read as a confident attribution.
        assert_eq!(source_of(&trace, base | 0x999), None);
        // Measured stale register values `0x7fff0201` and `0x7fffbe01` fall outside the placeholder
        // block, so they cannot be mistaken for a tag.
        assert_eq!(
            source_of(&trace, 0x7fff_0201),
            None,
            "PPSA28061's real stale register, now out of range"
        );
        assert_eq!(source_of(&trace, 0x7fff_be01), None, "and the other one");
    }

    /// An untagged placeholder or a fixed `GuestError` code names nothing, rather than the import
    /// at index 0.
    #[test]
    fn an_untagged_placeholder_attributes_nothing() {
        let base = u64::from(orbistoun_core::PLACEHOLDER_BASE);
        let trace = trace_with_slots();
        for fixed in [base, base | 0x1, base | 0xf] {
            assert_eq!(
                tagged_stub(fixed),
                None,
                "{fixed:#x} is a fixed placeholder code, not a tag"
            );
            assert_eq!(source_of(&trace, fixed), None);
        }
    }

    /// The detector recognises a tagged placeholder, which sits above the fixed codes.
    #[test]
    fn the_detector_sees_tagged_placeholders_too() {
        let base = u64::from(orbistoun_core::PLACEHOLDER_BASE);
        for tagged in [base | 0x10, base | 0x91, base | 0x225, base | 0xbeac] {
            assert!(
                looks_like_placeholder(tagged),
                "{tagged:#x} is one of ours and the detector missed it"
            );
        }
        // And still recognises the untagged one, and rejects an ordinary address.
        assert!(looks_like_placeholder(placeholder_code()));
        assert!(!looks_like_placeholder(0x4000_0000_0000));
    }

    /// Every tagged value the service can produce has bit 31 set and is recognised, across the
    /// whole slot range inside the reserved half-word (D670).
    #[test]
    fn every_tag_the_service_produces_is_negative_and_recognised() {
        const FLOOR: u64 = 0x10;
        let base = u64::from(orbistoun_core::PLACEHOLDER_BASE);
        for slot in [0_u64, 1, 0x81, 0x215, 0xbe9c, 0xffff - FLOOR] {
            let tag = base | (FLOOR + slot);
            assert_eq!(
                tag & 0x8000_0000,
                0x8000_0000,
                "{tag:#x} must be negative to the guest (D670)"
            );
            assert!(looks_like_placeholder(tag), "{tag:#x} must be recognised");
        }
    }
}
