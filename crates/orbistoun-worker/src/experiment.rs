//! The diagnostics a run can be put under, and the one place that knows about them.
//!
//! # Why these are not settings
//!
//! Each answers **one question**, once. A setting configures how the emulator behaves; a
//! diagnostic changes the program being observed in order to find something out, and then
//! goes away. Anything here that outlived its question would drift into being a permanent
//! workaround for a bug nobody found (D185).
//!
//! So they are read from the environment rather than the run configuration, and every one
//! of them is **recorded in the run's conditions** - because a verdict taken under a
//! diagnostic is not comparable with an ordinary one, and comparing them anyway is how a
//! settings change gets read as progress (D181).
//!
//! # Why they share a home
//!
//! There were three of these, each with its own parser, its own conditions field and its
//! own paragraph of documentation, and five more were wanted. Eight copies of one pattern
//! is the shape that drifts - three separate instances of exactly that were removed the
//! same day this was written (D213, D215, D217), and the last one had silently disabled
//! the only tool that could see the biggest wall.
//!
//! The *interface* stays one variable per question, because `ORBISTOUN_STACK_FILL=5a` is
//! easier to remember and to type than a grammar. Only the plumbing is shared (D220).
//!
//! # Why the method is black-box at all
//!
//! Worth stating plainly, because it is unusual. An emulator for this platform would
//! normally answer "what does this structure hold?" by reading an SDK header, another
//! project's source, or a disassembly of the vendor's own library. Principle 1 closes all
//! three. What is left is measurement - so the measuring tools are not a side quest here,
//! they *are* the method, and every one of them converts a guess into an experiment.

/// Which import an experiment applies to, and how that is decided.
///
/// Matching is by name **or by any part of the label**, so `libkernel::0x6abac2f3dc6f8cee`
/// is reachable as `0x6abac2f3dc6f8cee`. That is not a convenience: the functions most
/// worth experimenting on are the ones nothing has named, and a mechanism keyed only by
/// name would exclude exactly them (D198).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target(String);

impl Target {
    /// Whether `label` - `library::name` or `library::0xhash` - is one this applies to.
    pub fn matches(&self, label: &str) -> bool {
        let name = label.rsplit("::").next().unwrap_or(label);
        name == self.0 || label.contains(&self.0)
    }

