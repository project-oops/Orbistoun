//! Dumping every dynamic tag a real module carries, known or not.
//!
//! The import library table is somewhere in here. orbistoun has been indexing
//! `DT_NEEDED` with ids that do not fit it - 52 entries against ids running to 54 - so
//! the ids belong to a table this crate does not read (D117).
//!
//! Rather than guess which tag it is, this prints all of them with enough context to
//! recognise one: the raw tag, how many times it appears, and - where the value looks
//! like a string-table offset - the string it points at. A tag whose values resolve to
//! library names, in the right quantity, is the answer and will be obvious.
//!
//! That question is answered - `SCE_IMPORT_LIB` and `SCE_IMPORT_MODULE` are now parsed
//! properly - but this is kept, because the *next* unknown tag will be found the same
//! way and there is no cheaper instrument for it.
//!
//! Nothing was read to write this. It prints what the file contains.

use std::collections::BTreeMap;

use orbistoun_elf::Container;
use orbistoun_elf::dynamic::{DYNAMIC_ENTRY_SIZE, read_cstr, tag};

/// Tags the standard defines, so the output can lead with what is *not* one of them.
fn standard_name(raw: u64) -> Option<&'static str> {
    Some(match raw {
        tag::NEEDED => "NEEDED",
        tag::HASH => "HASH",
        tag::STRTAB => "STRTAB",
        tag::SYMTAB => "SYMTAB",
        tag::STRSZ => "STRSZ",
        tag::SYMENT => "SYMENT",
        tag::RELA => "RELA",
        tag::RELASZ => "RELASZ",
        tag::JMPREL => "JMPREL",
        tag::PLTRELSZ => "PLTRELSZ",
        tag::SCE_IMPORT_LIB => "SCE_IMPORT_LIB",
        tag::SCE_IMPORT_MODULE => "SCE_IMPORT_MODULE",
        3 => "PLTGOT",
        9 => "RELAENT",
        12 => "INIT",
        13 => "FINI",
        14 => "SONAME",
        20 => "PLTREL",
        21 => "DEBUG",
        22 => "TEXTREL",
        24 => "BIND_NOW",
        25 => "INIT_ARRAY",
        26 => "FINI_ARRAY",
        27 => "INIT_ARRAYSZ",
        28 => "FINI_ARRAYSZ",
        30 => "FLAGS",
        0x6FFF_FFFB => "FLAGS_1",
        0x6FFF_FFF0 => "VERSYM",
        0x6FFF_FFFE => "VERNEED",
        0x6FFF_FFFF => "VERNEEDNUM",
        _ => return None,
    })
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: dyntags <container>");
    let bytes = std::fs::read(&path).expect("read the container");
    let container = Container::parse(&bytes).expect("parse");

    let dyn_bytes = container
        .dynamic_bytes(&bytes)
        .expect("dynamic")
        .expect("a dynamic table");
    let info = orbistoun_elf::dynamic::DynamicInfo::parse(dyn_bytes);

    let strtab_at = container
        .vaddr_to_offset(&bytes, info.strtab)
        .expect("offset")
        .expect("a string table");
    let strsz = usize::try_from(info.strsz).unwrap_or(0);
    let strings = bytes.get(strtab_at..strtab_at + strsz).unwrap_or(&[]);
    println!("string table: {strsz} bytes at file offset {strtab_at:#x}\n");

    // Every entry, grouped by tag, so a table with 55 entries stands out from a tag that
    // appears once.
    let mut by_tag: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for chunk in dyn_bytes.chunks_exact(DYNAMIC_ENTRY_SIZE) {
        let t = u64::from_le_bytes(chunk[..8].try_into().unwrap_or_default());
        let v = u64::from_le_bytes(chunk[8..].try_into().unwrap_or_default());
        if t == 0 {
            break;
        }
        by_tag.entry(t).or_default().push(v);
    }

    println!("{:<14} {:>6}  WHAT THE VALUES LOOK LIKE", "TAG", "COUNT");
    for (t, values) in &by_tag {
        let known = standard_name(*t).unwrap_or("--");
        // The vendor packs an id and a string offset into one value for its own tables,
        // so both halves are worth resolving before deciding a tag is uninteresting.
        let whole: Vec<&str> = values
            .iter()
            .filter_map(|v| read_cstr(strings, usize::try_from(*v).ok()?))
            .collect();
        let low: Vec<&str> = values
            .iter()
            .filter_map(|v| read_cstr(strings, usize::try_from(*v & 0xFFFF_FFFF).ok()?))
            .collect();

        let note = if whole.len() == values.len() && !whole.is_empty() {
            format!(
                "all resolve as strings, e.g. {:?}",
                &whole[..whole.len().min(3)]
            )
        } else if low.len() == values.len() && !low.is_empty() {
            format!(
                "low word resolves, e.g. {:?}  (high word = {:?})",
                &low[..low.len().min(3)],
                values.iter().take(3).map(|v| v >> 32).collect::<Vec<_>>()
            )
        } else {
            format!("e.g. {:#x?}", values.iter().take(3).collect::<Vec<_>>())
        };
        println!("{t:#014x} {known:<6} {:>6}  {note}", values.len());
    }
}
