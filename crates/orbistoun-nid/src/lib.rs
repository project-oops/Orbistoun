//! NID hashing and symbol-name resolution.
//!
//! A guest module imports a library plus a NID: a 64-bit hash of the symbol name, encoded
//! in a base64 alphabet. [`NidHasher`] maps a name forward to its NID; [`SymbolDb`] maps a
//! NID back to a name by lookup, since a hash is not invertible.
//!
//! The hash suffix is runtime data supplied with the symbol database (D071); the default is
//! `selfish-nid`'s committed suffix. `docs/SYMBOLS.md` describes the file format. [`Nid`]
//! holds the first digest byte as its most significant byte, the reverse of
//! `selfish_nid::Nid`; the two convert with one byte swap and encode to the same characters.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A 64-bit symbol hash as it appears in a guest module import table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Nid(u64);

impl Nid {
    /// Wraps a raw 64-bit hash read out of an import table.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw 64-bit hash.
    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

impl From<selfish_nid::Nid> for Nid {
    fn from(nid: selfish_nid::Nid) -> Self {
        Self(nid.value().swap_bytes())
    }
}

impl From<Nid> for selfish_nid::Nid {
    fn from(nid: Nid) -> Self {
        Self::from_value(nid.0.swap_bytes())
    }
}

impl std::fmt::Display for Nid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}

/// An import as it appears in a dynamic symbol name.
///
/// Symbol names take the form `H2e8t5ScQGc#B#C`: an encoded NID, then a library id, then a
/// module id, both small base64-encoded integers indexing the dynamic table's library and
/// module entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodedImport {
    /// The hash, decoded.
    pub nid: Nid,
    /// Library index within the module's import list.
    pub library_id: u16,
    /// Module index.
    pub module_id: u16,
}

/// Decodes a dynamic symbol name of the form `<nid>#<library>#<module>`.
///
/// Returns `None` for any name that is not in that form: ordinary symbol names exist too,
/// and a name that does not encode an import is not an error.
pub fn decode_symbol_name(name: &str) -> Option<EncodedImport> {
    let import = selfish_nid::decode_symbol_name(name)?;
    Some(EncodedImport {
        nid: import.nid.into(),
        library_id: import.library_id,
        module_id: import.module_id,
    })
}

/// The suffix orbistoun uses unless told otherwise: `selfish-nid`'s committed one.
pub fn default_suffix() -> Vec<u8> {
    selfish_nid::suffix()
}

/// Decodes an even-length hex string.
///
/// Returns `None` on an odd length or a non-hex digit rather than skipping the character:
/// a suffix missing a byte hashes to plausible values that match nothing.
pub fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

/// Decodes a bare eleven-character NID, with no library or module beside it.
///
/// The form a conformance probe reading an export table reports. `None` unless the input
/// is exactly eleven characters of the alphabet: any other length decodes to a plausible
/// number that agrees with nothing.
#[must_use]
pub fn decode_nid(encoded: &str) -> Option<Nid> {
    selfish_nid::Nid::decode(encoded).ok().map(Nid::from)
}

/// Encodes a NID into the eleven-character form a symbol name carries.
pub fn encode_nid(nid: Nid) -> String {
    selfish_nid::Nid::from(nid).encode()
}

/// Renders bytes as lowercase hex.
///
/// The inverse of [`decode_hex`], so a suffix is written back into a database file exactly
/// as it is read out of one.
pub fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Computes NIDs from symbol names.
///
/// [`NidHasher::default`] uses the suffix orbistoun ships with. Cheap to clone; hold one
/// per loader.
#[derive(Debug, Clone)]
pub struct NidHasher {
    suffix: Vec<u8>,
}

impl NidHasher {
    /// How many bytes the suffix has. For tests and diagnostics.
    pub fn suffix_len(&self) -> usize {
        self.suffix.len()
    }

    /// The suffix itself, for a consumer that has to hash somewhere this hasher cannot reach.
    ///
    /// The kernel resolves a `sceKernelDlsym` name against guest export tables at the call,
    /// where a `NidHasher` built from configuration does not reach (D517).
    pub fn suffix_bytes(&self) -> &[u8] {
        &self.suffix
    }
}

impl Default for NidHasher {
    fn default() -> Self {
        Self::new(default_suffix())
    }
}

