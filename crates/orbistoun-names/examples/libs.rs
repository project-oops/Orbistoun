//! Checking whether import library attribution is real or merely plausible.
//!
//! An encoded symbol name carries two small indices alongside its hash. orbistoun has
//! been treating the first as an index into the module's `DT_NEEDED` list, and the
//! result *looks* fine - every index is in range and every import gets a library name.
//!
//! It is also visibly wrong: a graphics driver library does not export `setsockopt`.
//! This dumps the raw indices so the question can be settled by counting rather than by
//! reading attributions and being unconvinced by them.

use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: libs <container>");
    let bytes = std::fs::read(&path).expect("read the container");
    let container = orbistoun_elf::Container::parse(&bytes).expect("parse");
    // The shipped suffix: this example asks about attribution, not about hashing, and a
    // name that hashes to nothing still carries the ids being counted here.
    let hasher = orbistoun_nid::NidHasher::new(orbistoun_nid::default_suffix());
    let imports = container.raw_imports(&bytes, &hasher).expect("imports");
    let needed = container.needed_libraries(&bytes).expect("needed");

    let mut by_library: BTreeMap<u16, usize> = BTreeMap::new();
    let mut by_module: BTreeMap<u16, usize> = BTreeMap::new();
    // Plain names carry no ids at all, and counting them as zero would invent the very
    // attribution this example exists to check.
    for import in &imports {
        if let Some(id) = import.library_id() {
            *by_library.entry(id).or_default() += 1;
        }
        if let Some(id) = import.module_id() {
            *by_module.entry(id).or_default() += 1;
        }
    }

    println!("imports              {}", imports.len());
    println!("DT_NEEDED entries    {}", needed.len());
    println!("distinct library ids {}", by_library.len());
    println!("distinct module ids  {}", by_module.len());
    println!(
        "library id range     {:?}..={:?}",
        by_library.keys().next(),
        by_library.keys().next_back()
    );
    println!(
        "module id range      {:?}..={:?}",
        by_module.keys().next(),
        by_module.keys().next_back()
    );

    // If the ids really indexed this list, the busiest ids would land on libraries a
    // program plausibly leans on. Printing the mapping makes a wrong one obvious.
    println!("\nbusiest library ids, and what DT_NEEDED says they are:");
    let mut ranked: Vec<(&u16, &usize)> = by_library.iter().collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    for (id, count) in ranked.iter().take(12) {
        let name = needed
            .get(**id as usize)
            .map_or("<out of range>", String::as_str);
        println!("  id {id:>3}  {count:>5} imports  -> {name}");
    }

    println!("\nfirst ten DT_NEEDED entries, in declaration order:");
    for (i, name) in needed.iter().take(10).enumerate() {
        println!("  [{i}] {name}");
    }
}
