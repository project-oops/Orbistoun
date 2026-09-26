//! The dynamic table, and the import list it leads to.
//!
//! Imports use standard ELF machinery: `PT_DYNAMIC`, `DT_STRTAB`, `DT_SYMTAB`, `DT_HASH`
//! and an ordinary symbol table. Only the encoding of the names is vendor-specific: a
//! dynamic symbol is named like `H2e8t5ScQGc#B#C`, a base64 NID plus a library id plus a
//! module id (`orbistoun-nid::decode_symbol_name`). Table addresses are resolved through the
//! container, since the headers' own `p_offset` values point past end-of-file in a wrapped
//! container. There is no `DT_SYMSZ`: the symbol count is `DT_HASH`'s `nchain`, because the
//! string table sits before the symbol table in real material and the gap between them is
//! not a size.

use orbistoun_nid::{EncodedImport, NidHasher, decode_symbol_name};

use crate::ElfError;

/// Size of one `Elf64_Sym`.
pub const SYMBOL_SIZE: usize = 24;

/// Size of one dynamic table entry.
pub const DYNAMIC_ENTRY_SIZE: usize = 16;

/// Sanity ceiling on the symbol count, so a corrupt `DT_HASH` cannot drive an enormous loop
/// before the bounds checks catch it.
pub const MAX_SYMBOLS: u64 = 1_000_000;

/// Standard dynamic tags this parser uses.
pub mod tag {
    /// Library this module needs.
    pub const NEEDED: u64 = 1;
    /// Symbol hash table.
    pub const HASH: u64 = 4;
    /// GNU's replacement for [`HASH`], and often the only one present.
    ///
    /// A public ELF extension that carries no symbol count; the count is walked out of the
    /// bucket and chain arrays (see `symbol_count_from_gnu_hash`). Modules from the
    /// platform's toolchain carry `DT_HASH`; modules from an open toolchain carry only this
    /// (D305).
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
    /// A namespace-scope C++ object with a constructor gets an entry here; a function-local
    /// static initialises on first use behind a guard variable instead.
    pub const INIT_ARRAY: u64 = 25;
    /// Size in bytes of the initialisation array.
    pub const INIT_ARRAYSZ: u64 = 27;
    /// Array of functions run before everything above.
    pub const PREINIT_ARRAY: u64 = 32;
    /// Size in bytes of the pre-initialisation array.
    pub const PREINIT_ARRAYSZ: u64 = 33;
    /// The vendor's own names for the tables a hardware loader reads.
    ///
    /// A hardware loader ignores `DT_STRTAB`, `DT_SYMTAB`, `DT_HASH` and the rest and reads
    /// these, whose values are offsets into the `PT_SCE_DYNLIBDATA` segment rather than
    /// virtual addresses (D247). Many titles also carry the standard tags; a module built the
    /// platform's way carries only these.
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
    /// In the OS-specific range, so it is the platform's to define. It holds exactly as many
    /// entries as there are distinct library ids, where `DT_NEEDED` does not.
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
    /// Not `DT_NEEDED`: the library id in an encoded symbol name indexes this table, and the
    /// two lists differ in length and content.
    pub libraries: Vec<u64>,
    /// The vendor's module table, indexed by an import's module id.
    pub modules: Vec<u64>,
    /// Virtual address of the hash table, whose `nchain` gives the symbol count.
    pub hash: u64,
    /// Virtual address of a GNU hash table, or zero.
    ///
    /// Held apart from [`Self::hash`] because one states the symbol count and the other is
    /// walked for it.
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
    /// Changes how they are resolved: a standard tag holds a virtual address; a vendor tag
    /// holds an offset into `PT_SCE_DYNLIBDATA` (D247).
    pub vendor_tables: bool,
}

impl DynamicInfo {
    /// Parses the dynamic table out of its raw bytes.
    ///
    /// Stops at `DT_NULL`. Unknown tags are skipped rather than rejected: they carry
    /// information this parser does not need.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut info = Self::default();
        // Collected separately and applied afterwards, because a module may carry both sets
        // in any order; the vendor tags win, since they are what the platform reads (D247).
        // `Option`, not zero-means-absent: a vendor tag holds an offset into the data
        // segment, and offset zero is its first byte.
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
        // Only when all three are named, so one stray vendor tag cannot replace working
        // standard tables with an incomplete vendor set.
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
    /// Zero means "no such tag" for a standard entry, since a virtual address of zero is
    /// never a table. For a vendor entry it is offset zero into the data segment, so presence
    /// is what the parser recorded, not a test on the value (D247).
    pub const fn is_usable(&self) -> bool {
        if self.vendor_tables {
            return true;
        }
        self.strtab != 0 && self.symtab != 0 && (self.hash != 0 || self.gnu_hash != 0)
    }
}