    /// How it was asked for, for the run conditions.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Everything a run has been asked to do differently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Experiments {
    /// Imports to dump arguments for even though something implements them.
    ///
    /// The case that matters is when the implementation is *yours* and you suspect it:
    /// `memalign` was implemented in the morning and suspected by the afternoon, and the
    /// tool had just stopped being able to show what it was asked for (D198).
    pub dump: Vec<Target>,
    /// A byte to fill the guest stack with before entering.
    ///
    /// Answers "does this run depend on memory nobody wrote?". If two runs with different
    /// fills disagree, the guest read something uninitialised; if they agree, a whole class
    /// of explanation is eliminated rather than argued about (D185).
    pub stack_fill: Option<u8>,
    /// A byte to fill every heap allocation with before handing it to the guest.
    ///
    /// The same question, for the region the stack poison cannot reach. The host allocator
    /// returns uninitialised memory, which on a fresh page is usually **zero** - so a field
    /// nobody filled in and a deliberate zero are currently indistinguishable on the heap,
    /// and that is precisely the ambiguity the stack poison exists to remove (D220).
    pub heap_fill: Option<u8>,
    /// A value to plant at the address in an argument, before an import answers.
    ///
    /// Answers "is this argument an out-parameter the guest expects filled?". A stub policy
    /// can change what a function *answers*; nothing else can change what it *does* (D218).
    pub write: Vec<(Target, u8, i64, u64)>,
    /// Imports to answer with a chosen 64-bit value.
    ///
    /// **The one thing no diagnostic here could do.** The others change what memory holds
    /// or what an argument points at; none could change what a function *answers* unless
    /// it had a name, because the only mechanism for that is a policy file keyed by one.
    /// The function on the biggest wall has no name, so the question went untested while
    /// reading as tested (D230).
    pub returns: Vec<(Target, u64)>,
    /// A byte to fill zero-initialised static data with before the guest runs.
    ///
    /// The last region a poison could not reach. **It breaks a contract on purpose**: the
    /// guest is entitled to assume `.bss` is zero, so this makes it misbehave in ways an
    /// ordinary run would not. That is the point - if the value it was going to read from a
    /// static was never written by anything, the fault moves and says so (D223).
    pub bss_fill: Option<u8>,
    /// How the loader was told to resolve imports, when it was told anything.
    ///
    /// **In this list because it changes what the guest is.** An import left unresolved is a
    /// slot the guest finds empty, so a run under it reaches fewer imports by construction
    /// and is not comparable with an ordinary one - which is the whole reason the two slots
    /// exist (D312, D392).
    pub resolve: Option<String>,
    /// Which entry argument a run was told to hand over, when it was told.
    ///
    /// **Registered here as well as declared**, which is the step that gets forgotten: a
    /// setting `Experiments` cannot see is one a run can be under while reporting itself as
    /// ordinary, and that is how an honest status slot gets written by a propped run (D397).
    pub entry_argument: Option<String>,
    /// Which handoff field was poisoned, when one was.
    ///
    /// Also an intervention: the field holds an address nothing maps, so a runtime that uses
    /// it stops there rather than where it otherwise would (D390).
    pub handoff_poison: Option<String>,
    /// A region of guest address space to reserve before the run.
    ///
    /// **Asks a question the other diagnostics cannot.** They all assume the guest computed
    /// a wrong address; this asks whether the address was right all along and the region
    /// simply was not there. A fault reported as *"an address in no region this run mapped"*
    /// is as consistent with a missing mapping as with a bad pointer, and nothing had ever
    /// tested the first reading (D224).
    ///
    /// Emphatically a diagnostic. Mapping memory until a fault stops happening is the
    /// plausible-output trap principle 3 exists to refuse - what makes this legitimate is
    /// that it is ephemeral, recorded in the conditions, and answers a question rather than
    /// fixing a symptom.
    pub map: Option<(u64, u64)>,
    /// A value to write at a guest address before the run.
    ///
    /// **The absolute-address counterpart to [`Self::write`].** That one plants into what an
    /// argument points at, which reaches the stack; this reaches anything the loader mapped -
    /// which is where a static object lives, and static objects are where the walls are.
    ///
    /// Applied after relocation and before the entry jump, so it survives everything the
    /// loader does and is in place before the guest can read it (D223).
    pub poke: Option<(u64, u64)>,
    /// A region of guest memory to snapshot before the run and diff afterwards.
    ///
    /// **The cheapest way to ask what the guest actually initialised.** A watchpoint says
    /// which byte was touched and when, at the cost of debug registers and a per-platform
    /// API; a snapshot says which bytes ended up different, for a memcpy and no platform
    /// code at all. For "did anything ever fill this slot in?" the second is the whole
    /// answer (D223).
    pub watch: Option<(u64, u64)>,

    /// Addresses to trap on, as written - parsed where a bad one can be refused out loud.
    ///
    /// **The other half of `watch`, kept separate on purpose.** A snapshot says which bytes
    /// ended up different and this says which instruction touched them, so the cheap one is
    /// still the one to run first and the two compose: the snapshot names the words nobody
    /// wrote, and up to four of those addresses become the watchpoints for the next run
    /// (D223, D276).
    ///
    /// Held as text rather than parsed here because [`Experiments::from_env`] has nowhere to
    /// report a malformed request to, and a watchpoint that was asked for and silently not
    /// armed is the exact failure every diagnostic in this crate exists to avoid (D185).
    pub watchpoint: String,
    /// Whether to write self-identifying values into the memory-query structure.
    ///
    /// **The cheapest diagnostic here, and the most standard.** Instead of plausible
    /// values, each field gets a value that names itself - so whatever the guest does next
    /// says which field it read. No watchpoints and no new machinery: only different bytes.
    ///
    /// It has already worked by accident. The guest's next query offset is the `end` value,
    /// which is how field 1 is known to be the one it walks by - nobody set out to learn
    /// that (D220).
    pub mark_query: bool,
}

