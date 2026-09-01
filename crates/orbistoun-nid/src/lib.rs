//! NID hashing and symbol-name resolution.
//!
//! Guest modules do not import by name. A guest module's dynamic table
//! references a library plus a **NID**: a 64-bit hash derived from the symbol
//! name, encoded in a custom base64 alphabet. Resolving an import therefore has
//! two halves, and this crate owns both:
//!
//! 1. **Forward**: name -> NID, so a known symbol can be matched against what a
//!    module actually asks for. This is [`NidHasher`].
//! 2. **Reverse**: NID -> name, which is only possible by lookup. A hash is not
//!    invertible, so unknown NIDs stay unknown. This is [`SymbolDb`].
//!
//! # Why the hash suffix is data, not a constant
//!
//! The algorithm appends a fixed byte suffix to the symbol name before hashing.
//! That suffix is a publicly documented constant from console
//! reverse-engineering work, but it is deliberately **not** baked into this
//! source: it is supplied at construction from the same file as the symbol
//! database. Two reasons - it keeps a magic constant out of the source tree, and
//! it makes the hasher testable against any suffix without a recompile.
//!
//! See `docs/SYMBOLS.md` for the expected file format and where to obtain it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

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

impl std::fmt::Display for Nid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#018x}", self.0)
    }
}

/// Alphabet used to encode a NID inside a dynamic symbol name.
///
/// Standard base64 ordering with `+` and `-` as the final two characters.
pub const NID_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-";

/// Characters of encoded NID that precede the `#` separators.
pub const ENCODED_NID_LEN: usize = 11;

/// An import as it appears in a dynamic symbol name.
///
/// Real symbol names take the form `H2e8t5ScQGc#B#C`: an encoded NID, then a library
/// id, then a module id, both small base64-encoded integers indexing the dynamic
/// table's library and module entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodedImport {
    /// The hash, decoded.
    pub nid: Nid,
    /// Library index within the module's import list.
    pub library_id: u16,
    /// Module index.
    pub module_id: u16,
}

/// Decodes one base64 character to its 6-bit value.
fn b64_value(c: u8) -> Option<u8> {
    NID_ALPHABET
        .iter()
        .position(|a| *a == c)
        .and_then(|p| u8::try_from(p).ok())
}

/// Decodes a small base64-encoded integer, as used for library and module ids.
fn decode_small(text: &str) -> Option<u16> {
    let mut value: u32 = 0;
    for c in text.bytes() {
        value = value
            .checked_mul(64)?
            .checked_add(u32::from(b64_value(c)?))?;
    }
    u16::try_from(value).ok()
}

/// Decodes a dynamic symbol name of the form `<nid>#<library>#<module>`.
///
/// Returns `None` for any name that is not in that form - ordinary symbol names exist
/// too, and a name that does not encode an import is not an error.
///
/// # Byte order
///
/// The eleven encoded characters carry 66 bits, of which the low two are padding. The
/// The eight bytes are byte-swapped into the order [`NidHasher`] produces. This is
/// **independently verified**: hashing published C names with the shipped suffix
/// matches dozens of real imports in a real executable, and any other combination of
/// byte order matches none (D070).
pub fn decode_symbol_name(name: &str) -> Option<EncodedImport> {
    let mut parts = name.split('#');
    let encoded = parts.next()?;
    let library = parts.next()?;
    let module = parts.next()?;
    if parts.next().is_some() || encoded.len() != ENCODED_NID_LEN {
        return None;
    }

    let mut bits: u128 = 0;
    for c in encoded.bytes() {
        bits = (bits << 6) | u128::from(b64_value(c)?);
    }
    // 11 characters is 66 bits; the low two are padding.
    let value = (bits >> 2) as u64;

    Some(EncodedImport {
        nid: Nid::from_raw(value.swap_bytes()),
        library_id: decode_small(library)?,
        module_id: decode_small(module)?,
    })
}

/// The suffix orbistoun uses unless told otherwise, with its documentation.
///
/// Embedded rather than read from disk so a portable single-binary build carries it
/// with nothing to lose. See the file itself for what the value is, what it is not, and
/// how it verifies itself.
const HASH_SUFFIX_FILE: &str = include_str!("../data/hash-suffix.toml");