/// What kind of thing an import names.
///
/// For code the relocation slot holds a thunk. For data a thunk is a wrong answer that
/// looks right: a guest importing `__stderrp` dereferences the slot and reads instruction
/// bytes as a pointer. Read from `st_info`, which states it outright (D307).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Code. A thunk address is the right answer.
    Function,
    /// Data. A thunk address is not an answer.
    Object,
    /// The table did not say, which is its own fact and not a default.
    Unspecified,
}

impl Kind {
    /// Decodes the low nibble of `st_info`.
    ///
    /// Anything unrecognised is [`Self::Unspecified`]: files, sections and TLS entries appear
    /// here, and none of them is an import the thunk table answers.
    const fn from_info(info: u8) -> Self {
        match info & 0xf {
            1 => Self::Object,
            2 => Self::Function,
            _ => Self::Unspecified,
        }
    }
}

/// The binding attribute of an import, read from the high nibble of `st_info`.
///
/// Under the ELF gABI a weak undefined symbol that cannot be resolved binds to zero, while an
/// unresolved global undefined symbol is an error (D676).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// Local symbol.
    Local,
    /// Global symbol; must be answered.
    Global,
    /// Weak symbol; binds to zero if unanswered.
    Weak,
    /// Unknown or unrecognised binding.
    Unspecified,
}

impl Binding {
    /// Decodes the high nibble of `st_info`.
    pub const fn from_info(info: u8) -> Self {
        match info >> 4 {
            0 => Self::Local,
            1 => Self::Global,
            2 => Self::Weak,
            _ => Self::Unspecified,
        }
    }
}

/// How a module spelled an import's name, and what that spelling carried with it.
///
/// A vendor-encoded name carries the library and module it came from; a standard SysV name
/// carries none, leaving the exporter to a search across `DT_NEEDED` (D305). An enum keeps
/// "the format does not record this" from being written as id `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameForm {
    /// `H2e8t5ScQGc#B#C`: a NID with its attribution attached.
    Encoded {
        /// Library index within the module's import list.
        library_id: u16,
        /// Module index.
        module_id: u16,
    },
    /// A plain name, as an open toolchain emits it. Carries no attribution.
    Plain,
}

/// One import, as read out of the symbol table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImport {
    /// Index into the dynamic symbol table.
    ///
    /// Relocations and the per-import stub table index this table, so it links a name to
    /// everything that refers to it numerically.
    pub symbol_index: u32,
    /// The hash, decoded from the name or computed from it.
    ///
    /// Always present. For a plain name it is computed here, because the exporting library's
    /// NID is the hash of that same name, so everything downstream resolves one way (D305).
    pub nid: u64,
    /// How the name was spelled, and what it carried.
    pub form: NameForm,
    /// Whether the guest wants code or data here.
    pub kind: Kind,
    /// Whether the symbol is strong or weak.
    pub binding: Binding,
    /// The symbol name exactly as it appears, encoding included where there is one.
    pub name: String,
}

impl RawImport {
    /// Builds one from a vendor-encoded name.
    fn encoded(
        symbol_index: u32,
        name: String,
        decoded: EncodedImport,
        kind: Kind,
        binding: Binding,
    ) -> Self {
        Self {
            symbol_index,
            nid: decoded.nid.as_raw(),
            form: NameForm::Encoded {
                library_id: decoded.library_id,
                module_id: decoded.module_id,
            },
            kind,
            binding,
            name,
        }
    }

    /// Builds one from a plain name, hashing it to the NID its exporter publishes.
    fn plain(
        symbol_index: u32,
        name: String,
        hasher: &NidHasher,
        kind: Kind,
        binding: Binding,
    ) -> Self {
        Self {
            nid: hasher.hash(&name).as_raw(),
            symbol_index,
            form: NameForm::Plain,
            kind,
            binding,
            name,
        }
    }