impl Experiments {
    /// Reads every diagnostic from the environment.
    pub fn from_env() -> Self {
        Self {
            dump: targets(&orbistoun_env::DUMP.get().unwrap_or_default()),
            stack_fill: byte(&orbistoun_env::STACK_FILL.get().unwrap_or_default()),
            heap_fill: byte(&orbistoun_env::HEAP_FILL.get().unwrap_or_default()),
            write: parse_write(&orbistoun_env::WRITE.get().unwrap_or_default()),
            returns: parse_returns(&orbistoun_env::RETURN.get().unwrap_or_default()),
            bss_fill: byte(&orbistoun_env::BSS_FILL.get().unwrap_or_default()),
            map: parse_region(&orbistoun_env::MAP.get().unwrap_or_default()),
            poke: parse_pair(&orbistoun_env::POKE.get().unwrap_or_default()),
            watch: parse_region(&orbistoun_env::WATCH.get().unwrap_or_default()),
            watchpoint: orbistoun_env::WATCHPOINT.get().unwrap_or_default(),
            mark_query: truthy(&orbistoun_env::MARK_QUERY.get().unwrap_or_default()),
            resolve: orbistoun_env::RESOLVE.get(),
            handoff_poison: orbistoun_env::HANDOFF_POISON.get(),
            entry_argument: orbistoun_env::ENTRY_ARGUMENT.get(),
        }
    }

    /// Whether the run is ordinary, and therefore comparable with other ordinary runs.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether any diagnostic in force **changes the program** rather than only observing.
    ///
    /// Derived from the registry rather than listed again here, so a diagnostic added with
    /// the wrong effect is wrong in one place instead of two (D227).
    pub fn intervenes(&self) -> bool {
        [
            (!self.dump.is_empty(), orbistoun_env::DUMP.effect),
            (self.resolve.is_some(), orbistoun_env::RESOLVE.effect),
            (
                self.handoff_poison.is_some(),
                orbistoun_env::HANDOFF_POISON.effect,
            ),
            (
                self.entry_argument.is_some(),
                orbistoun_env::ENTRY_ARGUMENT.effect,
            ),
            (self.stack_fill.is_some(), orbistoun_env::STACK_FILL.effect),
            (self.heap_fill.is_some(), orbistoun_env::HEAP_FILL.effect),
            (self.bss_fill.is_some(), orbistoun_env::BSS_FILL.effect),
            (self.map.is_some(), orbistoun_env::MAP.effect),
            (self.poke.is_some(), orbistoun_env::POKE.effect),
            (!self.write.is_empty(), orbistoun_env::WRITE.effect),
            (!self.returns.is_empty(), orbistoun_env::RETURN.effect),
            (self.watch.is_some(), orbistoun_env::WATCH.effect),
            (
                !self.watchpoint.is_empty(),
                orbistoun_env::WATCHPOINT.effect,
            ),
            (self.mark_query, orbistoun_env::MARK_QUERY.effect),
        ]
        .iter()
        .any(|(active, effect)| *active && effect.needs_caveat())
    }