impl NidHasher {
    /// Creates a hasher that appends `suffix` to each symbol name before hashing.
    ///
    /// An empty suffix is accepted for tests but matches no real guest import.
    pub fn new(suffix: impl Into<Vec<u8>>) -> Self {
        Self {
            suffix: suffix.into(),
        }
    }

    /// Hashes `name` to the NID a guest module imports it by.
    pub fn hash(&self, name: &str) -> Nid {
        selfish_nid::Nid::with_suffix(name, &self.suffix).into()
    }
}

/// Maps NIDs back to symbol names.
///
/// Populated from a symbol-database file. A lookup that misses is a normal result, not an
/// error: an unknown NID is still reportable in an import dump.
#[derive(Debug, Clone, Default)]
pub struct SymbolDb {
    by_nid: HashMap<Nid, String>,
}

/// The on-disk shape of a symbol database.
///
/// One file carries both the hash suffix and the known names, so a single input determines
/// resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDbFile {
    /// Hex-encoded byte suffix appended to a name before hashing.
    pub suffix_hex: String,
    /// Known symbol names. NIDs are derived, not stored, so the file cannot disagree with
    /// itself.
    pub names: Vec<String>,
    /// How each name was arrived at, keyed by name.
    ///
    /// Each entry says which of this repository's inputs produced the name and where in
    /// them, so the name can be re-derived in isolation (D213). Optional and separate from
    /// `names`, so a database from elsewhere still loads; a name with no entry is reported
    /// as unaccounted for by an audit.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub derivations: std::collections::BTreeMap<String, Derivation>,
}

impl SymbolDbFile {
    /// The database that ships with the tool.
    ///
    /// Loaded by default so every run names the hashes the shipped file already knows
    /// (D188). Embedded rather than read from disk so a portable build and an installed one
    /// behave the same; `--symbols-db` overrides it for a database under construction.
    ///
    /// # Panics
    ///
    /// If the shipped file is malformed, which a test in this crate rules out.
    pub fn builtin() -> Self {
        Self::from_json(EMBEDDED_DB).expect("the shipped symbol database is malformed")
    }
}

/// The symbol database shipped with the tool.
///
/// Read from `symbols/` at the workspace root rather than copied into this crate, so the
/// loaded file is the one CI audits. This crate therefore builds only inside its workspace.
const EMBEDDED_DB: &str = include_str!("../../../symbols/generated.json");

/// Where a name came from, when, and any context worth keeping.
///
/// Every name carries one, recorded by whatever did the work at the time (D213).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Derivation {
    /// How it was arrived at.
    #[serde(flatten)]
    pub method: Method,
    /// The day it was recorded, as `YYYY-MM-DD`.
    ///
    /// Day resolution: the record says when a name entered the tree, not to the second.
    pub on: String,
    /// Anything a reader would want and cannot reconstruct.
    ///
    /// Which title, which probe, what the guest was doing. Free text, because the cases
    /// worth noting are the ones a schema would not anticipate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Derivation {
    /// Records a method with a date and no note.
    pub fn new(method: Method, on: impl Into<String>) -> Self {
        Self {
            method,
            on: on.into(),
            note: None,
        }
    }

    /// Adds context.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// How a name was arrived at.
///
/// Every variant records how a candidate was proposed. Confirmation is the same in every
/// case: the candidate's hash equals one a real module imports (`docs/PROVENANCE.md`).
/// [`Evidence`] (what material the candidate came from) and [`Reproducible`] (what someone
/// else needs to repeat it) are derived from the variant rather than stored, so a record
/// cannot claim a tier its method does not support (D213).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "found", rename_all = "kebab-case")]
pub enum Method {
    /// A name published by ISO C or POSIX, taken from the standard-library word list.
    ///
    /// These names are fixed by public standards, and the shipped list is the whole of what
    /// was tried.
    PublishedStandard {
        /// Which shipped list it came from.
        list: String,
    },
    /// A name the generator built, and exactly where.
    ///
    /// `pattern` and `index` together identify one candidate; re-running that pattern at
    /// that index reproduces the name without a full sweep.
    Generated {
        /// The pattern in the grammar file.
        pattern: String,
        /// Its index within that pattern.
        index: u64,
    },
    /// A name derived from a name this project already held, by a stated rule.
    ///
    /// The seed is a whole name already proved correct and the rule is one line: `snprintf`
    /// becomes `snprintf_s`, `getpeername` becomes `_getpeername`. Rechecking it means
    /// applying one rule to one string. The record claims only that the hash agrees, not why
    /// the platform exports the variant (D606).
    Affixed {
        /// The name it was derived from, which must itself be a name this project holds.
        seed: String,
        /// The rule applied, as spelled in the affix file.
        rule: String,
    },
    /// A name read out of guest material at rest, with nothing executed.
    ///
    /// Deterministic: the same module yields the same candidates every time, so anyone
    /// holding the same title reproduces it.
    Static {
        /// Which static harvester proposed it.
        by: StaticSource,
        /// The module it was read out of, as a path.
        from: String,
    },
    /// A name learned from something actually executing.
    ///
    /// Reproducible only by running the same thing again: a guest is not obliged to reach
    /// the same place twice.
    Runtime {
        /// Which runtime harvester proposed it.
        by: RuntimeSource,
        /// What was run, in enough detail to repeat it.
        how: String,
    },
    /// A name that came from outside this project.
    ///
    /// Recorded distinctly and never folded in with the rest, so an audit can tell what
    /// this repository did not derive.
    Supplied {
        /// Where it came from.
        source: String,
    },
}

