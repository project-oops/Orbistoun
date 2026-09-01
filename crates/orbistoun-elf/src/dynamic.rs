//! The dynamic table, and the import list it leads to.
//!
//! Imports turn out to use **standard ELF machinery**: `PT_DYNAMIC`, `DT_STRTAB`,
//! `DT_SYMTAB`, `DT_HASH`, and an ordinary symbol table. What makes them
//! vendor-specific is only the *encoding of the names* - a dynamic symbol is called
//! something like `H2e8t5ScQGc#B#C`, which is a base64 NID plus a library id plus a
//! module id (`orbistoun-nid::decode_symbol_name`).
//!
//! So there is far less bespoke parsing here than the vendor `DT_` tags suggest.
//!
//! # Addresses are virtual, and reached through the wrapper
//!
//! Every address in the dynamic table is a guest virtual address. Translating one to a
//! file position means finding the program header whose virtual range covers it, then
//! locating that header's bytes through the wrapper's descriptor table (D052) - the
//! headers' own `p_offset` values point past end-of-file and are not usable.
//!
//! # Symbol count
//!
//! There is no `DT_SYMSZ`. The count comes from `DT_HASH`, whose second word is
//! `nchain` and equals the number of symbols. Deriving it from the gap between tables
//! would be wrong here: the string table sits *before* the symbol table in real
//! material, so that heuristic produces nonsense.

use orbistoun_nid::{EncodedImport, NidHasher, decode_symbol_name};

use crate::ElfError;

/// Size of one `Elf64_Sym`.
pub const SYMBOL_SIZE: usize = 24;

/// Size of one dynamic table entry.
pub const DYNAMIC_ENTRY_SIZE: usize = 16;

/// Sanity ceiling on the symbol count, so a corrupt `DT_HASH` cannot drive an
/// enormous loop before the bounds checks catch it.
pub const MAX_SYMBOLS: u64 = 1_000_000;

/// Standard dynamic tags this parser uses.
pub mod tag {
    /// Library this module needs.
    pub const NEEDED: u64 = 1;
    /// Symbol hash table.
    pub const HASH: u64 = 4;
    /// GNU's replacement for [`HASH`], and often the only one present.
    ///
    /// A public extension to ELF rather than anything vendor-specific, but it matters
    /// here for a reason that is: it carries **no symbol count**. `DT_HASH` states
    /// `nchain` in its second word; this states nothing, and the count has to be walked
    /// out of the bucket and chain arrays (see `symbol_count_from_gnu_hash`).
    ///
    /// Every module built by the platform's own toolchain carries `DT_HASH`. Every one
    /// built by an open toolchain carries only this, which is why a loader that reads
    /// only the vendor's tables cannot see a homebrew module's imports at all (D305).
    pub const GNU_HASH: u64 = 0x6fff_fef5;
    /// String table.
    pub const STRTAB: u64 = 5;
    /// Symbol table.
    pub const SYMTAB: u64 = 6;
    /// String table size.
    pub const STRSZ: u64 = 10;
    /// Size of one symbol entry.
    pub const SYMENT: u64 = 11;
    /// Data relocation table.
    pub const RELA: u64 = 7;
    /// Size of the data relocation table.
    pub const RELASZ: u64 = 8;
    /// Procedure linkage table relocations - one slot per imported function.
    pub const JMPREL: u64 = 23;
    /// Size of the procedure linkage table relocations.
    pub const PLTRELSZ: u64 = 2;
    /// Address of a single initialisation function, run before the entry point.
    pub const INIT: u64 = 12;
    /// Address of the array of initialisation functions, run in order.
    ///
    /// **This is where a C++ global constructor lives.** A namespace-scope object with a
    /// constructor gets an entry here; a function-local static does not, because that one
    /// initialises on first use behind a guard variable. Ignoring this tag therefore
    /// produces a guest whose statics look like they initialised - the guard traffic is
    /// there in the trace - while every global object is still zero (D235).
    pub const INIT_ARRAY: u64 = 25;
    /// Size in bytes of the initialisation array.
    pub const INIT_ARRAYSZ: u64 = 27;
    /// Array of functions run before everything above.
    pub const PREINIT_ARRAY: u64 = 32;
    /// Size in bytes of the pre-initialisation array.
    pub const PREINIT_ARRAYSZ: u64 = 33;
    /// The vendor's own names for the tables a console loader actually reads.
    ///
    /// # Why these exist alongside the standard ones
    ///
    /// A console loader **ignores** `DT_STRTAB`, `DT_SYMTAB`, `DT_HASH` and the rest, and
    /// reads these instead. Their values are not virtual addresses: they are offsets into
    /// the `PT_SCE_DYNLIBDATA` segment, which is a different resolution entirely.
    ///
    /// Every title in the local corpus happens to carry the standard tags too, which is
    /// why reading only those has worked. A module built the way the platform expects
    /// carries **only** these - the conformance probe's minimal module does, and orbistoun
    /// refused it with "dynamic table lacks a string table, symbol table, or hash table"
    /// while the module had all three (D247).
    pub mod sce {
        /// Symbol hash table, as an offset into the vendor data segment.
        pub const HASH: u64 = 0x6100_0025;
        /// String table.
        pub const STRTAB: u64 = 0x6100_0035;
        /// String table size.
        pub const STRSZ: u64 = 0x6100_0037;
        /// Symbol table.
        pub const SYMTAB: u64 = 0x6100_0039;
        /// Size of one symbol entry.
        pub const SYMENT: u64 = 0x6100_003B;
        /// Data relocation table.
        pub const RELA: u64 = 0x6100_002F;
        /// Size of the data relocation table.
        pub const RELASZ: u64 = 0x6100_0031;
        /// Procedure linkage table relocations.
        pub const JMPREL: u64 = 0x6100_0029;
        /// Size of those relocations.
        pub const PLTRELSZ: u64 = 0x6100_002D;
    }