    /// Every active diagnostic, in one line, for the run conditions.
    ///
    /// **Not the switches - what they did.** `dump` costs nothing to state; the others carry
    /// what they changed, because a diagnostic that was requested and had no effect must not
    /// read the same as one that ran (D218).
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.dump.is_empty() {
            let named: Vec<&str> = self.dump.iter().map(Target::as_str).collect();
            parts.push(format!("dump {}", named.join(",")));
        }
        if let Some(b) = self.stack_fill {
            parts.push(format!("stack filled {b:#04x}"));
        }
        if let Some(b) = self.heap_fill {
            parts.push(format!("heap filled {b:#04x}"));
        }
        for (target, value) in &self.returns {
            parts.push(format!("{} answers {value:#x}", target.as_str()));
        }
        for (target, slot, offset, value) in &self.write {
            let at = match offset {
                0 => format!("*arg{slot}"),
                d if *d > 0 => format!("*(arg{slot}+{d:#x})"),
                d => format!("*(arg{slot}-{:#x})", -d),
            };
            parts.push(format!("{value:#x} at {at} of {}", target.as_str()));
        }
        if let Some(b) = self.bss_fill {
            parts.push(format!("static data filled {b:#04x}"));
        }
        if let Some(how) = &self.resolve {
            parts.push(format!("imports resolved {how}"));
        }
        if let Some(argument) = &self.entry_argument {
            parts.push(format!("entered with the {argument} argument"));
        }
        if let Some(field) = &self.handoff_poison {
            parts.push(format!("handoff field {field} poisoned"));
        }
        if let Some((base, len)) = self.map {
            parts.push(format!("{base:#x}+{len:#x} reserved"));
        }
        if let Some((at, value)) = self.poke {
            parts.push(format!("{value:#x} poked into {at:#x}"));
        }
        if let Some((base, len)) = self.watch {
            parts.push(format!("watching {base:#x}+{len:#x}"));
        }
        if !self.watchpoint.is_empty() {
            parts.push(format!("trapping on {}", self.watchpoint));
        }
        if self.mark_query {
            parts.push("memory-query fields marked".to_owned());
        }
        parts.join("; ")
    }

    /// The watchpoints this run asked for, or why the request cannot be honoured.
    ///
    /// Separated from [`Self::from_env`] so the caller decides what a refusal means - which
    /// here is halting before the guest starts, because running anyway would produce a
    /// report indistinguishable from one where the watchpoints had worked.
    pub fn watchpoints(&self) -> Result<Vec<crate::watchpoint::Request>, String> {
        crate::watchpoint::parse(&self.watchpoint)
    }
}

/// A comma-separated list of imports.
fn targets(raw: &str) -> Vec<Target> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Target(s.to_owned()))
        .collect()
}

/// A hexadecimal byte, with or without the prefix.
fn byte(raw: &str) -> Option<u8> {
    if raw.is_empty() {
        return None;
    }
    u8::from_str_radix(raw.trim_start_matches("0x"), 16).ok()
}

/// Whether a switch with no value is on.
///
/// Anything but an explicit off. A switch somebody set to `0` meaning "off" and got "on"
/// would be a diagnostic running when nobody asked, which is worse than one that refuses.
fn truthy(raw: &str) -> bool {
    !raw.is_empty() && !matches!(raw, "0" | "off" | "no" | "false")
}

/// `<addr>:<value>`, refused outright when it is not exactly that.
fn parse_pair(raw: &str) -> Option<(u64, u64)> {
    if raw.is_empty() {
        return None;
    }
    let (at, value) = raw.split_once(':')?;
    Some((number(at.trim())?, number(value.trim())?))
}

/// `<addr>` or `<addr>+<len>`, refused outright when it is neither.
///
/// A default length rather than a required one, because the common case is "show me what
/// happened around this address" and a person copying an address out of a fault report
/// should not have to invent a size to go with it.
fn parse_region(raw: &str) -> Option<(u64, u64)> {
    /// Enough for a small structure and its neighbours, which is what a report is read for.
    const DEFAULT_LENGTH: u64 = 0x80;
    if raw.is_empty() {
        return None;
    }
    let (base, len) = raw
        .split_once('+')
        .map_or((raw, None), |(b, l)| (b, Some(l)));
    let base = number(base.trim())?;
    let len = match len {
        Some(text) => number(text.trim())?,
        None => DEFAULT_LENGTH,
    };
    (len > 0).then_some((base, len))
}