    /// The library id the name carried, where it carried one.
    ///
    /// [`None`] means the format does not record it, not library zero; a caller attributing
    /// the import falls back to something else.
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

/// One export, as read out of the symbol table.
///
/// An import is a NID this module needs answered. An export is the same NID plus where in
/// this module it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawExport {
    /// Index into the dynamic symbol table, so a diagnostic can name the row.
    pub symbol_index: u32,
    /// The hash a caller imports it by.
    pub nid: u64,
    /// How the name was spelled, and what it carried.
    pub form: NameForm,
    /// Whether this is code or data.
    ///
    /// Binding a data export as a function hands the guest a thunk where it expects a value.
    pub kind: Kind,
    /// The symbol name exactly as it appears, encoding included where there is one.
    pub name: String,
    /// Where it lives, as an offset from the module's own base.
    ///
    /// Nothing is placed when this is read; the loader adds the base.
    pub offset: u64,
}

impl RawExport {
    /// Builds one from a vendor-encoded name.
    fn encoded(
        symbol_index: u32,
        name: String,
        decoded: EncodedImport,
        kind: Kind,
        offset: u64,
    ) -> Self {
        Self {
            symbol_index,
            nid: decoded.nid.as_raw(),
            form: NameForm::Encoded {
                library_id: decoded.library_id,
                module_id: decoded.module_id,
            },
            kind,
            name,
            offset,
        }
    }

    /// Builds one from a plain name, hashing it to the NID an importer asks for.
    fn plain(symbol_index: u32, name: String, hasher: &NidHasher, kind: Kind, offset: u64) -> Self {
        Self {
            nid: hasher.hash(&name).as_raw(),
            symbol_index,
            form: NameForm::Plain,
            kind,
            name,
            offset,
        }
    }
}

/// Extracts exports from the symbol table.
///
/// The mirror of [`imports_from_symbols`]: same table, same walk, and the opposite test on
/// `st_shndx`. A symbol bound to a section is one this module provides.
///
/// Binding and visibility are not consulted. Importers name NIDs, and the only question is
/// whether this module answers one: an unasked symbol costs a table row, while a symbol
/// wrongly filtered out is an import that never binds.
///
/// # Errors
///
/// [`ElfError::AbsurdSymbolCount`] when the table claims more symbols than could be real,
/// the same bound the import walk applies.
pub fn exports_from_symbols(
    symbols: &[u8],
    strings: &[u8],
    count: u64,
    syment: usize,
    hasher: &NidHasher,
) -> Result<Vec<RawExport>, ElfError> {
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
        // Defined and named. `shndx == 0` is SHN_UNDEF, the import case.
        if shndx == 0 || name_off == 0 {
            continue;
        }
        let offset = u64::from_le_bytes(entry[8..16].try_into().unwrap_or_default());
        let Some(name) = read_cstr(strings, name_off) else {
            continue;
        };
        let at_index = u32::try_from(index).unwrap_or(u32::MAX);
        // Both spellings are kept, as in the import walk (D305).
        out.push(match decode_symbol_name(name) {
            Some(decoded) => RawExport::encoded(at_index, name.to_owned(), decoded, kind, offset),
            None => RawExport::plain(at_index, name.to_owned(), hasher, kind, offset),
        });
    }
    Ok(out)
}

/// Extracts imports from the symbol table.
///
/// `symbols` and `strings` are the raw tables; `count` comes from `DT_HASH`. Only
/// undefined symbols are imports; a defined one is something this module provides.
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
        let binding = Binding::from_info(entry[4]);
        let shndx = u16::from_le_bytes(entry[6..8].try_into().unwrap_or_default());
        // shndx == 0 is SHN_UNDEF: the module needs this and does not provide it.
        if shndx != 0 || name_off == 0 {
            continue;
        }
        let Some(name) = read_cstr(strings, name_off) else {
            continue;
        };
        let at_index = u32::try_from(index).unwrap_or(u32::MAX);
        // Both spellings are imports; a plain name is hashed to its NID (D305).
        out.push(match decode_symbol_name(name) {
            Some(decoded) => RawImport::encoded(at_index, name.to_owned(), decoded, kind, binding),
            None => RawImport::plain(at_index, name.to_owned(), hasher, kind, binding),
        });
    }
    Ok(out)
}