/// The default hash suffix, decoded from the shipped data file.
///
/// Every emulator of this target necessarily contains this value - resolving imports is
/// the central act of high-level emulation - so requiring a user to supply it would add
/// a setup step that protects nothing (D071).
///
/// # Panics
///
/// If the shipped file is malformed, which a test in this crate rules out.
pub fn default_suffix() -> Vec<u8> {
    let line = HASH_SUFFIX_FILE
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("suffix_hex"))
        .expect("the shipped suffix file must define suffix_hex");
    let hex = line
        .split('"')
        .nth(1)
        .expect("suffix_hex must be a quoted string");
    decode_hex(hex).expect("the shipped suffix must be valid hex")
}

/// Decodes an even-length hex string.
///
/// Returns `None` on an odd length or a non-hex digit, rather than skipping the bad
/// character - a suffix silently missing a byte hashes to something plausible and
/// matches nothing, which is the hardest kind of wrong to notice.
pub fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

/// Encodes a NID into the eleven-character form a symbol name carries.
///
/// The exact inverse of [`decode_symbol_name`]'s first field, and it exists mainly so
/// that inverse can be **tested**. A hasher and a decoder that disagree about byte
/// order are each perfectly self-consistent, so nothing catches the disagreement until
/// something checks that a hash survives the round trip (D070).
pub fn encode_nid(nid: Nid) -> String {
    // Undo the swap the decoder applies, then re-add the two padding bits that make
    // 64 bits into the 66 that eleven six-bit characters carry.
    let bits = u128::from(nid.as_raw().swap_bytes()) << 2;
    (0..ENCODED_NID_LEN)
        .map(|i| {
            let shift = 6 * (ENCODED_NID_LEN - 1 - i);
            let index = ((bits >> shift) & 0x3F) as usize;
            NID_ALPHABET[index] as char
        })
        .collect()
}

/// Renders bytes as lowercase hex.
///
/// The inverse of [`decode_hex`], so a suffix can be written back into a database file
/// exactly as it will be read out of one.
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
/// [`NidHasher::default`] uses the suffix orbistoun ships with, which is what a caller
/// wants unless it is deliberately testing something else. Cheap to clone; hold one per
/// loader.
#[derive(Debug, Clone)]
pub struct NidHasher {
    suffix: Vec<u8>,
}

impl NidHasher {
    /// How many bytes the suffix has. For tests and diagnostics.
    pub fn suffix_len(&self) -> usize {
        self.suffix.len()
    }
}

impl Default for NidHasher {
    fn default() -> Self {
        Self::new(default_suffix())
    }
}

impl NidHasher {
    /// Creates a hasher using `suffix` as the trailing bytes appended to each
    /// symbol name before hashing.
    ///
    /// An empty suffix is accepted (and useful in tests) but will not match real
    /// guest imports.
    pub fn new(suffix: impl Into<Vec<u8>>) -> Self {
        Self {
            suffix: suffix.into(),
        }
    }

    /// Hashes `name` to the NID a guest module would import it by.
    ///
    /// The first eight bytes of the SHA-1 digest, read little-endian - matching
    /// how the encoded form is unpacked from an import table.
    pub fn hash(&self, name: &str) -> Nid {
        self.hash_bytes(name.as_bytes())
    }

    /// Hashes a name already held as bytes.
    ///
    /// The form a brute-force search wants. Turning each candidate into a `String`
    /// first allocates once per candidate, and a search that tests billions of them
    /// spends more time in the allocator than in SHA-1 - so the caller builds names in
    /// a buffer it reuses and passes the bytes straight through.
    ///
    /// A symbol name is bytes to the hash; validity as UTF-8 is the caller's business
    /// and never affects the result.
    ///
    /// # Byte order
    ///
    /// **Big-endian**, and this was wrong for a long time. Reading the digest
    /// little-endian produces a perfectly plausible hash that agrees with nothing, and
    /// nothing caught it because every test hashed with an arbitrary suffix and compared
    /// against its own output - self-consistent and self-consistently wrong. What
    /// exposed it was hashing published C names against a real import table, where the
    /// right order matches dozens and the wrong order matches none (D070).
    pub fn hash_bytes(&self, name: &[u8]) -> Nid {
        let mut h = Sha1::new();
        h.update(name);
        h.update(&self.suffix);
        let digest = h.finalize();

        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        Nid(u64::from_be_bytes(bytes))
    }
}

/// Maps NIDs back to symbol names.
///
/// Populated from a symbol-database file. Lookups that miss are the normal case
/// early on and must be handled, not treated as an error: an unknown NID means
/// "a function we have no name for yet", which is still perfectly reportable in
/// an import dump.
#[derive(Debug, Clone, Default)]
pub struct SymbolDb {
    by_nid: HashMap<Nid, String>,
}

