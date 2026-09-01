//! Parsing a whole module: headers, the dynamic table, and the import walk.
//!
//! # Why a synthetic module rather than a real one
//!
//! No title can be in this repository, and the conformance probe's module is not built by
//! this test suite either. So the fixtures here are assembled byte by byte, which has an
//! advantage a real file does not: **each one is wrong in exactly one way**. A truncated
//! program header table, a hash table past the end of the file, a symbol count that could
//! not be true - each is one edit from the module beside it, so a test that fails names the
//! thing that broke rather than "the file did not parse".
//!
//! Real material still decides the questions synthetic bytes cannot. Where a shape here was
//! chosen to match something observed, it says so.
//!
//! # The two tag dialects
//!
//! A standard dynamic tag holds a **virtual address**; a vendor tag holds an **offset into
//! the vendor data segment**. Resolving one the way the other is meant lands at a plausible
//! file position holding the wrong bytes, which is how a module with two ordinary
//! relocations came to report two of an unsupported type (D247). Both dialects are built
//! below, from the same layout, so the difference between them is the only variable.

use orbistoun_elf::{Container, ElfError, dynamic, is_vendor_segment};

const EHDR: usize = 64;
const PHDR: usize = 56;
const SYMBOL: usize = 24;
const DYN_ENTRY: usize = 16;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_GNU_EH_FRAME: u32 = 0x6474_e550;
const PT_SCE_DYNLIBDATA: u32 = 0x6100_0000;

// The layout every fixture shares. Virtual address equals file offset throughout, because
// one `PT_LOAD` covers the whole image from zero - so an address that resolves wrongly
// resolves to a *different* place rather than to nothing, and the test can tell.
const DYN_AT: usize = 0x200;
const DATA_AT: usize = 0x400;
const HASH_OFF: usize = 0x000;
const STR_OFF: usize = 0x020;
const SYM_OFF: usize = 0x100;
const STRSZ: usize = 0x0E0;
const TOTAL: usize = 0x800;

/// A string table under construction, handing back the offset of each name.
struct Strings {
    bytes: Vec<u8>,
}

impl Strings {
    /// Starts with the mandatory empty string at offset zero.
    fn new() -> Self {
        Self { bytes: vec![0] }
    }

    fn add(&mut self, name: &str) -> u32 {
        let at = self.bytes.len();
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.push(0);
        u32::try_from(at).expect("string table stays small")
    }
}

/// One dynamic symbol table entry.
///
/// `shndx` of zero is `SHN_UNDEF` - the module needs this and does not provide it, which
/// is what makes a symbol an import rather than an export.
#[derive(Clone, Copy)]
struct Symbol {
    name_off: u32,
    info: u8,
    shndx: u16,
}

/// The symbol table every fixture carries.
///
/// Seven entries, four of which are imports. The other three are each a different reason
/// *not* to be one, and all three have to be excluded for the count to come out right:
/// index 0 is the mandatory null entry, index 4 is defined by this module, and index 6 has
/// no name.
fn symbols(strings: &mut Strings) -> ([Symbol; 7], Names) {
    let encoded = strings.add("H2e8t5ScQGc#B#C");
    let plain = strings.add("memcpy");
    let object = strings.add("__stderrp");
    let defined = strings.add("provided_by_this_module");
    let other = strings.add("a_thread_local");
    let needed = strings.add("libkernel.sprx");
    let library = strings.add("libSceLibcInternal");
    let module = strings.add("libc");

    (
        [
            Symbol {
                name_off: 0,
                info: 0,
                shndx: 0,
            },
            // GLOBAL binding in the high nibble, FUNC in the low one.
            Symbol {
                name_off: encoded,
                info: 0x12,
                shndx: 0,
            },
            Symbol {
                name_off: plain,
                info: 0x12,
                shndx: 0,
            },
            // OBJECT. A thunk address is not an answer for this one, so the kind has to
            // survive the walk (D307).
            Symbol {
                name_off: object,
                info: 0x11,
                shndx: 0,
            },
            // Defined here, so not an import however it is named.
            Symbol {
                name_off: defined,
                info: 0x12,
                shndx: 1,
            },
            // TLS: neither code nor data as far as a thunk table is concerned.
            Symbol {
                name_off: other,
                info: 0x16,
                shndx: 0,
            },
            Symbol {
                name_off: 0,
                info: 0x12,
                shndx: 0,
            },
        ],
        Names {
            needed,
            library,
            module,
        },
    )
}