/// A hexadecimal or decimal number, as a person would type one.
fn number(text: &str) -> Option<u64> {
    text.strip_prefix("0x").map_or_else(
        || text.parse::<u64>().ok(),
        |hex| u64::from_str_radix(hex, 16).ok(),
    )
}

/// `<import>:<slot>[+<offset>]:<value>`, comma-separated for more than one.
///
/// A malformed request that silently planted nothing would be reported as "the experiment
/// ran and changed nothing", which is the failure this whole mechanism exists to avoid - so
/// **one bad clause refuses the whole list** rather than quietly planting the rest.
///
/// The offset is what makes a structure addressable. Without it only the word an argument
/// points *at* can be planted, and the question at a wall is which member of that structure
/// the guest was waiting for. With distinct values per clause, one run answers it (D229).
/// Whether a target is shaped like a label rather than like a mis-split clause.
///
/// **Reading a clause from the right makes the target greedy, and this is the guard on
/// it.** A label is a bare symbol, a bare hash, or `library::symbol` - so it may hold
/// double colons and must hold no single one. Without this, `f:0x1:0x2` parses happily
/// as a target of `f:0x1`, which is not a name anything exports; an existing test said so
/// and caught the first version of this change.
fn is_label(target: &str) -> bool {
    !target.replace("::", "").contains(':')
}

fn parse_write(raw: &str) -> Vec<(Target, u8, i64, u64)> {
    let mut plants = Vec::new();
    for clause in raw.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        // **From the right, so the import may contain colons.** The trailing two fields
        // are fixed - a slot and a value - so everything before them is the target, and
        // `libkernel::sceFoo` becomes expressible. Splitting left to right could not
        // represent it: a qualified label produced five fields where three were expected
        // and the whole clause was rejected, which is how two hundred and seventy-six runs
        // planted nothing and reported twenty-three clean negatives.
        let mut parts = clause.rsplitn(3, ':');
        let (Some(value), Some(slot), Some(import)) = (parts.next(), parts.next(), parts.next())
        else {
            return Vec::new();
        };
        let import = import.trim();
        if import.is_empty() || !is_label(import) {
            return Vec::new();
        }
        let (slot, offset) = match slot.trim().split_once(['+', '-']) {
            Some((position, magnitude)) => {
                let Ok(magnitude) = magnitude.trim().parse::<i64>() else {
                    return Vec::new();
                };
                let signed = if slot.contains('-') {
                    -magnitude
                } else {
                    magnitude
                };
                (position.trim(), signed)
            }
            None => (slot.trim(), 0),
        };
        let (Ok(slot), Some(value)) = (slot.parse::<u8>(), number(value.trim())) else {
            return Vec::new();
        };
        plants.push((Target(import.to_owned()), slot, offset, value));
    }
    plants
}

/// `<import>:<value>`, comma-separated for more than one.
///
/// Refused whole on a malformed clause, for the same reason as [`parse_write`]: a partly
/// applied experiment reports conditions describing what was asked for rather than what
/// happened.
fn parse_returns(raw: &str) -> Vec<(Target, u64)> {
    let mut forced = Vec::new();
    for clause in raw.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        // From the right, for the reason `parse_write` gives: the trailing field is the
        // value, so everything before it is the target and may be qualified.
        let mut parts = clause.rsplitn(2, ':');
        let (Some(value), Some(import)) = (parts.next(), parts.next()) else {
            return Vec::new();
        };
        let import = import.trim();
        if import.is_empty() || !is_label(import) {
            return Vec::new();
        }
        let Some(value) = number(value.trim()) else {
            return Vec::new();
        };
        forced.push((Target(import.to_owned()), value));
    }
    forced
}

#[cfg(test)]
mod tests {
    use super::{Experiments, Target, byte, parse_returns, parse_write, targets, truthy};