/// The on-disk shape of a symbol database.
///
/// One file carries both the hash suffix and the known names, so a single input
/// fully determines resolution behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDbFile {
    /// Hex-encoded byte suffix appended to a name before hashing.
    pub suffix_hex: String,
    /// Known symbol names. NIDs are derived, not stored, so the file cannot
    /// disagree with itself.
    pub names: Vec<String>,
    /// How each name was arrived at, keyed by name.
    ///
    /// **The provenance record.** A name is only as defensible as the account of where
    /// it came from, and "we brute-forced it" is a claim until something can check it.
    /// Each entry says which of this repository's own inputs produced the name and
    /// where in them - so anyone can re-derive it, in isolation, in microseconds.
    ///
    /// Optional and separate from `names`, so a database from elsewhere still loads.
    /// A name with no entry here is not an error; it is simply unaccounted for, which
    /// is exactly what an audit should surface (D073).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub derivations: std::collections::BTreeMap<String, Derivation>,
}

impl SymbolDbFile {
    /// The database that ships with the tool.
    ///
    /// # Why this is loaded unless told otherwise
    ///
    /// It was not, and every run reported hashes it could already name. `printf` and
    /// `memalign` were both in this file while the corpus reports listed them as
    /// "has no name" and told the reader to go and extend the vocabulary - work already
    /// done, in a file already committed, that nothing loaded (D188).
    ///
    /// That is worse than a missing feature. The findings are the output this project is
    /// *for*, and they were confidently recommending the wrong next action - which is the
    /// same failure as a stub that reports success, one layer up.
    ///
    /// Embedded rather than read from disk so a portable build and an installed one behave
    /// the same, matching how the knowledge files ship. `--symbols-db` still overrides it,
    /// because a database under construction has to be testable before it is committed.
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
/// Read from `symbols/` at the workspace root rather than copied into this crate, so there
/// is one file and it cannot drift from the one CI audits. The cost is that this crate no
/// longer builds outside its workspace, which is a cost worth paying here: nothing in this
/// project is published as a standalone crate, and two copies of a symbol database is
/// precisely the shape that ends with the audited one and the loaded one disagreeing.
const EMBEDDED_DB: &str = include_str!("../../../symbols/generated.json");

/// Where a name came from, when, and any context worth keeping.
///
/// **Every name carries one.** A hash-to-name mapping is the one artefact here somebody
/// could reasonably ask hard questions about, and "we worked it out ourselves" is a
/// claim like any other unless something records *how* - at the time, by whatever did
/// the work (D073).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Derivation {
    /// How it was arrived at.
    #[serde(flatten)]
    pub method: Method,
    /// The day it was recorded, as `YYYY-MM-DD`.
    ///
    /// Coarse on purpose. The point is to say roughly when a name entered the tree, not
    /// to timestamp it to the second.
    pub on: String,
    /// Anything a reader would want and cannot reconstruct.
    ///
    /// Which title, which probe, what the guest was doing. Free text, because the
    /// interesting cases are the ones a schema would not have anticipated.
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
/// # Two questions, and they are not the same question
///
/// Every variant here records **how a candidate was proposed**. None of them records how
/// it was *confirmed*, because confirmation is the same act in every case: the candidate
/// is hashed, and the hash either equals one a real module declares it needs or it does
/// not. There is one oracle and it is arithmetic (`docs/PROVENANCE.md`).
///
/// So the interesting axes are what kind of material the candidate came out of - see
/// [`Evidence`] - and what somebody else would have to hold in order to do it again - see
/// [`Reproducible`]. Both are derived from the variant rather than stored, so a record
/// cannot claim a tier its own method does not support.
///
/// # Why `observed` is not one of these any more
///
/// It used to be, and it covered two unrelated things: reading a literal string out of a
/// file at rest, and learning something from a guest actually executing. Its own
/// documentation said "watching something run" while 137 of the 154 names carrying it had
/// never run anything. A vocabulary that cannot tell those apart cannot answer the
/// question it exists for (D213).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "found", rename_all = "kebab-case")]
pub enum Method {
    /// A name published by ISO C or POSIX, taken from the standard-library word list.
    ///
    /// Not a guess at all: these names are fixed by public standards, and the list
    /// shipped in this repository is the whole of what was tried.
    PublishedStandard {
        /// Which shipped list it came from.
        list: String,
    },
    /// A name the generator built, and exactly where.
    ///
    /// `pattern` and `index` together identify one candidate out of hundreds of
    /// millions. Re-running that single pattern at that single index reproduces the
    /// name, so the claim costs a microsecond to check rather than a full sweep.
    Generated {
        /// The pattern in the grammar file.
        pattern: String,
        /// Its index within that pattern.
        index: u64,
    },
    /// A name read out of guest material at rest. **Nothing was executed.**
    ///
    /// The candidate was already lying in a file this project parses anyway. That makes
    /// it deterministic - the same module yields the same candidates every time - and it
    /// makes it reproducible by anyone holding the same title, which is a materially
    /// stronger claim than the old `observed` was able to make.
    Static {
        /// Which static harvester proposed it.
        by: StaticSource,
        /// The module it was read out of, as a path.
        from: String,
    },
    /// A name learned from something actually executing.
    ///
    /// Reproducible by running the same thing again, and no more precisely than that: a
    /// guest is not obliged to reach the same place twice, so this tier says "do what we
    /// did" rather than "evaluate this index".
    Runtime {
        /// Which runtime harvester proposed it.
        by: RuntimeSource,
        /// What was run, in enough detail to repeat it.
        how: String,
    },
    /// A name that came from outside this project.
    ///
    /// Recorded distinctly and never folded in with the rest. This is the variant that
    /// says "this repository did not derive this", which is the honest thing for it to
    /// say - and it is why an audit can be trusted at all.
    Supplied {
        /// Where it came from.
        source: String,
    },
}

/// Which static harvester proposed a candidate.
///
/// Closed on purpose. The failure the old free-text `how` field allowed was 137 records
/// describing one mechanism in several different sentences, with nothing able to count
/// them (D213). A new mechanism adds a variant here; it does not add a new sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StaticSource {
    /// Identifier-shaped runs of bytes in the module's own data (D193).
    ///
    /// Diagnostic format strings and assertion text leave real function names in a
    /// binary. The candidate is the vendor's own spelling, not a guess at it.
    ModuleStrings,
    /// A string harvested from one module that named an import of a different one.
    ///
    /// The same mechanism as [`StaticSource::ModuleStrings`], pooled across a corpus.
    /// Recorded apart because it answers a question the per-module form cannot: the name
    /// was in material the module needing it does not contain.
    CrossModule,
}