    /// Vendor tag listing the libraries an import's library id indexes.
    ///
    /// In the OS-specific range, so it is the platform's to define. Identified by
    /// counting: it holds exactly as many entries as there are distinct library ids,
    /// where `DT_NEEDED` does not, and it puts socket functions in a POSIX library
    /// rather than in a graphics driver (D117).
    pub const SCE_IMPORT_LIB: u64 = 0x0000_6100_0049;
    /// Vendor tag listing modules, indexed by an import's module id.
    pub const SCE_IMPORT_MODULE: u64 = 0x0000_6100_0045;
}

/// Splits a vendor table entry into its id and its name offset.
///
/// The value packs an id in the top sixteen bits, a version in the middle, and a
/// string-table offset in the bottom thirty-two.
pub const fn split_table_entry(value: u64) -> (u16, u32) {
    ((value >> 48) as u16, (value & 0xFFFF_FFFF) as u32)
}

/// The dynamic table's contents, as far as importing needs them.
#[derive(Debug, Clone, Default)]
pub struct DynamicInfo {
    /// Virtual address of the string table.
    pub strtab: u64,
    /// Size of the string table.
    pub strsz: u64,
    /// Virtual address of the symbol table.
    pub symtab: u64,
    /// Size of one symbol entry.
    pub syment: u64,
    /// The vendor's own import-library table, as `(id, name-offset)` pairs.
    ///
    /// **Not `DT_NEEDED`, and the difference is not cosmetic.** An encoded symbol name
    /// carries a library id, and those ids index *this* table. Indexing `DT_NEEDED`
    /// instead produced attributions that fit and meant nothing - a graphics driver
    /// exporting `setsockopt` - because the two lists are different lengths and
    /// different contents (D117).
    pub libraries: Vec<u64>,
    /// The vendor's module table, indexed by an import's module id.
    pub modules: Vec<u64>,
    /// Virtual address of the hash table, whose `nchain` gives the symbol count.
    pub hash: u64,
    /// Virtual address of a GNU hash table, or zero.
    ///
    /// Held separately rather than folded into [`Self::hash`] because the two are read
    /// differently: one states the symbol count and the other has to be walked for it.
    pub gnu_hash: u64,
    /// String-table offsets of the libraries this module needs.
    pub needed: Vec<u64>,
    /// Virtual address of the data relocation table.
    pub rela: u64,
    /// Size of the data relocation table.
    pub relasz: u64,
    /// Virtual address of the procedure linkage table relocations.
    pub jmprel: u64,
    /// Size of the procedure linkage table relocations.
    pub pltrelsz: u64,
    /// Virtual address of a single initialisation function, or zero.
    pub init: u64,
    /// Virtual address of the array of initialisation functions, or zero.
    pub init_array: u64,
    /// Size in bytes of that array.
    pub init_arraysz: u64,
    /// Virtual address of the pre-initialisation array, or zero.
    pub preinit_array: u64,
    /// Size in bytes of that array.
    pub preinit_arraysz: u64,
    /// Whether the table addresses above came from the vendor's tags.
    ///
    /// **Changes how they are resolved, not merely where they came from.** A standard tag
    /// holds a virtual address; a vendor tag holds an offset into `PT_SCE_DYNLIBDATA`.
    /// Reading one as the other lands somewhere plausible and wrong (D247).
    pub vendor_tables: bool,
}