/// Which static harvester proposed a candidate.
///
/// A closed set, so records are countable by mechanism (D213). A new mechanism adds a
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StaticSource {
    /// Identifier-shaped runs of bytes in the module's own data.
    ///
    /// Diagnostic format strings and assertion text leave real function names in a binary.
    ModuleStrings,
    /// A string harvested from one module that named an import of a different one.
    ///
    /// The same mechanism as [`StaticSource::ModuleStrings`], pooled across a corpus: the
    /// name was in material the module needing it does not contain.
    CrossModule,
    /// The firmware layout, reached through an address a hardware export table gave.
    ///
    /// A conformance probe enumerates the kernel export table as hash-to-address, and the
    /// firmware layout maps address-to-name. Together they propose a name the generator
    /// cannot reach, and the hash confirms it.
    FirmwareLayout,
}

/// Which runtime harvester proposed a candidate.
///
/// Closed for the same reason as [`StaticSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSource {
    /// Reasoning about what a real call trace showed, then confirmed by hash.
    ///
    /// A person read a trace, the trace narrowed the family, and the hash settled it. The
    /// `how` field carries the argument.
    CallTrace,
    /// Bytes read out of guest memory while it ran, because it passed them to a function.
    ///
    /// Memory captured by the dispatch path is post-relocation and can hold text no module
    /// contains as a literal, such as a path assembled at runtime.
    ArgumentDump,
    /// A name a conformance probe reported, running on real hardware.
    ///
    /// The one source this project cannot reproduce on its own machines, and the reason
    /// [`Reproducible`] has a tier above [`Reproducible::FromRun`].
    ProbeTranscript,
}

/// What kind of material a candidate came out of.
///
/// Derived from [`Method`] rather than stored, so it cannot disagree with its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Evidence {
    /// Built from inputs in this repository. Guest material only confirms it.
    Derived,
    /// Read out of guest material at rest.
    Static,
    /// Learned from something executing.
    Runtime,
    /// Came from outside this project.
    External,
}

impl Evidence {
    /// How it is written in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Derived => "derived",
            Self::Static => "static",
            Self::Runtime => "runtime",
            Self::External => "external",
        }
    }
}

/// What somebody else would need in order to arrive at the same name.
///
/// A string read out of a title is verifiable by anyone holding that title, so the tiers
/// say what is needed rather than sorting names into verified and not (D213). Ordered from
/// cheapest to check to most expensive, which is also how an audit prints them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reproducible {
    /// This repository, and nothing else. What CI can check on every commit.
    FromRepository,
    /// This repository and the same guest module. Deterministic; the module is never in
    /// the repository (`docs/SCOPE.md`).
    FromModule,
    /// This repository, the same module, and a run of it.
    FromRun,
    /// Hardware this project does not own and CI cannot have.
    FromHardware,
    /// Nothing here reproduces it; only its original source does.
    OnlyFromItsSource,
}