/// String-table offsets the dynamic table refers to.
struct Names {
    needed: u32,
    library: u32,
    module: u32,
}

/// Which dialect of dynamic tag a fixture uses for its table addresses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    /// Standard tags holding virtual addresses.
    Standard,
    /// Vendor tags holding offsets into the vendor data segment.
    Vendor,
}

/// Builds a complete, walkable module in the given dialect.
fn module(dialect: Dialect) -> Vec<u8> {
    let mut strings = Strings::new();
    let (syms, names) = symbols(&mut strings);

    let mut dynamic_entries: Vec<(u64, u64)> = Vec::new();
    match dialect {
        Dialect::Standard => {
            dynamic_entries.push((dynamic::tag::STRTAB, (DATA_AT + STR_OFF) as u64));
            dynamic_entries.push((dynamic::tag::STRSZ, STRSZ as u64));
            dynamic_entries.push((dynamic::tag::SYMTAB, (DATA_AT + SYM_OFF) as u64));
            dynamic_entries.push((dynamic::tag::SYMENT, SYMBOL as u64));
            dynamic_entries.push((dynamic::tag::HASH, (DATA_AT + HASH_OFF) as u64));
        }
        Dialect::Vendor => {
            dynamic_entries.push((dynamic::tag::sce::STRTAB, STR_OFF as u64));
            dynamic_entries.push((dynamic::tag::sce::STRSZ, STRSZ as u64));
            dynamic_entries.push((dynamic::tag::sce::SYMTAB, SYM_OFF as u64));
            dynamic_entries.push((dynamic::tag::sce::SYMENT, SYMBOL as u64));
            dynamic_entries.push((dynamic::tag::sce::HASH, HASH_OFF as u64));
        }
    }
    dynamic_entries.push((dynamic::tag::NEEDED, u64::from(names.needed)));
    // Id in the top sixteen bits, string-table offset in the bottom thirty-two.
    dynamic_entries.push((
        dynamic::tag::SCE_IMPORT_LIB,
        (1_u64 << 48) | u64::from(names.library),
    ));
    dynamic_entries.push((
        dynamic::tag::SCE_IMPORT_MODULE,
        (2_u64 << 48) | u64::from(names.module),
    ));
    dynamic_entries.push((0, 0));

    let mut headers = vec![
        // One load covering the whole image, so a virtual address is its own file offset.
        (PT_LOAD, 0_u64, 0_u64, TOTAL as u64),
        (
            PT_DYNAMIC,
            DYN_AT as u64,
            DYN_AT as u64,
            (dynamic_entries.len() * DYN_ENTRY) as u64,
        ),
        // Ordinary, and not vendor data however much its type looks like it.
        (PT_GNU_EH_FRAME, 0x100, 0x100, 0x10),
    ];
    if dialect == Dialect::Vendor {
        headers.push((
            PT_SCE_DYNLIBDATA,
            DATA_AT as u64,
            DATA_AT as u64,
            (TOTAL - DATA_AT) as u64,
        ));
    }

    let mut bytes = elf_with(&headers, TOTAL);

    for (index, (tag, value)) in dynamic_entries.iter().enumerate() {
        let at = DYN_AT + index * DYN_ENTRY;
        bytes[at..at + 8].copy_from_slice(&tag.to_le_bytes());
        bytes[at + 8..at + 16].copy_from_slice(&value.to_le_bytes());
    }

    // `DT_HASH` is `[nbucket][nchain]`, and `nchain` is the symbol count outright. There is
    // no `DT_SYMSZ`, so this is the only thing that states how far the table runs.
    let hash_at = DATA_AT + HASH_OFF;
    bytes[hash_at..hash_at + 4].copy_from_slice(&1_u32.to_le_bytes());
    bytes[hash_at + 4..hash_at + 8].copy_from_slice(&(syms.len() as u32).to_le_bytes());

    let str_at = DATA_AT + STR_OFF;
    bytes[str_at..str_at + strings.bytes.len()].copy_from_slice(&strings.bytes);

    for (index, sym) in syms.iter().enumerate() {
        let at = DATA_AT + SYM_OFF + index * SYMBOL;
        bytes[at..at + 4].copy_from_slice(&sym.name_off.to_le_bytes());
        bytes[at + 4] = sym.info;
        bytes[at + 6..at + 8].copy_from_slice(&sym.shndx.to_le_bytes());
    }

    bytes
}