impl DynamicInfo {
    /// Parses the dynamic table out of its raw bytes.
    ///
    /// Stops at `DT_NULL`. Unknown tags - including every vendor tag - are skipped
    /// rather than rejected: they carry information this parser does not need, and
    /// failing on them would reject every real module.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut info = Self::default();
        // Collected separately and applied afterwards, because a module may carry both
        // sets and the file does not promise an order. The vendor's win: they are what
        // the platform reads (D247).
        //
        // **`Option`, not zero-means-absent.** A standard tag holds a virtual address and
        // zero is never one; a vendor tag holds an *offset into the data segment*, and
        // offset zero is the first byte of it. The probe's minimal module puts its string
        // table exactly there, so treating zero as missing rejected a module whose tables
        // were all present and correctly described (D247).
        let mut vendor = Self::default();
        let (mut v_strtab, mut v_symtab, mut v_hash) = (None, None, None);
        for chunk in bytes.chunks_exact(DYNAMIC_ENTRY_SIZE) {
            let tag = u64::from_le_bytes(chunk[..8].try_into().unwrap_or_default());
            let value = u64::from_le_bytes(chunk[8..].try_into().unwrap_or_default());
            if tag == 0 {
                break;
            }
            match tag {
                tag::NEEDED => info.needed.push(value),
                tag::STRTAB => info.strtab = value,
                tag::STRSZ => info.strsz = value,
                tag::SYMTAB => info.symtab = value,
                tag::SYMENT => info.syment = value,
                tag::HASH => info.hash = value,
                tag::GNU_HASH => info.gnu_hash = value,
                tag::RELA => info.rela = value,
                tag::RELASZ => info.relasz = value,
                tag::JMPREL => info.jmprel = value,
                tag::SCE_IMPORT_LIB => info.libraries.push(value),
                tag::SCE_IMPORT_MODULE => info.modules.push(value),
                tag::PLTRELSZ => info.pltrelsz = value,
                tag::INIT => info.init = value,
                tag::INIT_ARRAY => info.init_array = value,
                tag::INIT_ARRAYSZ => info.init_arraysz = value,
                tag::PREINIT_ARRAY => info.preinit_array = value,
                tag::PREINIT_ARRAYSZ => info.preinit_arraysz = value,
                tag::sce::HASH => v_hash = Some(value),
                tag::sce::STRTAB => v_strtab = Some(value),
                tag::sce::STRSZ => vendor.strsz = value,
                tag::sce::SYMTAB => v_symtab = Some(value),
                tag::sce::SYMENT => vendor.syment = value,
                tag::sce::RELA => vendor.rela = value,
                tag::sce::RELASZ => vendor.relasz = value,
                tag::sce::JMPREL => vendor.jmprel = value,
                tag::sce::PLTRELSZ => vendor.pltrelsz = value,
                _ => {}
            }
        }
        // Only when all three are named. A module carrying one stray vendor tag must not
        // have its working standard tables replaced by an incomplete vendor set.
        if let (Some(strtab), Some(symtab), Some(hash)) = (v_strtab, v_symtab, v_hash) {
            info.strtab = strtab;
            info.strsz = vendor.strsz;
            info.symtab = symtab;
            info.syment = vendor.syment;
            info.hash = hash;
            info.rela = vendor.rela;
            info.relasz = vendor.relasz;
            info.jmprel = vendor.jmprel;
            info.pltrelsz = vendor.pltrelsz;
            info.vendor_tables = true;
        }
        info
    }

    /// Whether everything needed to walk the symbol table is present.
    ///
    /// Zero means "no such tag" for a standard entry, because a virtual address of zero is
    /// never a table. It means **offset zero into the data segment** for a vendor entry,
    /// which is where a real module puts its string table - so under vendor tags presence
    /// is what the parser established when it read them, not a test on the value (D247).
    pub const fn is_usable(&self) -> bool {
        if self.vendor_tables {
            return true;
        }
        self.strtab != 0 && self.symtab != 0 && (self.hash != 0 || self.gnu_hash != 0)
    }
}