    /// **A qualified label can be asked for, and could not be before.**
    ///
    /// `ORBISTOUN_WRITE` is `<import>:<slot>:<value>`. Split left to right,
    /// `libkernel::sceFoo:1:0x1100` is five fields where three are expected, so the whole
    /// clause was discarded and the run planted nothing - silently, because a run that
    /// plants nothing looks exactly like one that changed nothing. That distinction is the
    /// only reason it was ever noticed.
    #[test]
    fn a_library_qualified_import_can_be_asked_for() {
        assert_eq!(
            parse_write("libkernel::sceFoo:1:0x1100"),
            vec![(Target("libkernel::sceFoo".to_owned()), 1, 0, 0x1100)]
        );
        assert_eq!(
            parse_returns("libkernel::sceFoo:0x700000000000"),
            vec![(Target("libkernel::sceFoo".to_owned()), 0x7000_0000_0000)]
        );
    }

    /// **A stray single colon is still a mis-split, not a name.**
    ///
    /// Reading from the right makes the target greedy, so it has to be checked. A label is
    /// a bare symbol, a bare hash, or `library::symbol`; `f:0x1` is none of those, and an
    /// existing test caught the first version of this change accepting it.
    #[test]
    fn a_target_with_a_stray_colon_is_refused() {
        assert!(parse_returns("f:0x1:0x2").is_empty());
        assert!(parse_write("f:0x1:2:0x3").is_empty());
        assert!(
            !parse_returns("lib::f:0x2").is_empty(),
            "a real label was refused"
        );
    }

    /// A bare symbol still works, because most callers pass one.
    #[test]
    fn a_bare_symbol_is_unaffected_by_reading_from_the_right() {
        assert_eq!(
            parse_write("sceFoo:2:0x40"),
            vec![(Target("sceFoo".to_owned()), 2, 0, 0x40)]
        );
        assert_eq!(
            parse_returns("0x6abac2f3dc6f8cee:0x1"),
            vec![(Target("0x6abac2f3dc6f8cee".to_owned()), 1)]
        );
    }

    /// Too few fields is still refused, rather than silently taking a default.
    ///
    /// Reading from the right makes over-long clauses legal, and it must not make
    /// under-long ones legal too - a clause missing its slot would otherwise plant a value
    /// at argument zero of something nobody named.
    #[test]
    fn a_clause_missing_a_field_is_still_refused() {
        assert!(parse_write("sceFoo").is_empty());
        assert!(parse_write("sceFoo:1").is_empty());
        assert!(parse_returns("sceFoo").is_empty());
        assert!(
            parse_write(":1:0x1").is_empty(),
            "an empty import was accepted"
        );
    }

    #[test]
    fn an_unnamed_import_is_reachable_by_its_hash() {
        // **The case the whole mechanism is for.** The functions most worth experimenting
        // on are the ones nothing has named, and matching only on names would exclude
        // exactly them (D198).
        let by_hash = Target("0x6abac2f3dc6f8cee".to_owned());
        assert!(by_hash.matches("libkernel::0x6abac2f3dc6f8cee"));
        assert!(!by_hash.matches("libkernel::sceKernelCreateSema"));

        let by_name = Target("sceKernelCreateSema".to_owned());
        assert!(by_name.matches("libkernel::sceKernelCreateSema"));
        assert!(!by_name.matches("libkernel::0x6abac2f3dc6f8cee"));
    }