/// A bare ELF64 with the given `(p_type, offset, vaddr, filesz)` program headers.
fn elf_with(headers: &[(u32, u64, u64, u64)], size: usize) -> Vec<u8> {
    let phoff = EHDR;
    let mut elf = vec![0_u8; size.max(phoff + PHDR * headers.len())];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; // 64-bit
    elf[5] = 1; // little endian
    elf[6] = 1; // version
    elf[16..18].copy_from_slice(&0xfe18_u16.to_le_bytes()); // a vendor `e_type`
    elf[18..20].copy_from_slice(&62_u16.to_le_bytes()); // x86-64
    elf[24..32].copy_from_slice(&0x1234_u64.to_le_bytes()); // entry
    elf[32..40].copy_from_slice(&(phoff as u64).to_le_bytes());
    elf[54..56].copy_from_slice(&(PHDR as u16).to_le_bytes());
    elf[56..58].copy_from_slice(&(headers.len() as u16).to_le_bytes());

    for (index, (p_type, offset, vaddr, filesz)) in headers.iter().enumerate() {
        let at = phoff + PHDR * index;
        elf[at..at + 4].copy_from_slice(&p_type.to_le_bytes());
        elf[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        elf[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
        elf[at + 32..at + 40].copy_from_slice(&filesz.to_le_bytes());
        elf[at + 40..at + 48].copy_from_slice(&filesz.to_le_bytes());
    }
    elf
}

/// The hasher, with a fixed suffix so a NID in an assertion is reproducible.
fn hasher() -> orbistoun_nid::NidHasher {
    orbistoun_nid::NidHasher::new(*b"container-tests")
}

// --- the header ------------------------------------------------------------------------

/// Anything that is not an ELF is refused as such, and the refusal says which check failed.
///
/// Three different rejections rather than one, because a caller trying to work out whether
/// it handed over the wrong file or the right file truncated needs them apart.
#[test]
fn the_header_checks_are_distinguishable_from_each_other() {
    assert!(matches!(
        Container::parse(&[0_u8; 8]),
        Err(ElfError::Truncated { .. })
    ));

    let mut not_elf = module(Dialect::Standard);
    not_elf[1] = b'X';
    assert!(matches!(Container::parse(&not_elf), Err(ElfError::NotElf)));

    let mut wrong_class = module(Dialect::Standard);
    wrong_class[4] = 1; // 32-bit
    assert!(matches!(
        Container::parse(&wrong_class),
        Err(ElfError::UnsupportedFormat)
    ));

    let mut wrong_endian = module(Dialect::Standard);
    wrong_endian[5] = 2; // big endian
    assert!(matches!(
        Container::parse(&wrong_endian),
        Err(ElfError::UnsupportedFormat)
    ));
}

/// A file exactly one byte short of a header is truncated, not "not an ELF".
///
/// The boundary matters because the magic is read *after* the size check, so an off-by-one
/// there turns every short file into a confusing `NotElf`.
#[test]
fn a_file_one_byte_short_of_a_header_is_truncated() {
    let bytes = module(Dialect::Standard);
    assert!(matches!(
        Container::parse(&bytes[..EHDR - 1]),
        Err(ElfError::Truncated { need, have, .. }) if need == EHDR && have == EHDR - 1
    ));
    assert!(Container::parse(&bytes[..EHDR]).is_ok());
}

/// The entry point and the wrapper's absence are both read straight out of the header.
#[test]
fn a_bare_module_reports_its_entry_and_no_wrapper() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");
    assert_eq!(container.entry(), 0x1234);
    assert!(container.wrapper().is_none());
    assert_eq!(container.header().machine.get(), 62);
}

/// A program header table reaching past the end of the file is refused, not walked.
///
/// The count comes from arbitrary bytes, so it is an allocation and a read sized by the
/// input. Both have to be bounded before either happens.
#[test]
fn a_program_header_table_past_the_end_of_the_file_is_refused() {
    let mut bytes = module(Dialect::Standard);
    bytes[56..58].copy_from_slice(&1000_u16.to_le_bytes());
    let container = Container::parse(&bytes).expect("the header itself is still valid");
    assert!(matches!(
        container.program_headers(),
        Err(ElfError::Truncated { .. })
    ));
}

/// An empty program header table is a valid module that describes nothing.
#[test]
fn an_empty_program_header_table_is_not_an_error() {
    let bytes = elf_with(&[], EHDR);
    let container = Container::parse(&bytes).expect("parses");
    assert!(container.program_headers().expect("walks").is_empty());
    assert!(container.vendor_segments().expect("walks").is_empty());
    assert!(container.dynamic_bytes(&bytes).expect("walks").is_none());
    assert_eq!(container.symbol_count(&bytes).expect("walks"), 0);
    assert!(
        container
            .needed_libraries(&bytes)
            .expect("walks")
            .is_empty()
    );
}

// --- segments ---------------------------------------------------------------------------

/// A GNU segment sits in the OS-specific range and is not vendor data.
///
/// Not cosmetic: real material carries these alongside genuine vendor segments, and
/// counting them overstates how much of a module is unhandled.
#[test]
fn a_gnu_segment_is_not_counted_as_vendor_data() {
    assert!(is_vendor_segment(PT_SCE_DYNLIBDATA));
    assert!(is_vendor_segment(0x6fff_ff00));
    for gnu in orbistoun_elf::GNU_SEGMENT_TYPES {
        assert!(
            orbistoun_elf::OS_SPECIFIC_RANGE.contains(&gnu),
            "{gnu:#x} should be inside the OS-specific range"
        );
        assert!(!is_vendor_segment(gnu), "{gnu:#x} is a GNU extension");
    }
    assert!(!is_vendor_segment(PT_LOAD));
    assert!(!is_vendor_segment(0x7000_0000));
}

/// The vendor data segment is found among headers that include a GNU one.
#[test]
fn the_vendor_data_segment_is_picked_out_from_its_neighbours() {
    let bytes = module(Dialect::Vendor);
    let container = Container::parse(&bytes).expect("parses");

    let vendor = container.vendor_segments().expect("walks");
    assert_eq!(vendor.len(), 1, "only one header carries vendor data");
    assert_eq!(vendor[0].p_type.get(), PT_SCE_DYNLIBDATA);
    assert_eq!(
        container.vendor_data_offset(&bytes).expect("walks"),
        Some(DATA_AT)
    );

    // The standard-tag fixture has the GNU header and no vendor one, so it is the negative.
    let plain = module(Dialect::Standard);
    let plain_container = Container::parse(&plain).expect("parses");
    assert!(plain_container.vendor_segments().expect("walks").is_empty());
    assert_eq!(
        plain_container.vendor_data_offset(&plain).expect("walks"),
        None
    );
}

/// A vendor data segment whose offset is past the end of the file locates nothing.
///
/// Bounds-checked once here rather than at each use, so a nonsense offset is one clear
/// answer instead of three confusing failures downstream.
#[test]
fn a_vendor_segment_past_the_end_of_the_file_locates_nothing() {
    let mut bytes = module(Dialect::Vendor);
    // The vendor data segment is the fourth program header in this fixture.
    let at = EHDR + PHDR * 3;
    bytes[at + 8..at + 16].copy_from_slice(&0xFFFF_0000_u64.to_le_bytes());
    let container = Container::parse(&bytes).expect("parses");
    assert_eq!(container.vendor_data_offset(&bytes).expect("walks"), None);
}

/// A bare container's segment bytes come from the header's own file offset.
#[test]
fn segment_data_comes_from_the_headers_own_offset_when_unwrapped() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");

    let dynamic_segment = container
        .segment_data(&bytes, 1)
        .expect("walks")
        .expect("the dynamic segment has bytes");
    assert_eq!(&dynamic_segment[..8], &dynamic::tag::STRTAB.to_le_bytes());

    assert!(
        container.segment_data(&bytes, 99).expect("walks").is_none(),
        "an index past the table is absence, not an error"
    );
}