/// What kind of thing an import names.
///
/// # Why the loader has to care
///
/// Interception writes an address into a relocation slot, and for code that address is a
/// thunk - the whole of principle 7. **For data it is a wrong answer that looks right.**
/// A guest importing `__stderrp` loads the slot and then dereferences what it found; handed
/// a thunk it reads the first bytes of x86 instructions as a pointer and carries on. That
/// is indistinguishable from working until something unrelated breaks much later, which is
/// the failure principle 3 exists to stop.
///
/// Read from `st_info`, which the symbol table states outright - no inference (D307).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Code. A thunk address is the right answer.
    Function,
    /// Data. A thunk address is **not** an answer at all.
    Object,
    /// The table did not say, which is its own fact and not a default.
    Unspecified,
}

impl Kind {
    /// Decodes the low nibble of `st_info`.
    ///
    /// Anything this does not recognise is [`Self::Unspecified`] rather than guessed at:
    /// files, sections and TLS entries all appear here and none of them is an import
    /// the thunk table should be answering for.
    const fn from_info(info: u8) -> Self {
        match info & 0xf {
            1 => Self::Object,
            2 => Self::Function,
            _ => Self::Unspecified,
        }
    }
}

/// How a module spelled an import's name, and what that spelling carried with it.
///
/// # Why this is an enum rather than two optional ids
///
/// The two spellings differ in **what the format records**, not merely in syntax. A
/// vendor-encoded name carries its own attribution - the library and module it came
/// from are part of the string. A standard SysV name carries none: the format leaves
/// which library exports a symbol to a search across `DT_NEEDED`, and there is no
/// answer to read out.
///
/// Two `Option<u16>` fields that are always both present or both absent would say the
/// same thing less clearly, and would invite a `0` where "the format does not record
/// this" belongs (D305).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameForm {
    /// `H2e8t5ScQGc#B#C` - a NID with its attribution attached.
    Encoded {
        /// Library index within the module's import list.
        library_id: u16,
        /// Module index.
        module_id: u16,
    },
    /// A plain name, as an open toolchain emits it. **Carries no attribution.**
    Plain,
}

/// One import, as read out of the symbol table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImport {
    /// Index into the dynamic symbol table.
    ///
    /// The link between a name and everything that refers to it numerically -
    /// relocations index this table, and so does the per-import stub table. Without it
    /// a call trace can only say "import 260", which is a fact about the loader rather
    /// than about the guest.
    pub symbol_index: u32,
    /// The hash, decoded from the name or computed from it.
    ///
    /// **Always present, whichever way the name was spelled.** A plain name is not a
    /// second kind of import needing a second resolver - it is a NID nobody hashed yet,
    /// because the exporting library's NID *is* the hash of that same name. Computing it
    /// here means everything downstream resolves one way (D305).
    pub nid: u64,
    /// How the name was spelled, and what it carried.
    pub form: NameForm,
    /// Whether the guest wants code or data here.
    pub kind: Kind,
    /// The symbol name exactly as it appears, encoding included where there is one.
    pub name: String,
}

impl RawImport {
    /// Builds one from a vendor-encoded name.
    fn encoded(symbol_index: u32, name: String, decoded: EncodedImport, kind: Kind) -> Self {
        Self {
            symbol_index,
            nid: decoded.nid.as_raw(),
            form: NameForm::Encoded {
                library_id: decoded.library_id,
                module_id: decoded.module_id,
            },
            kind,
            name,
        }
    }

    /// Builds one from a plain name, hashing it to the NID its exporter publishes.
    fn plain(symbol_index: u32, name: String, hasher: &NidHasher, kind: Kind) -> Self {
        Self {
            nid: hasher.hash(&name).as_raw(),
            symbol_index,
            form: NameForm::Plain,
            kind,
            name,
        }
    }

    /// The library id the name carried, where it carried one.
    ///
    /// [`None`] is **"the format does not record this"**, not "library zero". A caller
    /// attributing an import has to fall back to something else, and blending the two
    /// would attribute every homebrew import to whichever library happens to be first.
    #[must_use]
    pub const fn library_id(&self) -> Option<u16> {
        match self.form {
            NameForm::Encoded { library_id, .. } => Some(library_id),
            NameForm::Plain => None,
        }
    }

    /// The module id the name carried, where it carried one.
    #[must_use]
    pub const fn module_id(&self) -> Option<u16> {
        match self.form {
            NameForm::Encoded { module_id, .. } => Some(module_id),
            NameForm::Plain => None,
        }
    }
}

/// Reads a NUL-terminated string from a table.
pub fn read_cstr(table: &[u8], offset: usize) -> Option<&str> {
    let rest = table.get(offset..)?;
    let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
    std::str::from_utf8(&rest[..end]).ok()
}