impl Reproducible {
    /// How it is written in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FromRepository => "from this repository",
            Self::FromModule => "from this repository and the module",
            Self::FromRun => "from this repository and a run of the module",
            Self::FromHardware => "from this repository and real hardware",
            Self::OnlyFromItsSource => "only from where it came from",
        }
    }

    /// How much this tier is worth, lowest first.
    ///
    /// Ordered by what a reader needs, not by how the name was found: a name re-derivable
    /// from this repository alone outranks the same name harvested from a module, because
    /// CI checks the first on every commit. A name settled by several sources in one run
    /// keeps the highest rank.
    pub const fn rank(self) -> u8 {
        match self {
            Self::FromRepository => 0,
            Self::FromModule => 1,
            Self::FromRun => 2,
            Self::FromHardware => 3,
            Self::OnlyFromItsSource => 4,
        }
    }
}

impl Method {
    /// What kind of material proposed this candidate.
    pub const fn evidence(&self) -> Evidence {
        match self {
            Self::PublishedStandard { .. } | Self::Generated { .. } | Self::Affixed { .. } => {
                Evidence::Derived
            }
            Self::Static { .. } => Evidence::Static,
            Self::Runtime { .. } => Evidence::Runtime,
            Self::Supplied { .. } => Evidence::External,
        }
    }

    /// What somebody else would need in order to arrive at the same name.
    pub const fn reproducible(&self) -> Reproducible {
        match self {
            Self::PublishedStandard { .. } | Self::Generated { .. } | Self::Affixed { .. } => {
                Reproducible::FromRepository
            }
            Self::Static { .. } => Reproducible::FromModule,
            // A probe transcript escapes its runtime tier: the hardware it came from is not
            // something this repository can provide.
            Self::Runtime {
                by: RuntimeSource::ProbeTranscript,
                ..
            } => Reproducible::FromHardware,
            Self::Runtime { .. } => Reproducible::FromRun,
            Self::Supplied { .. } => Reproducible::OnlyFromItsSource,
        }
    }

    /// Whether this claim can be rechecked mechanically, with no trust involved.
    ///
    /// True only for the tier that needs nothing but this repository, because that is the
    /// only tier CI re-runs.
    pub const fn is_mechanically_checkable(&self) -> bool {
        matches!(self.reproducible(), Reproducible::FromRepository)
    }

    /// Whether the name was worked out by this project, by any route.
    pub const fn is_our_own_work(&self) -> bool {
        !matches!(self, Self::Supplied { .. })
    }
}

/// Today, as `YYYY-MM-DD`, for stamping a derivation.
///
/// Hand-rolled from the civil-calendar algorithm rather than taking a date crate with time
/// zones and parsing for one function.
pub fn today() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    date_of(now)
}