/// Nothing is wrapper-mapped in a bare container, which is not the same as nothing being
/// mapped.
///
/// An empty list here means "the program headers address the file directly", and a caller
/// that read it as "this module has no segments" would be wrong about a working module.
#[test]
fn a_bare_container_maps_no_segments_through_a_wrapper() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");
    assert!(
        container
            .mapped_program_headers(&bytes)
            .expect("walks")
            .is_empty()
    );
    assert_eq!(container.program_headers().expect("walks").len(), 3);
}

// --- the dynamic table --------------------------------------------------------------------

/// The dynamic segment's bytes are located through its virtual address, not its file
/// offset.
#[test]
fn the_dynamic_table_is_found_through_its_address() {
    for dialect in [Dialect::Standard, Dialect::Vendor] {
        let bytes = module(dialect);
        let container = Container::parse(&bytes).expect("parses");
        let table = container
            .dynamic_bytes(&bytes)
            .expect("walks")
            .expect("there is a dynamic table");
        let info = dynamic::DynamicInfo::parse(table);
        assert!(info.is_usable(), "every table needed to walk is present");
        assert_eq!(info.vendor_tables, dialect == Dialect::Vendor);
    }
}

/// A module with no `PT_DYNAMIC` has no dynamic table, and says so rather than failing.
#[test]
fn a_module_with_no_dynamic_segment_reports_none() {
    let bytes = elf_with(&[(PT_LOAD, 0, 0, 0x100)], 0x100);
    let container = Container::parse(&bytes).expect("parses");
    assert!(container.dynamic_bytes(&bytes).expect("walks").is_none());
    assert_eq!(container.symbol_count(&bytes).expect("walks"), 0);
}