/// Walks a GNU hash table for the number of symbols it covers.
///
/// `DT_GNU_HASH` states no count. It holds a bloom filter, a bucket array of symbol indices,
/// and a chain array whose entries carry a stop bit in their low bit; the highest symbol
/// index is found by following the largest bucket's chain to its stop bit. Layout, from the
/// public ELF GNU hash extension: four `u32` headers (bucket count, the symbol index the
/// hashed range starts at, bloom word count, bloom shift), then the bloom words as `u64`,
/// then the buckets, then the chain. When every bucket is zero no symbol is hashed and the
/// count is the bias.
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
        DYNAMIC_ENTRY_SIZE, DynamicInfo, MAX_SYMBOLS, NameForm, SYMBOL_SIZE, exports_from_symbols,
        imports_from_symbols, read_cstr, tag,
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
        let placed: Vec<(u32, bool, u64)> = entries.iter().map(|(n, d)| (*n, *d, 0)).collect();
        symbols_at(&placed)
    }

    /// The same, with each symbol's value, which is where an export lives.
    fn symbols_at(entries: &[(u32, bool, u64)]) -> Vec<u8> {
        let mut v = Vec::new();
        for (name_off, defined, value) in entries {
            v.extend_from_slice(&name_off.to_le_bytes());
            v.push(0); // info
            v.push(0); // other
            v.extend_from_slice(&u16::from(*defined).to_le_bytes());
            v.extend_from_slice(&value.to_le_bytes());
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

    /// Every named symbol is exactly one of an import or an export, split on `SHN_UNDEF`.
    #[test]
    fn every_named_symbol_is_an_import_or_an_export_and_never_both() {
        let (strings, at) = strings(&["needed_one", "provided_one", "needed_two"]);
        let table = symbols_at(&[(at[0], false, 0), (at[1], true, 0x1234), (at[2], false, 0)]);
        let hasher = hasher();

        let imports = imports_from_symbols(&table, &strings, 3, SYMBOL_SIZE, &hasher)
            .expect("the table is small and well formed");
        let exports = exports_from_symbols(&table, &strings, 3, SYMBOL_SIZE, &hasher)
            .expect("the same table read the other way");

        let imported: Vec<&str> = imports.iter().map(|i| i.name.as_str()).collect();
        let exported: Vec<&str> = exports.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(imported, ["needed_one", "needed_two"]);
        assert_eq!(exported, ["provided_one"]);
    }

    /// An export carries its offset within the module.
    #[test]
    fn an_export_carries_its_offset_within_the_module() {
        let (strings, at) = strings(&["provided"]);
        let table = symbols_at(&[(at[0], true, 0x4142_4344)]);
        let hasher = hasher();

        let exports = exports_from_symbols(&table, &strings, 1, SYMBOL_SIZE, &hasher)
            .expect("one well-formed symbol");
        assert_eq!(exports.len(), 1);
        assert_eq!(
            exports[0].offset, 0x4142_4344,
            "the value is an offset from the module base, not an address"
        );
        assert_eq!(
            exports[0].nid,
            hasher.hash("provided").as_raw(),
            "a plain name is the NID an importer will ask for, hashed"
        );
    }

    /// A vendor-encoded export decodes to the NID it publishes, not to a hash of the encoding.
    #[test]
    fn an_encoded_export_publishes_the_nid_in_its_name() {
        let (strings, at) = strings(&["CRJcH8CnPSI#I#A"]);
        let table = symbols_at(&[(at[0], true, 0x10)]);
        let hasher = hasher();

        let exports = exports_from_symbols(&table, &strings, 1, SYMBOL_SIZE, &hasher)
            .expect("one well-formed symbol");
        assert_ne!(
            exports[0].nid,
            hasher.hash("CRJcH8CnPSI#I#A").as_raw(),
            "the encoding carries the NID; hashing the spelling answers a different one"
        );
        assert!(matches!(exports[0].form, NameForm::Encoded { .. }));
    }

    /// The tags the parser needs are collected and unknown tags skipped.
    #[test]
    fn the_tags_the_parser_needs_are_collected_and_the_rest_skipped() {
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

    /// Both relocation tables are collected.
    #[test]
    fn relocation_tables_are_collected() {
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

    /// Parsing stops at `DT_NULL`.
    #[test]
    fn parsing_stops_at_the_null_terminator() {
        let mut d = dynamic(&[(tag::STRTAB, 0x2000)]);
        // Anything after DT_NULL is ignored.
        d.extend_from_slice(&tag::SYMTAB.to_le_bytes());
        d.extend_from_slice(&0x9999_u64.to_le_bytes());
        let info = DynamicInfo::parse(&d);
        assert_eq!(info.strtab, 0x2000);
        assert_eq!(info.symtab, 0, "entries past DT_NULL are not read");
    }

    /// A table missing what the symbol walk needs reports itself unusable.
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

    /// Only undefined symbols are imports.
    #[test]
    fn only_undefined_symbols_are_imports() {
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

    /// A plainly named import is an import, and hashes to its exporter's NID (D305).
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

    /// A data import is reported as data, so it is not handed a function address (D307).
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

    /// An import's binding is read from the high nibble of `st_info` (D676).
    #[test]
    fn an_import_binding_is_read_from_st_info() {
        /// One symbol entry with an explicit `st_info` byte.
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

        let (strtab, offs) = strings(&["weak_sym", "global_sym", "local_sym"]);
        let mut symtab = typed(offs[0], 0x22); // STB_WEAK | STT_FUNC
        symtab.extend(typed(offs[1], 0x12)); // STB_GLOBAL | STT_FUNC
        symtab.extend(typed(offs[2], 0x01)); // STB_LOCAL | STT_OBJECT

        let imports =
            imports_from_symbols(&symtab, &strtab, 3, SYMBOL_SIZE, &hasher()).expect("walk");
        assert_eq!(imports[0].binding, super::Binding::Weak);
        assert_eq!(imports[1].binding, super::Binding::Global);
        assert_eq!(imports[2].binding, super::Binding::Local);
    }

    /// The count `DT_GNU_HASH` does not state, walked out of its chain.
    ///
    /// Four header words, one bloom word, one bucket naming symbol 4, then a chain whose
    /// second entry sets the stop bit: the highest symbol is 5 and the count is 6.
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

    /// A GNU hash table covering nothing answers its bias, not zero.
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

    /// A chain with no stop bit is refused rather than followed forever.
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

    /// An absurd symbol count is rejected before the walk.
    #[test]
    fn an_absurd_symbol_count_is_rejected_before_the_loop() {
        let err = imports_from_symbols(&[], &[], MAX_SYMBOLS + 1, SYMBOL_SIZE, &hasher())
            .expect_err("must refuse");
        assert!(matches!(err, ElfError::AbsurdSymbolCount { .. }));
    }

    /// A count larger than the table stops at the data.
    #[test]
    fn a_count_larger_than_the_table_stops_at_the_data_rather_than_reading_past_it() {
        let (strtab, offs) = strings(&["H2e8t5ScQGc#B#C"]);
        let symtab = symbols(&[(offs[0], false)]);
        // Claim ten symbols where one exists.
        let imports =
            imports_from_symbols(&symtab, &strtab, 10, SYMBOL_SIZE, &hasher()).expect("walk");
        assert_eq!(imports.len(), 1, "stops at the end of real data");
    }

    /// A name offset past the string table is skipped.
    #[test]
    fn a_name_offset_past_the_string_table_is_skipped() {
        let symtab = symbols(&[(9999, false)]);
        let imports =
            imports_from_symbols(&symtab, &[0_u8; 4], 1, SYMBOL_SIZE, &hasher()).expect("walk");
        assert!(imports.is_empty(), "no panic, no bogus entry");
    }

    /// Strings read up to the NUL terminator.
    #[test]
    fn strings_read_up_to_the_terminator() {
        let table = b"\0first\0second\0";
        assert_eq!(read_cstr(table, 1), Some("first"));
        assert_eq!(read_cstr(table, 7), Some("second"));
        assert_eq!(read_cstr(table, 999), None);
    }

    /// The initialiser tags are read rather than skipped.
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

    /// A module with no initialiser tags reads as having none.
    #[test]
    fn absent_initialiser_tags_read_as_absent() {
        let info = DynamicInfo::parse(&dynamic(&[(tag::STRTAB, 0x100)]));
        assert_eq!(info.init_array, 0);
        assert_eq!(info.init_arraysz, 0);
    }
}