    #[test]
    fn a_malformed_request_is_refused_rather_than_half_understood() {
        assert_eq!(
            parse_write("0xabc:0:0x11000000"),
            vec![(Target("0xabc".to_owned()), 0, 0, 0x1100_0000)]
        );
        assert_eq!(
            parse_write("sceKernelFoo:5:4096"),
            vec![(Target("sceKernelFoo".to_owned()), 5, 0, 4096)]
        );
        for bad in [
            "",
            "just-a-name",
            "name:0",
            "name:0:0x11:extra",
            "name:notaslot:1",
            "name:0:notavalue",
            ":0:1",
        ] {
            assert!(parse_write(bad).is_empty(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn a_switch_is_on_unless_it_is_explicitly_off() {
        // Somebody setting `0` and meaning "off" must not get "on": a diagnostic running
        // when nobody asked is worse than one that refuses to.
        assert!(truthy("1") && truthy("yes") && truthy("on"));
        for off in ["", "0", "off", "no", "false"] {
            assert!(!truthy(off), "{off:?} should be off");
        }
    }

    #[test]
    fn a_list_survives_the_spacing_people_actually_type() {
        let parsed = targets(" memalign , 0xabc ,, ");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].as_str(), "memalign");
        assert_eq!(parsed[1].as_str(), "0xabc");
        assert!(targets("").is_empty());
        assert_eq!(byte("5a"), Some(0x5a));
        assert_eq!(byte("0x5a"), Some(0x5a));
        assert_eq!(byte("zz"), None);
    }

    #[test]
    fn an_ordinary_run_says_nothing_and_a_diagnostic_run_says_what_it_did() {
        // The conditions line is what stops a verdict taken under a diagnostic being
        // compared with an ordinary one, so an empty run must produce an empty line and a
        // non-empty one must name what changed (D181, D185).
        assert!(Experiments::default().is_empty());
        assert_eq!(Experiments::default().describe(), "");

        let e = Experiments {
            stack_fill: Some(0x5a),
            mark_query: true,
            ..Experiments::default()
        };
        assert!(!e.is_empty());
        let described = e.describe();
        assert!(described.contains("stack filled 0x5a"), "{described}");
        assert!(described.contains("marked"), "{described}");
    }

    /// A structure member is reachable, and more than one in a run.
    ///
    /// The list is the point: six candidate slots used to be six runs against six separate
    /// baselines, and with distinct values it is one run where the guest names the slot it
    /// used. Offsets carry a sign because a header below a pointer is as ordinary a shape
    /// as a field above one.
    #[test]
    fn a_plant_may_name_an_offset_and_a_list() {
        assert_eq!(
            parse_write("0xabc:0+24:0x44"),
            vec![(Target("0xabc".to_owned()), 0, 24, 0x44)]
        );
        assert_eq!(
            parse_write("0xabc:5-8:0x55"),
            vec![(Target("0xabc".to_owned()), 5, -8, 0x55)]
        );
        assert_eq!(
            parse_write("f:0+8:0x11, g:1:0x22"),
            vec![
                (Target("f".to_owned()), 0, 8, 0x11),
                (Target("g".to_owned()), 1, 0, 0x22),
            ]
        );
    }

    /// One bad clause refuses the whole list rather than planting the rest.
    ///
    /// The half-applied case is the one that lies: it reports an experiment that ran, under
    /// conditions that describe what was asked for rather than what happened.
    #[test]
    fn a_malformed_clause_refuses_every_plant() {
        for bad in [
            "f:0+8:0x11,g:1",
            "f:0+8:0x11,:1:0x22",
            "f:0+x:0x11",
            "f:0+8:0x11,g:1:0x22:0x33",
        ] {
            assert!(
                parse_write(bad).is_empty(),
                "{bad:?} should be refused whole"
            );
        }
    }

    /// A forced answer is reachable by hash, which is the whole point of it.
    ///
    /// The policy file is keyed by symbol name, so the function on the biggest wall - which
    /// has none - was unreachable by any means of changing what it answered.
    #[test]
    fn a_forced_return_reaches_a_function_with_no_name() {
        assert_eq!(
            parse_returns("0x6abac2f3dc6f8cee:0x700000000000"),
            vec![(Target("0x6abac2f3dc6f8cee".to_owned()), 0x7000_0000_0000)]
        );
        assert_eq!(
            parse_returns("f:0x1, g:0x2"),
            vec![(Target("f".to_owned()), 1), (Target("g".to_owned()), 2),]
        );
        for bad in ["f", "f:", ":0x1", "f:0x1:0x2", "f:zz", "f:0x1,g"] {
            assert!(parse_returns(bad).is_empty(), "{bad:?} should be refused");
        }
    }
}