/// A dynamic segment describing more than the file holds cannot be located.
///
/// Bounds-checked against the file rather than trusted, so a truncated container reads as
/// "cannot locate that" rather than as a panic.
///
/// Note which header does the locating. A segment outside every `PT_LOAD` is still found
/// through **its own** header, since in an unwrapped module those offsets are the only
/// thing there is - so making this unlocatable takes an offset past end-of-file, not merely
/// an address no load covers.
#[test]
fn a_dynamic_segment_describing_more_than_the_file_holds_locates_nothing() {
    let bytes = elf_with(
        &[(PT_LOAD, 0, 0, 0x100), (PT_DYNAMIC, 0x9000, 0x9000, 0x20)],
        0x400,
    );
    let container = Container::parse(&bytes).expect("parses");
    assert!(container.dynamic_bytes(&bytes).expect("walks").is_none());
    assert_eq!(container.symbol_count(&bytes).expect("walks"), 0);
}

/// A segment outside every `PT_LOAD` is still located through its own header.
///
/// The other half of the pair above, and the behaviour that was missing entirely: this
/// returned [`None`] for every address of every bare module, so a container parsed, the
/// loader mapped its segments, and nothing downstream could find a byte (D237).
#[test]
fn a_segment_outside_every_load_is_found_through_its_own_header() {
    let bytes = elf_with(
        &[(PT_LOAD, 0, 0, 0x100), (PT_DYNAMIC, 0x200, 0x9000, 0x20)],
        0x400,
    );
    let container = Container::parse(&bytes).expect("parses");
    assert!(
        container.dynamic_bytes(&bytes).expect("walks").is_some(),
        "its own header says where it is"
    );
}

/// The two tag dialects describe the same tables and must produce the same answer.
///
/// **This is D247 as a test.** The vendor fixture's offsets are small numbers - `0x20`,
/// `0x100` - which are also perfectly good virtual addresses in this image, so resolving
/// them the standard way succeeds and lands on the ELF header. The bug it protects against
/// does not fail loudly; it reads the wrong bytes and carries on.
#[test]
fn both_tag_dialects_describe_the_same_module() {
    let standard = module(Dialect::Standard);
    let vendor = module(Dialect::Vendor);
    let hasher = hasher();

    let a = Container::parse(&standard).expect("parses");
    let b = Container::parse(&vendor).expect("parses");

    assert_eq!(
        a.symbol_count(&standard).expect("walks"),
        b.symbol_count(&vendor).expect("walks"),
        "the same hash table, reached two ways"
    );

    let a_imports = a.raw_imports(&standard, &hasher).expect("walks");
    let b_imports = b.raw_imports(&vendor, &hasher).expect("walks");
    assert_eq!(
        a_imports, b_imports,
        "the same symbol table, reached two ways"
    );
    assert!(
        !a_imports.is_empty(),
        "a module that imports nothing would make this vacuous"
    );
}

