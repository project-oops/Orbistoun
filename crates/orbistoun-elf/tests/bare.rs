//! Address translation in a container with no wrapper.
//!
//! # Why these exist
//!
//! Because every one of them passed vacuously before, by returning "cannot locate that"
//! for every address of every unwrapped module. The container parsed, the loader mapped
//! its segments, and then nothing downstream could find a single byte - reported as *no
//! `PT_DYNAMIC` segment, or its address could not be located* about a module that had one
//! sitting inside a mapped `PT_LOAD` (D237).
//!
//! The modules this matters for are the ones nobody can sign: a conformance probe emits a
//! bare ELF because it has no other option, and that is the guest most worth being able to
//! run repeatedly.

use orbistoun_elf::Container;

const EHDR: usize = 64;
const PHDR: usize = 56;

/// Builds a bare ELF from a list of `(p_type, offset, vaddr, filesz)`.
fn bare_elf(headers: &[(u32, u64, u64, u64)], body: usize) -> Vec<u8> {
    let phoff = EHDR;
    let start = phoff + PHDR * headers.len();
    let mut elf = vec![0u8; start.max(body)];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; // 64-bit
    elf[5] = 1; // little endian
    elf[6] = 1; // version
    elf[7] = 9; // FreeBSD, as a target module carries
    elf[8] = 2; // ABI version - a generation-5 module
    elf[16..18].copy_from_slice(&0xfe18u16.to_le_bytes());
    elf[18..20].copy_from_slice(&62u16.to_le_bytes()); // x86-64
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[32..40].copy_from_slice(&(phoff as u64).to_le_bytes());
    elf[54..56].copy_from_slice(&(PHDR as u16).to_le_bytes());
    elf[56..58].copy_from_slice(&(headers.len() as u16).to_le_bytes());

    for (index, (p_type, offset, vaddr, size)) in headers.iter().enumerate() {
        let at = phoff + PHDR * index;
        elf[at..at + 4].copy_from_slice(&p_type.to_le_bytes());
        elf[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        elf[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
        elf[at + 32..at + 40].copy_from_slice(&size.to_le_bytes());
        elf[at + 40..at + 48].copy_from_slice(&size.to_le_bytes());
    }
    elf
}

/// One `PT_LOAD` at 0x1000, backed by file offset 0x200, with a `PT_DYNAMIC` inside it -
/// the shape a conformance probe's module actually has.
fn probe_shaped() -> Vec<u8> {
    bare_elf(
        &[(1, 0x200, 0x1000, 0x200), (2, 0x300, 0x1100, 0x100)],
        0x400,
    )
}

#[test]
fn an_address_inside_a_load_resolves_through_its_file_offset() {
    let bytes = probe_shaped();
    let container = Container::parse(&bytes).expect("parses");
    assert!(container.wrapper().is_none(), "this fixture is a bare ELF");
    assert_eq!(
        container.vaddr_to_offset(&bytes, 0x1000).expect("resolves"),
        Some(0x200),
        "the start of the segment is its own file offset"
    );
    assert_eq!(
        container.vaddr_to_offset(&bytes, 0x1100).expect("resolves"),
        Some(0x300),
        "an address part-way in is the offset plus the distance"
    );
}

/// The failure that started this: a dynamic table that is present and was unreachable.
#[test]
fn a_dynamic_table_inside_a_load_can_be_found() {
    let bytes = probe_shaped();
    let container = Container::parse(&bytes).expect("parses");
    let dynamic = container
        .dynamic_bytes(&bytes)
        .expect("no error")
        .expect("the table is there and reachable");
    assert_eq!(dynamic.len(), 0x100, "the whole declared table came back");
}

/// An address outside every segment is still unlocatable, which is the honest answer.
#[test]
fn an_address_in_no_segment_resolves_to_nothing() {
    let bytes = probe_shaped();
    let container = Container::parse(&bytes).expect("parses");
    assert_eq!(
        container.vaddr_to_offset(&bytes, 0x9000).expect("no error"),
        None
    );
}

/// A header describing more than the file holds does not resolve past the end of it.
///
/// Truncated containers exist, and the difference between "cannot locate that" and an
/// index past the end of a slice is the difference between a report and a panic.
#[test]
fn a_header_pointing_past_the_end_of_the_file_locates_nothing() {
    let bytes = bare_elf(&[(1, 0x9000, 0x1000, 0x200)], 0x400);
    let container = Container::parse(&bytes).expect("parses");
    assert_eq!(
        container.vaddr_to_offset(&bytes, 0x1000).expect("no error"),
        None,
        "an offset beyond the file is not a position in it"
    );
}

/// When a vendor segment and a `PT_LOAD` both claim an address, the `PT_LOAD` wins.
///
/// Both genuinely cover it: a vendor segment carrying dynamic data is commonly declared at
/// virtual address zero, and a module whose first `PT_LOAD` also starts at zero then has
/// two headers over the same low range. An address in the image means the image.
#[test]
fn a_load_is_preferred_over_a_vendor_segment_claiming_the_same_address() {
    const SCE_DYNLIBDATA: u32 = 0x6100_0000;
    let bytes = bare_elf(
        &[(SCE_DYNLIBDATA, 0x300, 0x0, 0x100), (1, 0x200, 0x0, 0x200)],
        0x400,
    );
    let container = Container::parse(&bytes).expect("parses");
    assert_eq!(
        container.vaddr_to_offset(&bytes, 0x10).expect("resolves"),
        Some(0x210),
        "the load segment's file offset, not the vendor segment's"
    );
}

/// A vendor segment is still reachable when nothing else covers the address.
#[test]
fn a_vendor_segment_resolves_when_no_load_covers_the_address() {
    const SCE_DYNLIBDATA: u32 = 0x6100_0000;
    let bytes = bare_elf(
        &[
            (1, 0x200, 0x1000, 0x100),
            (SCE_DYNLIBDATA, 0x300, 0x5000, 0x100),
        ],
        0x400,
    );
    let container = Container::parse(&bytes).expect("parses");
    assert_eq!(
        container.vaddr_to_offset(&bytes, 0x5020).expect("resolves"),
        Some(0x320)
    );
}

/// An unwrapped container reports no wrapper-located segments, and that stays true.
///
/// It is not the defect it looked like from outside: program headers address the file
/// directly when there is no wrapper, so there is nothing for a descriptor table to locate.
/// The bug was never here.
#[test]
fn a_bare_container_still_reports_no_wrapper_mapped_segments() {
    let bytes = probe_shaped();
    let container = Container::parse(&bytes).expect("parses");
    assert!(
        container
            .mapped_program_headers(&bytes)
            .expect("no error")
            .is_empty(),
        "a bare ELF has no descriptor table, so it locates nothing through one"
    );
}

/// **A vendor hash table at offset zero is a table, not a missing tag.**
///
/// D247 is the entry about this: a vendor `DT_` value is an offset into
/// `PT_SCE_DYNLIBDATA`, and offset zero is the first byte of it - where a real module puts
/// a table. D305 added a `DT_GNU_HASH` fallback and guarded it with `info.hash != 0`,
/// which reintroduced exactly that bug for any module whose hash sits at the front. It was
/// caught by writing this, which is the point of writing it.
#[test]
fn a_vendor_hash_table_at_offset_zero_is_still_found() {
    /// `PT_SCE_DYNLIBDATA`.
    const VENDOR_DATA: u32 = 0x6100_0000;
    const SCE_HASH: u64 = 0x6100_0025;
    const SCE_STRTAB: u64 = 0x6100_0035;
    const SCE_SYMTAB: u64 = 0x6100_0039;

    // A load, a dynamic table inside it, and the vendor data segment at file offset 0x400.
    let mut bytes = bare_elf(
        &[
            (1, 0x200, 0x1000, 0x200),
            (2, 0x300, 0x1100, 0x100),
            (VENDOR_DATA, 0x400, 0x2000, 0x100),
        ],
        0x500,
    );

    // The dynamic table: every vendor tag named, and the hash **at offset zero**.
    let mut at = 0x300;
    for (tag, value) in [(SCE_HASH, 0_u64), (SCE_STRTAB, 0x40), (SCE_SYMTAB, 0x80)] {
        bytes[at..at + 8].copy_from_slice(&tag.to_le_bytes());
        bytes[at + 8..at + 16].copy_from_slice(&value.to_le_bytes());
        at += 16;
    }

    // The hash table itself, at the front of the vendor segment: [nbucket][nchain].
    bytes[0x400..0x404].copy_from_slice(&1_u32.to_le_bytes());
    bytes[0x404..0x408].copy_from_slice(&3_u32.to_le_bytes());

    let container = Container::parse(&bytes).expect("parses");
    assert_eq!(
        container.symbol_count(&bytes).expect("no error"),
        3,
        "nchain is the count, and offset zero is where the table is - not an absent tag"
    );
}

/// The shape a **real title** has, where two segments claim address zero.
///
/// ```text
/// PT_SCE_DYNLIBDATA  off 0x8c130  filesz 0x3760  vaddr 0   -> ends 0x8f890
/// PT_DYNAMIC         off 0x8f450  filesz 0x0440  vaddr 0   -> ends 0x8f890
/// ```
///
/// Scaled down, and with the vendor segment first in the table so that a reader resolving
/// `PT_DYNAMIC` *by address* picks the wrong one - which is what the file layout does on real
/// hardware, and what nothing in this repository could load until it was measured there
/// (D391).
fn title_shaped() -> Vec<u8> {
    const PT_LOAD: u32 = 1;
    const PT_DYNAMIC: u32 = 2;
    const PT_SCE_DYNLIBDATA: u32 = 0x6100_0000;
    bare_elf(
        &[
            (PT_LOAD, 0x200, 0x1000, 0x100),
            // Both at address zero, and this one comes first.
            (PT_SCE_DYNLIBDATA, 0x300, 0, 0x100),
            (PT_DYNAMIC, 0x380, 0, 0x80),
        ],
        0x400,
    )
}

/// **The dynamic table is found by its own file offset, not by address.**
///
/// The failure this protects against is silent: resolving by address returns the *vendor*
/// segment's bytes, which parse as a dynamic table of nonsense rather than as an error.
#[test]
fn a_dynamic_table_with_no_address_is_still_found() {
    let bytes = title_shaped();
    let container = Container::parse(&bytes).expect("parses");
    let dynamic = container
        .dynamic_bytes(&bytes)
        .expect("walks")
        .expect("a title's dynamic table is at its own file offset");

    assert_eq!(dynamic.len(), 0x80, "the dynamic segment's own size");
    let at = dynamic.as_ptr() as usize - bytes.as_ptr() as usize;
    assert_eq!(
        at, 0x380,
        "and its own offset - 0x300 would be the vendor segment, which also claims address zero"
    );
}

/// Resolving *by address* is exactly the ambiguity, kept as a statement about the file.
///
/// Two segments cover address zero, so the question has no single answer - which is why the
/// dynamic table is not found that way any more.
#[test]
fn address_zero_is_claimed_by_two_segments_in_a_title() {
    let bytes = title_shaped();
    let container = Container::parse(&bytes).expect("parses");
    let by_address = container.vaddr_to_offset(&bytes, 0).expect("resolves");
    assert!(
        by_address == Some(0x300) || by_address == Some(0x380),
        "address zero lands in one of the two segments that claim it, and which one is          nothing but header order: {by_address:?}"
    );
}