/// A timestamp as `YYYY-MM-DD`.
pub fn date_of(unix_seconds: u64) -> String {
    let (year, month, day) = civil_from_days((unix_seconds / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A timestamp as `YYYY-MM-DD HH:MM`, in UTC.
///
/// Shares `civil_from_days` with [`today`] for the same reason. UTC, so two build stamps
/// are always comparable.
pub fn timestamp_of(unix_seconds: u64) -> String {
    let seconds_today = unix_seconds % 86_400;
    format!(
        "{} {:02}:{:02}",
        date_of(unix_seconds),
        seconds_today / 3600,
        (seconds_today % 3600) / 60
    )
}

/// Converts days since the Unix epoch to a calendar date.
///
/// Howard Hinnant's public-domain `civil_from_days`. It shifts the year to start in March
/// so the leap day lands at the end, which removes every special case.
const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

impl SymbolDbFile {
    /// Parses a database from JSON.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The hash suffix, decoded from its hex form.
    ///
    /// Returns `None` for malformed hex rather than a partial suffix, so a bad file reads as
    /// an error and not as "no names known".
    pub fn suffix(&self) -> Option<Vec<u8>> {
        let text = self.suffix_hex.trim();
        if text.len() % 2 != 0 {
            return None;
        }
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
            .collect()
    }
}

impl SymbolDb {
    /// Builds a database from a parsed file, deriving every NID from its name.
    ///
    /// Returns `None` if the file's suffix is malformed.
    pub fn from_file(file: &SymbolDbFile) -> Option<(Self, NidHasher)> {
        let hasher = NidHasher::new(file.suffix()?);
        let db = Self::from_names(&hasher, &file.names);
        Some((db, hasher))
    }

    /// How many of `nids` this database can name.
    ///
    /// A name list and a suffix are correct exactly to the extent that they explain hashes
    /// a real module imports (D068).
    pub fn explains(&self, nids: impl IntoIterator<Item = Nid>) -> usize {
        nids.into_iter().filter(|n| self.name(*n).is_some()).count()
    }

    /// Builds a database by hashing every known name with `hasher`.
    pub fn from_names<I, S>(hasher: &NidHasher, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let by_nid = names
            .into_iter()
            .map(|n| {
                let n = n.as_ref();
                (hasher.hash(n), n.to_owned())
            })
            .collect();
        Self { by_nid }
    }

    /// The name for `nid`, if known.
    pub fn name(&self, nid: Nid) -> Option<&str> {
        self.by_nid.get(&nid).map(String::as_str)
    }

    /// Every name it holds, in no particular order.
    ///
    /// For a search that derives candidates from proved names. Unordered because the map is;
    /// a caller that needs a stable list sorts it.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_nid.values().map(String::as_str)
    }
    /// How many names are known.
    pub fn len(&self) -> usize {
        self.by_nid.len()
    }

    /// Whether the database is empty.
    pub fn is_empty(&self) -> bool {
        self.by_nid.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use selfish_nid::ENCODED_LEN as ENCODED_NID_LEN;

    use super::{
        Nid, NidHasher, SymbolDb, SymbolDbFile, decode_hex, decode_nid, decode_symbol_name,
        default_suffix, encode_nid,
    };

    /// Timestamps render to fixed known instants in UTC.
    #[test]
    fn a_timestamp_renders_as_a_date_and_a_clock() {
        // Pinned against known instants rather than round-tripped, so two readers agree
        // about what a stamp means.
        assert_eq!(super::timestamp_of(0), "1970-01-01 00:00");
        assert_eq!(super::date_of(0), "1970-01-01");
        // Past the 2000 leap-year special case.
        assert_eq!(super::timestamp_of(1_787_607_600), "2026-08-24 21:40");
        // A minute before midnight.
        assert_eq!(super::timestamp_of(86_399), "1970-01-01 23:59");
        // `today` shares the function underneath.
        assert_eq!(super::today(), super::date_of(now_unix()));
    }

    /// The clock, for the one test that compares against it.
    fn now_unix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// An encoded symbol name decodes to its NID, library id and module id.
    #[test]
    fn an_encoded_symbol_name_decodes_to_a_nid_and_two_ids() {
        let got = decode_symbol_name("H2e8t5ScQGc#B#C").expect("valid encoded import");
        assert_eq!(got.nid.as_raw(), 0x6740_9c94_b7bc_671f);
        assert_eq!(got.library_id, 1, "B is index 1");
        assert_eq!(got.module_id, 2, "C is index 2");
    }

    /// Decoding agrees with an independent implementation of the same rule.
    #[test]
    fn decoding_matches_an_independent_implementation() {
        // Cross-checked against a separate implementation, so a transcription slip in
        // either shows up.
        for (name, expected) in [
            ("H2e8t5ScQGc#B#C", 0x6740_9c94_b7bc_671f_u64),
            ("ZT4ODD2Ts9o#B#C", 0xdab3_933d_0c0e_3e65),
            ("f7uOxY9mM1U#A#B", 0x5533_668f_c58e_bb7f),
        ] {
            assert_eq!(
                decode_symbol_name(name).expect("valid").nid.as_raw(),
                expected,
                "{name}"
            );
        }
    }

    /// A name that is not an encoded import decodes to `None`.
    #[test]
    fn ordinary_symbol_names_are_not_imports_and_that_is_not_an_error() {
        // `None` rather than an error lets a symbol walk skip ordinary names.
        for name in [
            "main",
            "_init",
            "H2e8t5ScQGc",       // no separators
            "H2e8t5ScQGc#B",     // one separator
            "H2e8t5ScQGc#B#C#D", // too many
            "short#B#C",         // wrong encoded length
            "H2e8t5ScQG!#B#C",   // character outside the alphabet
        ] {
            assert!(
                decode_symbol_name(name).is_none(),
                "{name} should not decode"
            );
        }
    }

    /// An encoded NID is exactly eleven characters.
    #[test]
    fn the_encoded_length_is_what_carries_a_64_bit_value() {
        // Eleven characters is 66 bits, two of them padding. Any other length cannot be a
        // NID.
        assert_eq!(ENCODED_NID_LEN, 11);
        assert!(decode_symbol_name(&format!("{}#B#C", "A".repeat(10))).is_none());
        assert!(decode_symbol_name(&format!("{}#B#C", "A".repeat(12))).is_none());
        assert!(decode_symbol_name(&format!("{}#B#C", "A".repeat(11))).is_some());
    }

    /// Library and module ids decode as base64 integers.
    #[test]
    fn library_and_module_ids_decode_as_base64_integers() {
        let got = decode_symbol_name("AAAAAAAAAAA#BA#CB").expect("valid");
        assert_eq!(got.library_id, 64, "BA is 1*64 + 0");
        assert_eq!(got.module_id, 128 + 1, "CB is 2*64 + 1");
    }

    /// Hashing is deterministic and depends on the suffix.
    #[test]
    fn hashing_is_stable_and_suffix_sensitive() {
        let a = NidHasher::new(*b"\x01\x02\x03\x04");
        let b = NidHasher::new(*b"\x05\x06\x07\x08");

        // Same input, same hasher: identical.
        assert_eq!(a.hash("sceAudioOutInit"), a.hash("sceAudioOutInit"));
        // A different suffix gives a different hash.
        assert_ne!(a.hash("sceAudioOutInit"), b.hash("sceAudioOutInit"));
        // Different names do not collide.
        assert_ne!(a.hash("sceAudioOutInit"), a.hash("sceAudioOutOpen"));
    }

    /// A database file round-trips and derives its NIDs from its names.
    #[test]
    fn a_database_file_round_trips_and_derives_its_own_hashes() {
        let json = r#"{"suffix_hex":"0102feff","names":["sceAudioOutInit","sceAudioOutOpen"]}"#;
        let file = SymbolDbFile::from_json(json).expect("parses");
        assert_eq!(
            file.suffix().expect("valid hex"),
            vec![0x01, 0x02, 0xfe, 0xff]
        );

        let (db, hasher) = SymbolDb::from_file(&file).expect("valid file");
        assert_eq!(db.len(), 2);
        // NIDs are derived, never stored, so the file cannot disagree with itself.
        assert_eq!(
            db.name(hasher.hash("sceAudioOutInit")),
            Some("sceAudioOutInit")
        );
    }

    /// A malformed suffix is rejected, not truncated.
    #[test]
    fn a_malformed_suffix_is_rejected_rather_than_silently_truncated() {
        // A suffix that lost a byte would read as "no names known" rather than as a bad
        // file.
        for bad in ["abc", "zz", "0102fe0"] {
            let file = SymbolDbFile {
                suffix_hex: bad.to_owned(),
                names: vec!["x".to_owned()],
                derivations: BTreeMap::new(),
            };
            assert!(file.suffix().is_none(), "{bad} should be rejected");
            assert!(SymbolDb::from_file(&file).is_none());
        }
    }

    /// `explains` counts the real hashes a name list accounts for.
    #[test]
    fn explains_measures_a_name_list_against_real_hashes() {
        // A name list and a suffix are correct exactly to the extent that they explain
        // hashes a real module imports.
        let hasher = NidHasher::new(*b"salt");
        let db = SymbolDb::from_names(&hasher, ["known_a", "known_b"]);

        let observed = [
            hasher.hash("known_a"),
            hasher.hash("known_b"),
            hasher.hash("never_guessed"),
        ];
        assert_eq!(
            db.explains(observed),
            2,
            "two of three hashes are explained"
        );

        // A wrong suffix explains nothing.
        let wrong = SymbolDb::from_names(&NidHasher::new(*b"different"), ["known_a", "known_b"]);
        assert_eq!(wrong.explains(observed), 0);
    }

    /// Reverse lookup names a known NID and returns `None` for an unknown one.
    #[test]
    fn reverse_lookup_resolves_known_and_admits_unknown() {
        let hasher = NidHasher::new(*b"salt");
        let db = SymbolDb::from_names(&hasher, ["sceAudioOutInit", "sceAudioOutOpen"]);

        assert_eq!(
            db.name(hasher.hash("sceAudioOutInit")),
            Some("sceAudioOutInit")
        );
        // An unknown name resolves to nothing: no panic, no fabricated name.
        assert_eq!(db.name(hasher.hash("sceNeverHeardOfIt")), None);
        assert_eq!(db.len(), 2);
    }
    /// The shipped suffix is valid hex of the expected length.
    #[test]
    fn the_shipped_suffix_is_valid_and_the_expected_length() {
        // Embedded, so a typo in the data file would break import resolution for every
        // user.
        let suffix = default_suffix();
        assert_eq!(suffix.len(), 16, "the suffix is sixteen bytes");
        assert!(
            suffix.iter().any(|b| *b != 0),
            "an all-zero suffix means the file failed to parse into anything real"
        );
        assert_eq!(NidHasher::default().suffix_len(), suffix.len());
    }

    /// A hash decodes back to itself through an encoded symbol name.
    #[test]
    fn a_hash_survives_the_round_trip_through_a_symbol_name() {
        // A hasher and a decoder that disagree about byte order are each self-consistent;
        // only a round trip through both catches it.
        let hasher = NidHasher::default();
        for name in ["memcpy", "sceKernelAllocateDirectMemory", "a", ""] {
            let nid = hasher.hash(name);
            let encoded = encode_nid(nid);
            assert_eq!(encoded.len(), ENCODED_NID_LEN, "for {name}");
            let decoded = decode_symbol_name(&format!("{encoded}#A#A"))
                .unwrap_or_else(|| panic!("{encoded} should decode"));
            assert_eq!(decoded.nid, nid, "round trip failed for {name}");
        }
    }

    /// A bare eleven-character NID decodes without library and module ids.
    #[test]
    fn a_bare_encoded_hash_decodes_without_a_library_and_module_beside_it() {
        // The form a hardware export table reports: eleven characters and an address, with
        // no `#B#C` for `decode_symbol_name` to read.
        let hasher = NidHasher::default();
        for name in [
            "memcpy",
            "sceKernelAllocateDirectMemory",
            "_ZNSt6_WinitC1Ev",
        ] {
            let nid = hasher.hash(name);
            let encoded = encode_nid(nid);
            assert_eq!(
                decode_nid(&encoded),
                Some(nid),
                "bare decode disagreed for {name}"
            );
            // It agrees with the decoder that reads a whole symbol name.
            assert_eq!(
                decode_nid(&encoded),
                decode_symbol_name(&format!("{encoded}#A#A")).map(|i| i.nid),
                "the two decoders disagreed for {name}"
            );
        }
    }

    /// A bare NID of the wrong length or alphabet is refused.
    #[test]
    fn a_bare_hash_of_the_wrong_length_or_alphabet_is_refused() {
        // Ten characters decode to a plausible number that agrees with nothing, so the
        // refusal is what is asserted.
        assert!(
            decode_nid(&"A".repeat(ENCODED_NID_LEN - 1)).is_none(),
            "short"
        );
        assert!(
            decode_nid(&"A".repeat(ENCODED_NID_LEN + 1)).is_none(),
            "long"
        );
        assert!(decode_nid("").is_none(), "empty");
        assert!(
            decode_nid(&format!("{}#", "A".repeat(ENCODED_NID_LEN - 1))).is_none(),
            "a separator is not in the alphabet"
        );
        assert!(
            decode_nid(&format!("{}=", "A".repeat(ENCODED_NID_LEN - 1))).is_none(),
            "base64 padding is not in this alphabet"
        );
    }

    /// Encoding inverts decoding for any 64-bit value.
    #[test]
    fn encoding_is_the_exact_inverse_of_decoding_for_arbitrary_values() {
        // Including the extremes, where a sign or padding mistake shows up.
        for raw in [0, 1, u64::MAX, 0x92f5_7c2d_c704_346f, 0x0123_4567_89ab_cdef] {
            let nid = Nid::from_raw(raw);
            let encoded = encode_nid(nid);
            let decoded = decode_symbol_name(&format!("{encoded}#A#A"))
                .expect("should decode")
                .nid;
            assert_eq!(decoded, nid, "for {raw:#x}");
        }
    }

    /// Odd-length or non-hex input is refused, not partially decoded.
    #[test]
    fn odd_length_or_non_hex_is_refused_rather_than_partially_decoded() {
        // A suffix missing a byte hashes to plausible values that match nothing.
        assert!(decode_hex("abc").is_none(), "odd length");
        assert!(decode_hex("zz").is_none(), "not hex");
        assert_eq!(decode_hex("00ff").as_deref(), Some(&[0, 255][..]));
        assert_eq!(decode_hex(""), Some(Vec::new()));
    }
}