/// A vendor table offset of zero is a real offset, not an absent tag.
///
/// The hash table in these fixtures sits at offset zero into the data segment, which is
/// where a real module puts one. Testing the value for zero would refuse a module whose
/// tables are all present and correctly described.
#[test]
fn a_vendor_offset_of_zero_is_the_first_byte_and_not_an_absence() {
    let bytes = module(Dialect::Vendor);
    let container = Container::parse(&bytes).expect("parses");
    let table = container
        .dynamic_bytes(&bytes)
        .expect("walks")
        .expect("there is a dynamic table");
    let info = dynamic::DynamicInfo::parse(table);

    assert_eq!(
        info.hash, 0,
        "the fixture puts the hash table at offset zero"
    );
    assert!(info.is_usable(), "which must not read as a missing tag");
    assert_eq!(container.symbol_count(&bytes).expect("walks"), 7);
    assert_eq!(
        container.table_offset(&bytes, &info, 0).expect("resolves"),
        Some(DATA_AT)
    );
}

/// `table_offset` resolves a value differently in each dialect, which is its whole purpose.
#[test]
fn table_offset_resolves_each_dialect_its_own_way() {
    let standard = module(Dialect::Standard);
    let a = Container::parse(&standard).expect("parses");
    let a_info = dynamic::DynamicInfo::parse(
        a.dynamic_bytes(&standard)
            .expect("walks")
            .expect("has a table"),
    );
    // A standard tag is an address, and in this image an address is its own offset.
    assert_eq!(
        a.table_offset(&standard, &a_info, 0x220).expect("resolves"),
        Some(0x220)
    );

    let vendor = module(Dialect::Vendor);
    let b = Container::parse(&vendor).expect("parses");
    let b_info = dynamic::DynamicInfo::parse(
        b.dynamic_bytes(&vendor)
            .expect("walks")
            .expect("has a table"),
    );
    // The same number under a vendor tag is an offset into the data segment instead.
    assert_eq!(
        b.table_offset(&vendor, &b_info, 0x220).expect("resolves"),
        Some(DATA_AT + 0x220)
    );
}

// --- imports ------------------------------------------------------------------------------

/// The import walk finds exactly the undefined, named symbols.
///
/// Three of the seven entries are excluded for three different reasons, and all three
/// exclusions have to work: the null entry, the symbol this module defines, and the one
/// with no name. A walk that got any of them wrong would still return a plausible list.
#[test]
fn only_undefined_named_symbols_are_imports() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");
    let imports = container.raw_imports(&bytes, &hasher()).expect("walks");

    let names: Vec<&str> = imports.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        ["H2e8t5ScQGc#B#C", "memcpy", "__stderrp", "a_thread_local"]
    );

    // The index is the link between a name and everything that refers to it numerically,
    // so it has to be the position in the table rather than the position in this list.
    let indices: Vec<u32> = imports.iter().map(|i| i.symbol_index).collect();
    assert_eq!(indices, [1, 2, 3, 5]);
}

/// A vendor-encoded name carries its own attribution; a plain one carries none.
///
/// **Not two optional ids that happen to be absent.** A plain name has no answer to give -
/// the format leaves which library exports a symbol to a search - and a `0` there would
/// attribute every homebrew import to whichever library came first (D305).
#[test]
fn an_encoded_name_carries_attribution_and_a_plain_one_does_not() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");
    let imports = container.raw_imports(&bytes, &hasher()).expect("walks");

    let encoded = &imports[0];
    assert_eq!(
        encoded.form,
        dynamic::NameForm::Encoded {
            library_id: 1,
            module_id: 2
        },
        "`B` and `C` are 1 and 2 in the NID alphabet"
    );
    assert_eq!(encoded.library_id(), Some(1));
    assert_eq!(encoded.module_id(), Some(2));

    for plain in &imports[1..] {
        assert_eq!(plain.form, dynamic::NameForm::Plain, "{}", plain.name);
        assert_eq!(plain.library_id(), None);
        assert_eq!(plain.module_id(), None);
    }
}

