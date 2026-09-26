//! Reading a guest module's own `SHT_SYMTAB`, and refusing to invent one.
//!
//! Open-toolchain guests are unstripped and commercial titles are stripped, so the reader
//! returns a list of functions for the first and an empty list, with no error, for the
//! second.

use orbistoun_elf::Container;

/// A module with no section table answers empty, not an error.
///
/// Every commercial title is this case. An error would push callers to swallow it, and a
/// real parse failure would then read as "stripped".
#[test]
fn a_module_without_a_section_table_names_nothing_and_does_not_fail() {
    let bytes = minimal_elf(0, 0);
    let container = Container::parse(&bytes).expect("a minimal ELF64 header parses");
    let found = container
        .function_symbols(&bytes)
        .expect("no section table is an ordinary state");
    assert!(
        found.is_empty(),
        "a stripped module names no functions, and says so by being empty"
    );
}

/// A section table pointing off the end of the file names nothing.
///
/// The only acceptable answers to hostile bytes are "nothing" and "these symbols": never a
/// panic and never a read past the end.
#[test]
fn a_section_table_past_the_end_of_the_file_names_nothing() {
    // Twenty-four headers at an offset well beyond a 64-byte file.
    let bytes = minimal_elf(0x0010_0000, 24);
    let container = Container::parse(&bytes).expect("the header itself is still well formed");
    let found = container
        .function_symbols(&bytes)
        .expect("an unreadable section table is not an error, it is nothing");
    assert!(
        found.is_empty(),
        "an offset outside the file yields no symbols rather than a panic"
    );
}

/// The conformance payload, when present, names its own functions.
///
/// Skipped when the corpus has not been fetched, since it is not committed. It is the one
/// test on a genuine `SHT_SYMTAB`, and it asserts on a name the probe's source defines, so a
/// mis-walked table fails rather than passing on a non-empty list.
#[test]
fn the_conformance_payload_names_its_own_functions() {
    let path = std::path::Path::new("../../titles/obscene-payload/eboot.bin");
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!(
            "{} is not here - corpus not fetched, skipping",
            path.display()
        );
        return;
    };
    let container = Container::parse(&bytes).expect("the payload is a well-formed ELF64");
    let found = container
        .function_symbols(&bytes)
        .expect("the payload carries a section table");
    assert!(
        found.iter().any(|s| s.name == "obs_run_all"),
        "the probe's own suite entry point is in its symbol table, and this found {} symbol(s)",
        found.len()
    );
    assert!(
        found.iter().all(|s| s.value != 0),
        "a symbol at address zero names nothing and must not be listed"
    );
}

/// A 64-byte ELF64 header with the section table fields set as asked.
fn minimal_elf(shoff: u64, shnum: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2; // 64-bit
    bytes[5] = 1; // little-endian
    bytes[6] = 1; // version
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
    bytes[18..20].copy_from_slice(&0x3eu16.to_le_bytes()); // x86-64
    bytes[40..48].copy_from_slice(&shoff.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    bytes[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
    bytes[60..62].copy_from_slice(&shnum.to_le_bytes());
    bytes
}