/// Which runtime harvester proposed a candidate.
///
/// Closed for the same reason as [`StaticSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSource {
    /// Reasoning about what a real call trace showed, then confirmed by hash.
    ///
    /// Not automated and not pretending to be: a person read a trace, the trace narrowed
    /// the family, and the hash settled it. The `how` field carries the argument.
    CallTrace,
    /// Bytes read out of guest memory while it ran, because it passed them to a function.
    ///
    /// The dispatch path already captures what a pointer argument points at. That memory
    /// is post-relocation and can hold text no module contains as a literal - a path
    /// assembled at runtime, a name read out of a data file.
    ArgumentDump,
    /// A name a conformance probe reported, running on real hardware.
    ///
    /// The only source here that this project cannot reproduce on its own machines, and
    /// the reason [`Reproducible`] has a tier above [`Reproducible::FromRun`]. obSCEne is
    /// ours and its transcripts are ours; the console is not something CI has.
    ProbeTranscript,
}

/// What kind of material a candidate came out of.
///
/// The axis the old vocabulary could not express. Derived from [`Method`] rather than
/// stored, so it cannot disagree with the record it describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Evidence {
    /// Built from inputs that live in this repository. No guest material involved in
    /// proposing it - only in confirming it.
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
/// # Why this is a tier rather than a boolean
///
/// The audit used to sort names into "re-derived" and "documented, not verified", and the
/// second bucket was doing far too much work. A string read out of a title is not
/// unverifiable - it is verifiable by anyone holding that title, deterministically, and
/// saying so is both truer and stronger than declining to classify it (D213).
///
/// Ordered from cheapest to check to most expensive, which is also how an audit prints
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reproducible {
    /// This repository, and nothing else. What CI can check on every commit.
    FromRepository,
    /// This repository and the same guest module. Deterministic, and the module is not
    /// here and never will be (`docs/SCOPE.md`).
    FromModule,
    /// This repository, the same module, and a run of it.
    FromRun,
    /// Hardware this project does not own and CI cannot have.
    FromHardware,
    /// Nothing here reproduces it. You would have to go back to wherever it came from.
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
}