/// A plain name's hash is the hash of that name, so everything downstream resolves one way.
///
/// A plain name is not a second kind of import needing a second resolver - it is a NID
/// nobody has hashed yet, because the exporting library's NID *is* the hash of the same
/// name (D305).
#[test]
fn a_plain_name_hashes_to_the_nid_its_exporter_would_publish() {
    let bytes = module(Dialect::Standard);
    let hasher = hasher();
    let container = Container::parse(&bytes).expect("parses");
    let imports = container.raw_imports(&bytes, &hasher).expect("walks");

    let memcpy = imports
        .iter()
        .find(|i| i.name == "memcpy")
        .expect("present");
    assert_eq!(memcpy.nid, hasher.hash("memcpy").as_raw());

    // And the encoded one is decoded rather than hashed: its name is not what gets hashed,
    // so the two must differ.
    let encoded = &imports[0];
    assert_ne!(encoded.nid, hasher.hash(&encoded.name).as_raw());
    assert_ne!(encoded.nid, 0);
}

/// The kind is read from the symbol table rather than inferred from the name.
///
/// For data, a thunk address is not a wrong answer but no answer at all: the guest loads
/// the slot and dereferences what it finds, reading the first bytes of x86 instructions as
/// a pointer. That is indistinguishable from working until something unrelated breaks
/// (D307).
#[test]
fn the_kind_of_each_import_comes_from_the_symbol_table() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");
    let imports = container.raw_imports(&bytes, &hasher()).expect("walks");

    let kind = |name: &str| {
        imports
            .iter()
            .find(|i| i.name == name)
            .map(|i| i.kind)
            .expect("present")
    };
    assert_eq!(kind("memcpy"), dynamic::Kind::Function);
    assert_eq!(kind("__stderrp"), dynamic::Kind::Object);
    // TLS is neither, and saying so is a fact rather than a default.
    assert_eq!(kind("a_thread_local"), dynamic::Kind::Unspecified);
}

/// A module with no dynamic table refuses to report imports rather than reporting none.
///
/// An empty list reads as "needs nothing", which is never true of a real module - the exact
/// claim principle 3 forbids an import list from making.
#[test]
fn a_module_that_cannot_be_walked_refuses_rather_than_reporting_nothing() {
    let bytes = elf_with(&[(PT_LOAD, 0, 0, 0x100)], 0x100);
    let container = Container::parse(&bytes).expect("parses");
    assert!(matches!(
        container.raw_imports(&bytes, &hasher()),
        Err(ElfError::NoDynamicTable { .. })
    ));
}

/// A dynamic table missing the tables a walk needs is refused, and says which are missing.
#[test]
fn a_dynamic_table_without_its_tables_is_refused() {
    // A `PT_DYNAMIC` holding only `DT_NEEDED` and the terminator.
    let mut bytes = elf_with(
        &[(PT_LOAD, 0, 0, 0x400), (PT_DYNAMIC, 0x200, 0x200, 0x20)],
        0x400,
    );
    bytes[0x200..0x208].copy_from_slice(&dynamic::tag::NEEDED.to_le_bytes());

    let container = Container::parse(&bytes).expect("parses");
    match container.raw_imports(&bytes, &hasher()) {
        Err(ElfError::NoDynamicTable { reason }) => assert!(
            reason.contains("string table"),
            "the reason should name what was missing, not just fail: {reason}"
        ),
        other => panic!("expected a refusal naming the missing tables, got {other:?}"),
    }
}

/// A symbol count larger than any real module is refused before it becomes an allocation.
#[test]
fn an_absurd_symbol_count_is_refused() {
    let mut bytes = module(Dialect::Standard);
    let nchain_at = DATA_AT + HASH_OFF + 4;
    bytes[nchain_at..nchain_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());

    let container = Container::parse(&bytes).expect("parses");
    assert!(matches!(
        container.raw_imports(&bytes, &hasher()),
        Err(ElfError::AbsurdSymbolCount { .. })
    ));
    // The count itself is still reported: the limit belongs to the walk, not to the read.
    assert_eq!(
        container.symbol_count(&bytes).expect("reads"),
        u64::from(u32::MAX)
    );
}

/// A symbol count beyond the end of the table stops at the bytes that exist.
///
/// The count comes from the hash table and the table it describes comes from somewhere
/// else, so the two can disagree without the file being obviously malformed.
#[test]
fn a_count_larger_than_the_table_stops_at_what_is_there() {
    let mut bytes = module(Dialect::Standard);
    let nchain_at = DATA_AT + HASH_OFF + 4;
    bytes[nchain_at..nchain_at + 4].copy_from_slice(&5000_u32.to_le_bytes());

    let container = Container::parse(&bytes).expect("parses");
    let imports = container.raw_imports(&bytes, &hasher()).expect("walks");
    assert!(
        imports.len() < 5000,
        "the walk should stop at the end of the file, not run to the claimed count"
    );
}

