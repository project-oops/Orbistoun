//! The diagnostics a run can be put under, and the one place that knows about them.
//!
//! Each diagnostic answers one question by changing the program being observed, so it is read
//! from the environment rather than the run configuration (D221), and every one is recorded in
//! the run's conditions: a verdict taken under a diagnostic is not comparable with an ordinary
//! one (D181). The interface is one variable per question; only the parsing and recording are
//! shared. Measurement is the method here, since provenance rules out reading vendor headers,
//! other projects' source or disassembled vendor libraries.

/// Which import an experiment applies to, and how that is decided.
/// Matching is by name or by any part of the label, so `libkernel::0x6abac2f3dc6f8cee` is
/// reachable as `0x6abac2f3dc6f8cee`: the functions most worth experimenting on are often the
/// ones nothing has named.
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
    /// Imports to dump arguments for even though something implements them, for when the
    /// implementation itself is suspect.
    pub dump: Vec<Target>,
    /// A byte to fill the guest stack with before entering.
    ///
    /// Two runs with different fills that disagree show the guest read uninitialised stack.
    pub stack_fill: Option<u8>,
    /// A byte to fill every heap allocation with before handing it to the guest.
    ///
    /// The host allocator's fresh pages are usually zero, so without a fill a field nobody wrote
    /// and a deliberate zero look the same on the heap.
    pub heap_fill: Option<u8>,
    /// A value to plant at the address in an argument, before an import answers.
    ///
    /// Answers "is this argument an out-parameter the guest expects filled?".
    pub write: Vec<(Target, u8, i64, u64)>,
    /// Imports to answer with a chosen 64-bit value.
    ///
    /// Reaches functions by hash as well as name, which a policy file keyed by symbol name
    /// cannot (D166).
    pub returns: Vec<(Target, u64)>,
    /// A byte to fill zero-initialised static data with before the guest runs.
    ///
    /// It breaks the guarantee that `.bss` is zero on purpose: if the guest reads a static that
    /// nothing wrote, the fault moves.
    pub bss_fill: Option<u8>,
    /// How the loader was told to resolve imports, when it was told anything.
    ///
    /// An intervention: an unresolved import is a slot the guest finds empty, so the run is not
    /// comparable with an ordinary one (D392).
    pub resolve: Option<String>,
    /// Which entry argument a run was told to hand over, when it was told.
    ///
    /// Registered here so a run under it cannot report itself as ordinary.
    pub entry_argument: Option<String>,
    /// Which handoff field was poisoned, when one was.
    ///
    /// An intervention: the field holds an address nothing maps, so a runtime that uses it stops
    /// there (D390).
    pub handoff_poison: Option<String>,
    /// A region of guest address space to reserve before the run.
    ///
    /// Asks whether a faulting address was right and the region simply absent, rather than a bad
    /// pointer. It is a diagnostic only: ephemeral, recorded in the conditions, never a fix.
    pub map: Option<(u64, u64)>,
    /// A value to write at a guest address before the run.
    ///
    /// The absolute-address counterpart to [`Self::write`], reaching anything the loader mapped,
    /// including static objects. Applied after relocation and before the entry jump.
    pub poke: Option<(u64, u64)>,
    /// A region of guest memory to snapshot before the run and diff afterwards.
    ///
    /// The cheapest way to ask what the guest initialised; see [`crate::watch`].
    pub watch: Option<(u64, u64)>,

    /// Addresses to trap on, as written.
    ///
    /// Separate from `watch` (D276): the snapshot names the words nobody wrote, and those
    /// addresses become the next run's watchpoints. Held as text because
    /// [`Experiments::from_env`] cannot report a malformed request; [`Self::watchpoints`] parses
    /// it where a refusal can halt the run.
    pub watchpoint: String,
    /// Whether to write self-identifying values into the memory-query structure.
    ///
    /// Each field gets a value that names itself, so whatever the guest does next says which
    /// field it read.
    pub mark_query: bool,
    /// Whether the asynchronous file path delivers the file it resolved.
    ///
    /// Carried here because it intervenes, so the verdict carries a caveat.
    pub apr_deliver: bool,
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
            apr_deliver: orbistoun_env::APR_DELIVER.is_set(),
            resolve: orbistoun_env::RESOLVE.get(),
            handoff_poison: orbistoun_env::HANDOFF_POISON.get(),
            entry_argument: orbistoun_env::ENTRY_ARGUMENT.get(),
        }
    }

    /// Whether the run is ordinary, and therefore comparable with other ordinary runs.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether any diagnostic in force changes the program rather than only observing (D227).
    ///
    /// Asked of the [`orbistoun_env`] registry for every variable that is set, so a new diagnostic
    /// is covered by being declared. A variable set but unparseable still counts as intervening,
    /// so the run is refused rather than recorded.
    pub fn intervenes(&self) -> bool {
        any_intervenes(&orbistoun_env::active())
    }

    /// Every active diagnostic, in one line, for the run conditions.
    ///
    /// Each part states what the diagnostic changed, not only that it was switched on.
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
        if self.apr_deliver {
            parts.push("the asynchronous file path delivering what it resolved".to_owned());
        }
        parts.join("; ")
    }

    /// The watchpoints this run asked for, or why the request cannot be honoured.
    ///
    /// Separated from [`Self::from_env`] so the caller decides what a refusal means: here, halting
    /// before the guest starts, since a run without its watchpoints reads like one with them.
    ///
    /// # Errors
    ///
    /// When the watchpoint list does not parse; see [`crate::watchpoint::parse`].
    pub fn watchpoints(&self) -> Result<Vec<crate::watchpoint::Request>, crate::Error> {
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
/// Anything but an explicit off, so `0` meaning "off" never runs a diagnostic nobody asked for.
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
/// The length defaults, so an address copied out of a fault report needs no invented size.
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

/// Whether a target is shaped like a label rather than like a mis-split clause.
///
/// Reading a clause from the right makes the target greedy, and this guards it: a label is a
/// bare symbol, a bare hash, or `library::symbol`, so it may hold double colons and no single
/// one. Without it `f:0x1:0x2` parses as a target of `f:0x1`.
fn is_label(target: &str) -> bool {
    !target.replace("::", "").contains(':')
}

/// `<import>:<slot>[+<offset>]:<value>`, comma-separated for more than one.
///
/// One bad clause refuses the whole list, since a partly planted list would report an
/// experiment that ran. The signed offset addresses a member of the structure an argument
/// points at, and distinct values per clause let one run name the member the guest used.
fn parse_write(raw: &str) -> Vec<(Target, u8, i64, u64)> {
    let mut plants = Vec::new();
    for clause in raw.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        // From the right, so the import may be qualified: the trailing slot and value are fixed and
        // everything before them is the target.
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

/// Whether any of these active variables changes the program.
///
/// The decision, with the environment left outside it: [`Experiments::intervenes`] is the thin
/// wrapper that reads, and this is testable without mutating process-global state.
fn any_intervenes(active: &[(&'static orbistoun_env::Var, String)]) -> bool {
    active.iter().any(|(var, _)| var.effect.needs_caveat())
}

#[cfg(test)]
mod tests {
    use super::{Experiments, Target, byte, parse_returns, parse_write, targets, truthy};

    /// A library-qualified label can be asked for in a plant and a forced return.
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

    /// A stray single colon is a mis-split, not a name, and a real label is still accepted.
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
    /// A clause missing its slot must not plant at argument zero of something nobody named.
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

    /// An unnamed import is reachable by its hash, and a name matches only its own label.
    #[test]
    fn an_unnamed_import_is_reachable_by_its_hash() {
        let by_hash = Target("0x6abac2f3dc6f8cee".to_owned());
        assert!(by_hash.matches("libkernel::0x6abac2f3dc6f8cee"));
        assert!(!by_hash.matches("libkernel::sceKernelCreateSema"));

        let by_name = Target("sceKernelCreateSema".to_owned());
        assert!(by_name.matches("libkernel::sceKernelCreateSema"));
        assert!(!by_name.matches("libkernel::0x6abac2f3dc6f8cee"));
    }

    /// A malformed write request is refused rather than half understood.
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

    /// A switch is on unless explicitly off.
    #[test]
    fn a_switch_is_on_unless_it_is_explicitly_off() {
        assert!(truthy("1") && truthy("yes") && truthy("on"));
        for off in ["", "0", "off", "no", "false"] {
            assert!(!truthy(off), "{off:?} should be off");
        }
    }

    /// A target list and a fill byte tolerate the spacing and prefixes people type.
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

    /// An ordinary run's conditions line is empty, and a diagnostic run's names what changed
    /// (D181).
    #[test]
    fn an_ordinary_run_says_nothing_and_a_diagnostic_run_says_what_it_did() {
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

    /// A plant may name a signed offset into a structure, and several plants fit in one run.
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

    /// A forced answer reaches a function by hash, and a malformed clause refuses the list.
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

    /// Every diagnostic the registry marks as changing the program makes a run intervened.
    ///
    /// Driven from the registry, so a new diagnostic is covered once declared. It cannot catch a
    /// wrong effect in the registry, nor a break in the one-line wrapper
    /// [`Experiments::intervenes`], which reads process-global state and is left untested.
    #[test]
    fn every_intervening_diagnostic_in_the_registry_makes_a_run_intervened() {
        let intervening: Vec<&orbistoun_env::Var> = orbistoun_env::REGISTRY
            .iter()
            .filter(|v| v.effect.needs_caveat())
            .collect();
        assert!(
            !intervening.is_empty(),
            "the registry lists no intervening diagnostics, so this test proves nothing"
        );

        for var in intervening {
            assert!(
                super::any_intervenes(&[(var, "1".to_owned())]),
                concat!(
                    "{} is declared as changing the program, and a run under it would ",
                    "still be recorded as an honest measurement"
                ),
                var.name
            );
        }
    }

    /// A run under no diagnostic is not intervened, or the guard would refuse every ordinary run.
    #[test]
    fn a_run_under_no_diagnostic_is_not_intervened() {
        assert!(!super::any_intervenes(&[]));
    }

    /// An observing diagnostic leaves a run recordable, which is what the `Observes` tier is for.
    #[test]
    fn an_observing_diagnostic_leaves_a_run_recordable() {
        let observing: Vec<&orbistoun_env::Var> = orbistoun_env::REGISTRY
            .iter()
            .filter(|v| !v.effect.needs_caveat())
            .collect();
        assert!(!observing.is_empty(), "no observing variables to check");
        for var in observing {
            assert!(
                !super::any_intervenes(&[(var, "1".to_owned())]),
                "{} only observes, and a run under it was disqualified anyway",
                var.name
            );
        }
    }
}