impl Method {
    /// What kind of material proposed this candidate.
    pub const fn evidence(&self) -> Evidence {
        match self {
            Self::PublishedStandard { .. } | Self::Generated { .. } => Evidence::Derived,
            Self::Static { .. } => Evidence::Static,
            Self::Runtime { .. } => Evidence::Runtime,
            Self::Supplied { .. } => Evidence::External,
        }
    }

    /// What somebody else would need in order to arrive at the same name.
    pub const fn reproducible(&self) -> Reproducible {
        match self {
            Self::PublishedStandard { .. } | Self::Generated { .. } => Reproducible::FromRepository,
            Self::Static { .. } => Reproducible::FromModule,
            // A probe transcript is the one runtime source that escapes its own tier: the
            // evidence is ours, the console it came off is not something anybody here can
            // hand you.
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
    /// only tier CI can actually re-run. The rest are reproducible elsewhere and are
    /// reported as such rather than counted here, which is the distinction the whole
    /// mechanism exists to keep honest.
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
/// Hand-rolled from the civil-calendar algorithm rather than taking a date crate for
/// one function. Days-from-epoch to a calendar date is arithmetic, and a dependency
/// that pulls in time zones and parsing to do it is not worth the supply chain.
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
/// # Why this lives beside the date rather than in a date crate
///
/// The same reason [`today`] does: days-to-calendar is arithmetic, and a dependency that
/// brings time zones and parsing along to do it is not worth the supply chain. Adding the
/// clock is a division - it shares `civil_from_days` rather than repeating it, which is
/// the whole point of putting it here instead of wherever it was needed.
///
/// UTC, and unapologetically. A build stamp is compared against another build stamp, and a
/// local time that shifts twice a year makes two of them incomparable for no benefit.
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
/// Howard Hinnant's `civil_from_days`, which is public-domain arithmetic and correct
/// for any date in range. It shifts the year to start in March so the leap day lands at
/// the end, which is what removes every special case.
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
    /// Returns `None` for malformed hex rather than a partial suffix - a suffix that
    /// silently lost a byte would produce hashes that match nothing, and the failure
    /// would look like "no names known" rather than "your file is wrong".
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
    /// The self-verifying measure from D025: a name list and a suffix are correct
    /// exactly to the extent that they explain hashes a real module actually imports.
    /// No external authority is needed - a collision is the proof.
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

    use super::{
        ENCODED_NID_LEN, Nid, NidHasher, SymbolDb, SymbolDbFile, decode_hex, decode_symbol_name,
        default_suffix, encode_nid,
    };

    #[test]
    fn a_timestamp_renders_as_a_date_and_a_clock() {
        // Pinned against known instants rather than round-tripped, because the point of a
        // build stamp is that two people reading two of them agree about what they mean.
        assert_eq!(super::timestamp_of(0), "1970-01-01 00:00");
        assert_eq!(super::date_of(0), "1970-01-01");
        // 2026-08-24 21:40 UTC - a date past the 2000 leap-year special case, which is
        // where the civil arithmetic would show an error if it had one.
        assert_eq!(super::timestamp_of(1_787_607_600), "2026-08-24 21:40");
        // A minute before midnight, where an hours/minutes split goes wrong if it can.
        assert_eq!(super::timestamp_of(86_399), "1970-01-01 23:59");
        // `today` is the same function underneath, so it cannot drift from these.
        assert_eq!(super::today(), super::date_of(now_unix()));
    }

    /// The clock, for the one test that compares against it.
    fn now_unix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    #[test]
    fn an_encoded_symbol_name_decodes_to_a_nid_and_two_ids() {
        // The shape real dynamic symbol names take.
        let got = decode_symbol_name("H2e8t5ScQGc#B#C").expect("valid encoded import");
        assert_eq!(got.nid.as_raw(), 0x6740_9c94_b7bc_671f);
        assert_eq!(got.library_id, 1, "B is index 1");
        assert_eq!(got.module_id, 2, "C is index 2");
    }

    #[test]
    fn decoding_matches_an_independent_implementation() {
        // Cross-checked against a separate implementation of the same rule, so a
        // transcription slip in either shows up rather than being self-consistent.
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

    #[test]
    fn ordinary_symbol_names_are_not_imports_and_that_is_not_an_error() {
        // Plenty of names are just names. Returning None rather than erroring is what
        // lets a symbol walk skip them without special-casing.
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

    #[test]
    fn the_encoded_length_is_what_carries_a_64_bit_value() {
        // Eleven characters is 66 bits, of which two are padding. Any other length
        // cannot be a NID, which is why it is rejected rather than padded.
        assert_eq!(ENCODED_NID_LEN, 11);
        assert!(decode_symbol_name(&format!("{}#B#C", "A".repeat(10))).is_none());
        assert!(decode_symbol_name(&format!("{}#B#C", "A".repeat(12))).is_none());
        assert!(decode_symbol_name(&format!("{}#B#C", "A".repeat(11))).is_some());
    }

    #[test]
    fn library_and_module_ids_decode_as_base64_integers() {
        let got = decode_symbol_name("AAAAAAAAAAA#BA#CB").expect("valid");
        assert_eq!(got.library_id, 64, "BA is 1*64 + 0");
        assert_eq!(got.module_id, 128 + 1, "CB is 2*64 + 1");
    }

    #[test]
    fn hashing_is_stable_and_suffix_sensitive() {
        let a = NidHasher::new(*b"\x01\x02\x03\x04");
        let b = NidHasher::new(*b"\x05\x06\x07\x08");

        // Same input, same hasher: identical. This is the property the whole
        // import-resolution path depends on.
        assert_eq!(a.hash("sceAudioOutInit"), a.hash("sceAudioOutInit"));
        // Different suffix must give a different hash, or the suffix is not
        // actually participating.
        assert_ne!(a.hash("sceAudioOutInit"), b.hash("sceAudioOutInit"));
        // Different names must not collide on anything we test with.
        assert_ne!(a.hash("sceAudioOutInit"), a.hash("sceAudioOutOpen"));
    }

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

    #[test]
    fn a_malformed_suffix_is_rejected_rather_than_silently_truncated() {
        // A suffix that lost a byte produces hashes matching nothing, and the failure
        // would read as "no names known" rather than "your file is wrong".
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

    #[test]
    fn explains_measures_a_name_list_against_real_hashes() {
        // The self-verifying loop from D025: a name list and a suffix are correct
        // exactly to the extent that they explain hashes a real module imports. A
        // collision is the proof; no external authority is involved.
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

        // A wrong suffix explains nothing, which is exactly how a wrong guess reads.
        let wrong = SymbolDb::from_names(&NidHasher::new(*b"different"), ["known_a", "known_b"]);
        assert_eq!(wrong.explains(observed), 0);
    }

    #[test]
    fn reverse_lookup_resolves_known_and_admits_unknown() {
        let hasher = NidHasher::new(*b"salt");
        let db = SymbolDb::from_names(&hasher, ["sceAudioOutInit", "sceAudioOutOpen"]);

        assert_eq!(
            db.name(hasher.hash("sceAudioOutInit")),
            Some("sceAudioOutInit")
        );
        // A name the DB never saw resolves to nothing - not a panic, not a
        // fabricated name.
        assert_eq!(db.name(hasher.hash("sceNeverHeardOfIt")), None);
        assert_eq!(db.len(), 2);
    }
    #[test]
    fn the_shipped_suffix_is_valid_and_the_expected_length() {
        // It is embedded, so a typo in the data file breaks import resolution for every
        // user and would otherwise surface as "nothing resolves" much later.
        let suffix = default_suffix();
        assert_eq!(suffix.len(), 16, "the suffix is sixteen bytes");
        assert!(
            suffix.iter().any(|b| *b != 0),
            "an all-zero suffix means the file failed to parse into anything real"
        );
        assert_eq!(NidHasher::default().suffix_len(), suffix.len());
    }

    #[test]
    fn a_hash_survives_the_round_trip_through_a_symbol_name() {
        // **The invariant that was missing.** A hasher and a decoder that disagree about
        // byte order are each perfectly self-consistent, so every test passed while the
        // two produced values that could never match. Nothing catches that except
        // checking a hash back through the decoder.
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

    #[test]
    fn odd_length_or_non_hex_is_refused_rather_than_partially_decoded() {
        // A suffix silently missing a byte hashes to something plausible and matches
        // nothing, which is the hardest kind of wrong to notice.
        assert!(decode_hex("abc").is_none(), "odd length");
        assert!(decode_hex("zz").is_none(), "not hex");
        assert_eq!(decode_hex("00ff").as_deref(), Some(&[0, 255][..]));
        assert_eq!(decode_hex(""), Some(Vec::new()));
    }
}