/// Extracts imports from the symbol table.
///
/// `symbols` and `strings` are the raw tables; `count` comes from `DT_HASH`. Only
/// undefined symbols are imports - a defined one is something this module provides.
pub fn imports_from_symbols(
    symbols: &[u8],
    strings: &[u8],
    count: u64,
    syment: usize,
    hasher: &NidHasher,
) -> Result<Vec<RawImport>, ElfError> {
    if count > MAX_SYMBOLS {
        return Err(ElfError::AbsurdSymbolCount {
            count,
            max: MAX_SYMBOLS,
        });
    }
    let stride = if syment == 0 { SYMBOL_SIZE } else { syment };
    let mut out = Vec::new();

    for index in 0..count as usize {
        let at = index.saturating_mul(stride);
        let Some(entry) = symbols.get(at..at + SYMBOL_SIZE) else {
            break;
        };
        let name_off = u32::from_le_bytes(entry[..4].try_into().unwrap_or_default()) as usize;
        let kind = Kind::from_info(entry[4]);
        let shndx = u16::from_le_bytes(entry[6..8].try_into().unwrap_or_default());
        // shndx == 0 is SHN_UNDEF: the module needs this and does not provide it.
        if shndx != 0 || name_off == 0 {
            continue;
        }
        let Some(name) = read_cstr(strings, name_off) else {
            continue;
        };
        let at_index = u32::try_from(index).unwrap_or(u32::MAX);
        // **Both spellings are imports, and neither is dropped.** Skipping the ones that
        // did not decode was silent, and it made a module that needs eighty-five things
        // report needing none - which reads as "needs nothing", the exact claim principle
        // 3 forbids an import list from making (D305).
        out.push(match decode_symbol_name(name) {
            Some(decoded) => RawImport::encoded(at_index, name.to_owned(), decoded, kind),
            None => RawImport::plain(at_index, name.to_owned(), hasher, kind),
        });
    }
    Ok(out)
}