/// A hash table that runs off the end of the file is a truncation, not a count of zero.
#[test]
fn a_truncated_hash_table_is_reported_rather_than_read_as_empty() {
    let mut bytes = module(Dialect::Standard);
    // Point `DT_HASH` at the last four bytes, so `nchain` is not there to read.
    let hash_tag_at = DYN_AT + 4 * DYN_ENTRY;
    assert_eq!(
        u64::from_le_bytes(
            bytes[hash_tag_at..hash_tag_at + 8]
                .try_into()
                .expect("8 bytes")
        ),
        dynamic::tag::HASH,
        "the fixture's fifth dynamic entry should be DT_HASH"
    );
    bytes[hash_tag_at + 8..hash_tag_at + 16].copy_from_slice(&((TOTAL - 4) as u64).to_le_bytes());

    let container = Container::parse(&bytes).expect("parses");
    match container.raw_imports(&bytes, &hasher()) {
        Err(ElfError::NoDynamicTable { reason }) => {
            assert!(reason.contains("truncated"), "unexpected reason: {reason}");
        }
        other => panic!("expected a truncated hash table, got {other:?}"),
    }
}

// --- the name tables ------------------------------------------------------------------------

/// The library and module tables are the ones an encoded name's ids actually index.
///
/// **Not `DT_NEEDED`**, which is a different list of a different length: indexing that
/// instead produced attributions that fit and meant nothing, like a graphics driver
/// exporting a socket function (D117).
#[test]
fn the_import_tables_are_keyed_by_the_ids_an_encoded_name_carries() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");

    let libraries = container.import_libraries(&bytes).expect("walks");
    let modules = container.import_modules(&bytes).expect("walks");
    assert_eq!(
        libraries.get(&1).map(String::as_str),
        Some("libSceLibcInternal")
    );
    assert_eq!(modules.get(&2).map(String::as_str), Some("libc"));

    // The encoded import's own ids reach them, which is the whole point of the pair.
    let imports = container.raw_imports(&bytes, &hasher()).expect("walks");
    let encoded = &imports[0];
    assert_eq!(
        encoded.library_id().and_then(|id| libraries.get(&id)),
        Some(&"libSceLibcInternal".to_owned())
    );
    assert_eq!(
        encoded.module_id().and_then(|id| modules.get(&id)),
        Some(&"libc".to_owned())
    );
}

/// `DT_NEEDED` is a separate list, and reading it does not answer the attribution question.
#[test]
fn needed_libraries_is_a_different_list_from_the_import_table() {
    let bytes = module(Dialect::Standard);
    let container = Container::parse(&bytes).expect("parses");

    let needed = container.needed_libraries(&bytes).expect("walks");
    assert_eq!(needed, ["libkernel.sprx"]);

    let libraries = container.import_libraries(&bytes).expect("walks");
    assert!(
        !libraries.values().any(|name| name == "libkernel.sprx"),
        "the two lists hold different things, which is why both exist"
    );
}

/// Reading a name out of the string table stops at the terminator and tolerates a bad
/// offset.
#[test]
fn a_name_offset_past_the_string_table_yields_nothing() {
    let table = b"\0alpha\0beta\0";
    assert_eq!(dynamic::read_cstr(table, 1), Some("alpha"));
    assert_eq!(dynamic::read_cstr(table, 7), Some("beta"));
    assert_eq!(dynamic::read_cstr(table, 0), Some(""));
    assert_eq!(dynamic::read_cstr(table, 999), None);
}

/// A vendor table entry packs an id and a name offset into one value.
#[test]
fn a_table_entry_splits_into_an_id_and_an_offset() {
    assert_eq!(
        dynamic::split_table_entry((7_u64 << 48) | 0x1234),
        (7, 0x1234)
    );
    assert_eq!(dynamic::split_table_entry(0), (0, 0));
    // The middle sixteen bits are a version, and belong to neither half.
    assert_eq!(
        dynamic::split_table_entry((3_u64 << 48) | (0xFFFF << 32) | 9),
        (3, 9)
    );
}