/// Walks a GNU hash table for the number of symbols it covers.
///
/// # Why this is walked rather than read
///
/// `DT_HASH` states the count outright in its second word. `DT_GNU_HASH` states no count
/// anywhere: it holds a bloom filter, a bucket array of symbol indices, and a chain array
/// whose entries carry a stop bit in their low bit. The highest symbol index is found by
/// taking the largest bucket and following its chain to the entry whose stop bit is set.
///
/// Layout, from the public ELF GNU hash extension: four `u32` headers - bucket count,
/// the symbol index the hashed range starts at, bloom word count, bloom shift - then the
/// bloom words as `u64`, then the buckets, then the chain.
///
/// # A table covering nothing is not a failure
///
/// When every bucket is zero the module hashes no symbols at all, and the answer is the
/// bias: every symbol below it is unhashed and real. Returning zero there would drop a
/// module's whole symbol table on a legitimate layout.
pub fn symbol_count_from_gnu_hash(table: &[u8]) -> Result<u64, ElfError> {
    /// Bytes of header before the bloom filter.
    const HEADER: usize = 16;

    let word = |at: usize| -> Option<u32> {
        table
            .get(at..at + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let truncated = || ElfError::NoDynamicTable {
        reason: "GNU hash table is truncated",
    };

    let buckets = word(0).ok_or_else(truncated)? as usize;
    let bias = word(4).ok_or_else(truncated)?;
    let bloom_words = word(8).ok_or_else(truncated)? as usize;

    if u64::try_from(buckets).unwrap_or(u64::MAX) > MAX_SYMBOLS
        || u64::try_from(bloom_words).unwrap_or(u64::MAX) > MAX_SYMBOLS
    {
        return Err(ElfError::NoDynamicTable {
            reason: "GNU hash table declares an absurd size",
        });
    }

    let buckets_at = HEADER + bloom_words * 8;
    let mut highest = 0_u32;
    for bucket in 0..buckets {
        let value = word(buckets_at + bucket * 4).ok_or_else(truncated)?;
        highest = highest.max(value);
    }
    // No bucket names a symbol, so nothing is hashed and the bias is the whole answer.
    if highest < bias {
        return Ok(u64::from(bias));
    }

    let chain_at = buckets_at + buckets * 4;
    let mut index = highest;
    loop {
        let offset = usize::try_from(index - bias).map_err(|_| truncated())?;
        let entry = word(chain_at + offset * 4).ok_or_else(truncated)?;
        if entry & 1 == 1 {
            return Ok(u64::from(index) + 1);
        }
        index = index.checked_add(1).ok_or_else(truncated)?;
        if u64::from(index) > MAX_SYMBOLS {
            return Err(ElfError::NoDynamicTable {
                reason: "GNU hash chain has no end",
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DYNAMIC_ENTRY_SIZE, DynamicInfo, MAX_SYMBOLS, SYMBOL_SIZE, imports_from_symbols, read_cstr,
        tag,
    };
    use crate::ElfError;

    fn dynamic(entries: &[(u64, u64)]) -> Vec<u8> {
        let mut v = Vec::new();
        for (t, val) in entries {
            v.extend_from_slice(&t.to_le_bytes());
            v.extend_from_slice(&val.to_le_bytes());
        }
        v.extend_from_slice(&[0_u8; DYNAMIC_ENTRY_SIZE]);
        v
    }

    /// Builds a symbol table. `defined` marks a symbol as provided rather than needed.
    fn symbols(entries: &[(u32, bool)]) -> Vec<u8> {
        let mut v = Vec::new();
        for (name_off, defined) in entries {
            v.extend_from_slice(&name_off.to_le_bytes());
            v.push(0); // info
            v.push(0); // other
            v.extend_from_slice(&u16::from(*defined).to_le_bytes());
            v.extend_from_slice(&0_u64.to_le_bytes()); // value
            v.extend_from_slice(&0_u64.to_le_bytes()); // size
        }
        v
    }

    fn strings(names: &[&str]) -> (Vec<u8>, Vec<u32>) {
        let mut table = vec![0_u8];
        let mut offsets = Vec::new();
        for n in names {
            offsets.push(u32::try_from(table.len()).expect("small"));
            table.extend_from_slice(n.as_bytes());
            table.push(0);
        }
        (table, offsets)
    }

    #[test]
    fn the_tags_the_parser_needs_are_collected_and_the_rest_skipped() {
        // Vendor tags carry information this parser does not need. Rejecting them
        // would reject every real module.
        let d = dynamic(&[
            (tag::NEEDED, 0x100),
            (0x6100_0045, 0xdead),
            (tag::STRTAB, 0x2000),
            (0x6100_0049, 0xbeef),
            (tag::SYMTAB, 0x3000),
            (tag::HASH, 0x4000),
            (tag::SYMENT, 24),
            (tag::STRSZ, 0x500),
            (tag::NEEDED, 0x200),
        ]);
        let info = DynamicInfo::parse(&d);
        assert_eq!(info.strtab, 0x2000);
        assert_eq!(info.symtab, 0x3000);
        assert_eq!(info.hash, 0x4000);
        assert_eq!(info.syment, 24);
        assert_eq!(info.strsz, 0x500);
        assert_eq!(
            info.needed,
            [0x100, 0x200],
            "every NEEDED is kept, in order"
        );
        assert!(info.is_usable());
    }

    #[test]
    fn relocation_tables_are_collected() {
        // Two tables, and the split matters: JMPREL is where imports become calls.
        let d = dynamic(&[
            (tag::RELA, 0x5000),
            (tag::RELASZ, 0x120),
            (tag::JMPREL, 0x6000),
            (tag::PLTRELSZ, 0x60),
        ]);
        let info = DynamicInfo::parse(&d);
        assert_eq!(info.rela, 0x5000);
        assert_eq!(info.relasz, 0x120);
        assert_eq!(info.jmprel, 0x6000);
        assert_eq!(info.pltrelsz, 0x60);
    }

    #[test]
    fn parsing_stops_at_the_null_terminator() {
        let mut d = dynamic(&[(tag::STRTAB, 0x2000)]);
        // Anything after DT_NULL must be ignored, not parsed.
        d.extend_from_slice(&tag::SYMTAB.to_le_bytes());
        d.extend_from_slice(&0x9999_u64.to_le_bytes());
        let info = DynamicInfo::parse(&d);
        assert_eq!(info.strtab, 0x2000);
        assert_eq!(info.symtab, 0, "entries past DT_NULL are not read");
    }

    #[test]
    fn a_table_missing_what_the_walk_needs_reports_itself_unusable() {
        let info = DynamicInfo::parse(&dynamic(&[(tag::STRTAB, 0x2000)]));
        assert!(
            !info.is_usable(),
            "no symtab and no hash means no symbol walk"
        );
    }

    /// A hasher for the tests, with the shipped suffix.
    fn hasher() -> orbistoun_nid::NidHasher {
        orbistoun_nid::NidHasher::new(orbistoun_nid::default_suffix())
    }

    #[test]
    fn only_undefined_symbols_are_imports() {
        // A defined symbol is something this module provides, not something it needs.
        let (strtab, offs) = strings(&["H2e8t5ScQGc#B#C", "ZT4ODD2Ts9o#B#C"]);
        let symtab = symbols(&[(offs[0], false), (offs[1], true)]);

        let imports =
            imports_from_symbols(&symtab, &strtab, 2, SYMBOL_SIZE, &hasher()).expect("walk");
        assert_eq!(imports.len(), 1, "the defined symbol is not an import");
        assert_eq!(imports[0].name, "H2e8t5ScQGc#B#C");
        assert_eq!(imports[0].nid, 0x6740_9c94_b7bc_671f);
        assert_eq!(imports[0].library_id(), Some(1));
        assert_eq!(imports[0].module_id(), Some(2));
    }

    /// **A plainly named import is an import, and it hashes to its exporter's NID.**
    ///
    /// This replaces a test that pinned the opposite - plain names were skipped, which
    /// made a module needing eighty-five things report needing none. The reversal is the
    /// whole of D305: the exporting library publishes `SHA-1(name + suffix)`, so a plain
    /// name is not a second kind of import needing a second resolver. It is a NID nobody
    /// hashed yet.
    #[test]
    fn a_plain_name_becomes_the_nid_its_exporter_publishes() {
        let (strtab, offs) = strings(&["memcpy", "H2e8t5ScQGc#B#C"]);
        let symtab = symbols(&[(offs[0], false), (offs[1], false)]);

        let hasher = hasher();
        let imports =
            imports_from_symbols(&symtab, &strtab, 2, SYMBOL_SIZE, &hasher).expect("walk");

        assert_eq!(imports.len(), 2, "neither spelling is dropped");
        assert_eq!(imports[0].name, "memcpy");
        assert_eq!(
            imports[0].nid,
            hasher.hash("memcpy").as_raw(),
            "the hash of the name, which is what the exporting library publishes"
        );
        assert_eq!(
            imports[0].library_id(),
            None,
            "the format records no attribution, and zero would be an invented one"
        );
        assert_eq!(
            imports[1].library_id(),
            Some(1),
            "an encoded name still carries its own"
        );
    }

    /// **A data import is known to be data, so nobody hands it a function address.**
    ///
    /// `__stderrp` and `optarg` are `STT_OBJECT`, and a guest that imports one loads the
    /// slot and then dereferences what it found. Given a thunk it reads instruction bytes
    /// as a pointer and carries on looking fine, which is the whole reason `st_info` is
    /// read rather than assumed (D307).
    #[test]
    fn an_import_naming_data_is_not_reported_as_a_function() {
        /// One symbol entry with an explicit `st_info` type.
        fn typed(name_off: u32, info: u8) -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&name_off.to_le_bytes());
            v.push(info);
            v.push(0); // other
            v.extend_from_slice(&0_u16.to_le_bytes()); // SHN_UNDEF - an import
            v.extend_from_slice(&0_u64.to_le_bytes()); // value
            v.extend_from_slice(&0_u64.to_le_bytes()); // size
            v
        }

        let (strtab, offs) = strings(&["__stderrp", "puts"]);
        let mut symtab = typed(offs[0], 1); // STT_OBJECT
        symtab.extend(typed(offs[1], 2)); // STT_FUNC

        let imports =
            imports_from_symbols(&symtab, &strtab, 2, SYMBOL_SIZE, &hasher()).expect("walk");
        assert_eq!(imports[0].kind, super::Kind::Object, "__stderrp is data");
        assert_eq!(imports[1].kind, super::Kind::Function, "puts is code");
    }

    /// The count `DT_GNU_HASH` does not state, walked out of its chain.
    ///
    /// Built by hand rather than read from a file so the layout is visible: four header
    /// words, one bloom word, one bucket naming symbol 4, then a chain whose second entry
    /// sets the stop bit - so the highest symbol is 5 and the count is 6.
    #[test]
    fn a_gnu_hash_table_yields_the_symbol_count_it_never_states() {
        let mut table = Vec::new();
        for word in [1_u32, 4, 1, 0] {
            table.extend_from_slice(&word.to_le_bytes());
        }
        table.extend_from_slice(&0_u64.to_le_bytes());
        table.extend_from_slice(&4_u32.to_le_bytes());
        // chain[0] is symbol 4 and continues; chain[1] is symbol 5 and stops.
        table.extend_from_slice(&0x0000_0010_u32.to_le_bytes());
        table.extend_from_slice(&0x0000_0011_u32.to_le_bytes());

        assert_eq!(super::symbol_count_from_gnu_hash(&table).expect("walks"), 6);
    }

    /// **A table covering nothing is a count, not a failure.**
    ///
    /// Every bucket zero means the module hashes no symbols, and the answer is the bias.
    /// Returning zero would silently drop a whole symbol table on a legitimate layout -
    /// the failure mode principle 3 names, arriving as an empty import list.
    #[test]
    fn a_gnu_hash_table_with_no_hashed_symbols_answers_its_bias() {
        let mut table = Vec::new();
        for word in [1_u32, 3, 1, 0] {
            table.extend_from_slice(&word.to_le_bytes());
        }
        table.extend_from_slice(&0_u64.to_le_bytes());
        table.extend_from_slice(&0_u32.to_le_bytes());

        assert_eq!(
            super::symbol_count_from_gnu_hash(&table).expect("walks"),
            3,
            "every symbol below the bias is unhashed and real"
        );
    }

    /// A truncated table is refused rather than guessed at.
    #[test]
    fn a_truncated_gnu_hash_table_is_refused() {
        super::symbol_count_from_gnu_hash(&[0, 0, 0]).expect_err("must refuse");
    }

    /// A chain with no stop bit must not be followed forever.
    #[test]
    fn a_gnu_hash_chain_that_never_stops_is_refused() {
        let mut table = Vec::new();
        for word in [1_u32, 0, 0, 0] {
            table.extend_from_slice(&word.to_le_bytes());
        }
        table.extend_from_slice(&1_u32.to_le_bytes());
        // Every chain entry has a clear low bit, so nothing ever ends the walk.
        table.extend_from_slice(&vec![0_u8; 4096]);

        super::symbol_count_from_gnu_hash(&table).expect_err("must refuse");
    }

    #[test]
    fn an_absurd_symbol_count_is_rejected_before_the_loop() {
        let err = imports_from_symbols(&[], &[], MAX_SYMBOLS + 1, SYMBOL_SIZE, &hasher())
            .expect_err("must refuse");
        assert!(matches!(err, ElfError::AbsurdSymbolCount { .. }));
    }

    #[test]
    fn a_count_larger_than_the_table_stops_at_the_data_rather_than_reading_past_it() {
        let (strtab, offs) = strings(&["H2e8t5ScQGc#B#C"]);
        let symtab = symbols(&[(offs[0], false)]);
        // Claim ten symbols where one exists.
        let imports =
            imports_from_symbols(&symtab, &strtab, 10, SYMBOL_SIZE, &hasher()).expect("walk");
        assert_eq!(imports.len(), 1, "stops at the end of real data");
    }

    #[test]
    fn a_name_offset_past_the_string_table_is_skipped() {
        let symtab = symbols(&[(9999, false)]);
        let imports =
            imports_from_symbols(&symtab, &[0_u8; 4], 1, SYMBOL_SIZE, &hasher()).expect("walk");
        assert!(imports.is_empty(), "no panic, no bogus entry");
    }

    #[test]
    fn strings_read_up_to_the_terminator() {
        let table = b"\0first\0second\0";
        assert_eq!(read_cstr(table, 1), Some("first"));
        assert_eq!(read_cstr(table, 7), Some("second"));
        assert_eq!(read_cstr(table, 999), None);
    }

    /// The initialiser tags are read rather than skipped.
    ///
    /// They were in the `_ => {}` arm, so a module's global constructors were invisible -
    /// not skipped deliberately, simply never looked at. Reading them turns "we do not run
    /// initialisers" from something nobody had checked into something measured: none of the
    /// three titles at a wall has a `DT_INIT_ARRAY` at all (D235).
    #[test]
    fn the_initialiser_tags_are_parsed() {
        let bytes = dynamic(&[
            (tag::INIT, 0x1000),
            (tag::INIT_ARRAY, 0x2000),
            (tag::INIT_ARRAYSZ, 64),
            (tag::PREINIT_ARRAY, 0x3000),
            (tag::PREINIT_ARRAYSZ, 16),
        ]);
        let info = DynamicInfo::parse(&bytes);
        assert_eq!(info.init, 0x1000);
        assert_eq!(info.init_array, 0x2000);
        assert_eq!(info.init_arraysz, 64);
        assert_eq!(info.preinit_array, 0x3000);
        assert_eq!(info.preinit_arraysz, 16);
    }

    /// A module with no initialiser tags reads as having none, not as having some at zero.
    #[test]
    fn absent_initialiser_tags_read_as_absent() {
        let info = DynamicInfo::parse(&dynamic(&[(tag::STRTAB, 0x100)]));
        assert_eq!(info.init_array, 0);
        assert_eq!(info.init_arraysz, 0);
    }
}
