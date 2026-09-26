//! Two readers over one corpus, and every field where they disagree.
//!
//! `orbistoun-elf` and `selfish-elf` encode the same knowledge twice and have drifted. A
//! parser has no oracle, so a second parser written from the same facts is the cheap check
//! on each (D653). This reports disagreements and asserts nothing about which side is right:
//! deciding that needs a person and a third source. The modules are installed titles, not
//! repository fixtures; when the corpus is absent the test prints what it looked for and
//! skips, since a silent pass would read as agreement.

use std::path::{Path, PathBuf};

/// One field, as each reader read it.
struct Disagreement {
    field: &'static str,
    ours: String,
    theirs: String,
    /// Whether this is a difference in units rather than in what the two readers found.
    ///
    /// Counted apart so the headline counts real disagreements: every module with encoded
    /// imports differs on NID byte order. The line still prints.
    units: bool,
}

/// Compares the tables, once both readers have found one.
///
/// Split from [`compare`] so whether a file has a vendor dynamic table and what is in it stay
/// separate questions; the first is where the readers differ.
fn compare_tables(
    our_info: &orbistoun_elf::dynamic::DynamicInfo,
    their_info: &selfish_elf::dynamic::Info,
    base: u64,
    found: &mut Vec<Disagreement>,
) {
    let mut note = |field, ours: String, theirs: String| {
        if ours != theirs {
            found.push(Disagreement {
                field,
                ours,
                theirs,
                units: false,
            });
        }
    };
    // Legacy, not `is_some`: orbistoun's field says its values came from vendor tags, while
    // SELFish's says which convention the file follows, and `Current` means standard tables
    // from standard tags, exactly when orbistoun answers `false`.
    note(
        "table values came from vendor tags",
        our_info.vendor_tables.to_string(),
        matches!(their_info.table, Some(selfish_elf::dynamic::Table::Orbis)).to_string(),
    );
    // Rebased into orbistoun's units before comparing: SELFish measures from the slice it
    // returns, orbistoun from the image.
    for (field, ours, theirs) in [
        ("strtab", our_info.strtab, base + their_info.strtab),
        ("strsz", our_info.strsz, their_info.strsz),
        ("symtab", our_info.symtab, base + their_info.symtab),
        ("syment", our_info.syment, their_info.syment),
        ("hash", our_info.hash, base + their_info.hash),
        ("rela", our_info.rela, base + their_info.rela),
        ("relasz", our_info.relasz, their_info.relasz),
        ("jmprel", our_info.jmprel, base + their_info.jmprel),
        ("pltrelsz", our_info.pltrelsz, their_info.pltrelsz),
    ] {
        note(field, format!("{ours:#x}"), format!("{theirs:#x}"));
    }
    // The packed entries themselves, not only how many: equal-length tables holding different
    // ids answer every count identically and every lookup differently.
    for (field, ours, theirs) in [
        (
            "import library entries",
            &our_info.libraries,
            &their_info.import_libs,
        ),
        (
            "module table entries",
            &our_info.modules,
            &their_info.needed_modules,
        ),
        ("DT_NEEDED entries", &our_info.needed, &their_info.needed),
    ] {
        note(field, format!("{ours:x?}"), format!("{theirs:x?}"));
    }
}

/// The virtual address the slice `Elf::tables` returns is based at.
///
/// SELFish returns table positions as offsets into the byte slice it hands back; orbistoun
/// returns virtual addresses. The base is found by matching the slice's file offset against
/// the program headers, so it is read from the file rather than assumed.
fn slice_base(elf: &selfish_elf::Elf<'_>, whole: &[u8], slice: &[u8]) -> u64 {
    let at = (slice.as_ptr() as usize).saturating_sub(whole.as_ptr() as usize) as u64;
    elf.program_headers()
        .iter()
        .find(|p| p.offset.get() == at)
        .map_or(0, |p| p.vaddr.get())
}
/// The bytes to hand SELFish's reader, for a module inside a signed container.
///
/// `selfish_container` splices the segment payloads from the container's entry list back into
/// the ELF, so SELFish reads the whole file. An inner ELF unwrapped by orbistoun has program
/// headers whose payloads live elsewhere, and SELFish refuses it.
fn readable_by_selfish(bytes: &[u8]) -> Option<Vec<u8>> {
    selfish_container::Container::parse(bytes)
        .ok()?
        .to_elf()
        .ok()
}
fn compare(outer: &[u8]) -> Result<Vec<Disagreement>, String> {
    // Each reader gets the bytes its own entry point expects: orbistoun's `Container` unwraps
    // the signed container itself, and SELFish's `Elf` wants the spliced ELF. Both read the same
    // file, which is what makes this a differential.
    let ours = orbistoun_elf::Container::parse(outer).map_err(|e| format!("orbistoun: {e}"))?;
    let spliced = readable_by_selfish(outer);
    let for_them: &[u8] = spliced.as_deref().unwrap_or(outer);
    let theirs = selfish_elf::Elf::parse(for_them).map_err(|e| format!("selfish: {e}"))?;

    let our_dyn = ours
        .dynamic_bytes(outer)
        .map_err(|e| format!("orbistoun dynamic: {e}"))?;
    let their_tables = theirs
        .tables()
        .map_err(|e| format!("selfish tables: {e}"))?
        .map(|(slice, info)| (slice_base(&theirs, for_them, slice), slice, info));
    let mut found = Vec::new();
    let mut note = |field, ours: String, theirs: String| {
        if ours != theirs {
            found.push(Disagreement {
                field,
                ours,
                theirs,
                units: false,
            });
        }
    };

    // The no-vendor-tables case first. A plain freestanding ELF has no vendor dynamic table,
    // and a reader that finds one is reading a standard tag as a vendor tag.
    let (our_info, their_info, base, segment) = match (our_dyn, their_tables) {
        (Some(dyn_bytes), Some((base, segment, their_info))) => (
            orbistoun_elf::dynamic::DynamicInfo::parse(dyn_bytes),
            their_info,
            base,
            segment,
        ),
        (ours_had, theirs_had) => {
            // Like for like: `dynamic_bytes` answers "is there a `PT_DYNAMIC`", which a plain
            // freestanding ELF has; `tables()` answers "are there resolvable vendor tables",
            // which it has not.
            let ours_vendor = ours.vendor_segments().is_ok_and(|v| !v.is_empty());
            note(
                "carries the vendor dynamic segment",
                ours_vendor.to_string(),
                theirs_had.is_some().to_string(),
            );
            let _ = ours_had;
            return Ok(found);
        }
    };
    compare_tables(&our_info, &their_info, base, &mut found);
    let (same_order, swapped) = compare_symbols(&ours, outer, segment, &their_info, &mut found)?;
    compare_relocations(&ours, outer, &our_info, segment, &their_info, &mut found);
    if swapped > 0 && same_order == 0 {
        found.push(Disagreement {
            units: true,
            field: "NID byte order",
            ours: format!("{swapped} import(s) match only byte-reversed"),
            theirs: "the same hashes, read the other way".to_owned(),
        });
    }
    Ok(found)
}

/// The symbol level: counts, import triples, and the relocation census.
///
/// Three differences in meaning, each taken from the two crates' own source:
///
/// - The NID is byte-reversed between the two. Orbistoun prints the big-endian read of the
///   hash's first eight bytes, SELFish the little-endian one.
/// - SELFish's `imports` returns encoded names only. A plain undefined symbol is carried here
///   as `NameForm::Plain`, so plain imports are counted apart and reported, not compared.
/// - Orbistoun carries raw library and module ids and resolves them through its own tables;
///   SELFish resolves them. The names are compared, since a wrong id gives a name that fits
///   and means nothing.
fn compare_symbols(
    ours: &orbistoun_elf::Container<'_>,
    outer: &[u8],
    segment: &[u8],
    their_info: &selfish_elf::dynamic::Info,
    found: &mut Vec<Disagreement>,
) -> Result<(usize, usize), String> {
    // The shipped suffix, so a plain name hashes as the rest of this project computes it. Only
    // encoded imports are compared, but a wrong suffix would change the plain count.
    let hasher = orbistoun_nid::NidHasher::new(orbistoun_nid::default_suffix());
    let our_imports = ours
        .raw_imports(outer, &hasher)
        .map_err(|e| format!("orbistoun imports: {e}"))?;
    let their_imports = selfish_elf::dynamic::imports(segment, their_info)
        .map_err(|e| format!("selfish imports: {e}"))?;
    let our_libs = ours
        .import_libraries(outer)
        .map_err(|e| format!("orbistoun libraries: {e}"))?;
    let our_mods = ours
        .import_modules(outer)
        .map_err(|e| format!("orbistoun modules: {e}"))?;

    let mut note = |field: &'static str, ours: String, theirs: String| {
        if ours != theirs {
            found.push(Disagreement {
                field,
                ours,
                theirs,
                units: false,
            });
        }
    };

    // Two derivations of one number: orbistoun takes the symbol count from the hash table's
    // `nchain`, SELFish divides `symtabsz` by `syment`. A disagreement means one is wrong.
    match (ours.symbol_count(outer), their_info.symbol_count()) {
        (Ok(our_count), Some(their_count)) => note(
            "dynamic symbol count",
            our_count.to_string(),
            their_count.to_string(),
        ),
        // Said rather than skipped: a comparison that never ran would read as agreement.
        (ours_said, theirs_said) => note(
            "dynamic symbol count could not be compared",
            format!("{ours_said:?}"),
            format!("{theirs_said:?}"),
        ),
    }

    let encoded: Vec<&orbistoun_elf::dynamic::RawImport> = our_imports
        .iter()
        .filter(|i| matches!(i.form, orbistoun_elf::dynamic::NameForm::Encoded { .. }))
        .collect();
    note(
        "encoded import count",
        encoded.len().to_string(),
        their_imports.len().to_string(),
    );

    // Joined on the symbol index, which is what a relocation names.
    let theirs_by_index: std::collections::BTreeMap<u32, &selfish_elf::dynamic::Import<'_>> =
        their_imports.iter().map(|i| (i.index, i)).collect();
    let tally = tally_triples(&encoded, &theirs_by_index, &our_libs, &our_mods);
    if let Some((index, ours_said, theirs_said)) = tally.first {
        note(
            "first differing import triple",
            format!("index {index} {ours_said}"),
            format!("index {index} {theirs_said}"),
        );
    }
    if tally.mismatched > 0 {
        note(
            "import triples differing",
            tally.mismatched.to_string(),
            "0".to_owned(),
        );
    }
    if tally.only_ours > 0 {
        note(
            "encoded imports at an index the other has no import for",
            tally.only_ours.to_string(),
            "0".to_owned(),
        );
    }
    // Reported as a field only when the two orders are mixed within one module, which means
    // something is wrong in one reader. A module wholly one way is a units fact, stated once.
    if tally.same_order > 0 && tally.swapped > 0 {
        note(
            "NID byte order is not consistent within the module",
            format!("{} same, {} swapped", tally.same_order, tally.swapped),
            "one order throughout".to_owned(),
        );
    }
    Ok((tally.same_order, tally.swapped))
}

/// One import as both readers describe it, in the units each uses.
struct Triple {
    nid: u64,
    library: Option<String>,
    module: Option<String>,
}

/// How many imports match by hash in each byte order, and how many differ in their attribution.
struct TripleTally {
    same_order: usize,
    swapped: usize,
    mismatched: usize,
    only_ours: usize,
    first: Option<(u32, String, String)>,
}

/// Walks the two import lists, joined on symbol index.
///
/// Split from [`compare_symbols`] to keep each under the line limit; the join carries the
/// three unit differences.
fn tally_triples(
    encoded: &[&orbistoun_elf::dynamic::RawImport],
    theirs_by_index: &std::collections::BTreeMap<u32, &selfish_elf::dynamic::Import<'_>>,
    our_libs: &std::collections::BTreeMap<u16, String>,
    our_mods: &std::collections::BTreeMap<u16, String>,
) -> TripleTally {
    let mut tally = TripleTally {
        same_order: 0,
        swapped: 0,
        mismatched: 0,
        only_ours: 0,
        first: None,
    };
    for import in encoded {
        let Some(theirs) = theirs_by_index.get(&import.symbol_index) else {
            tally.only_ours += 1;
            continue;
        };
        let orbistoun_elf::dynamic::NameForm::Encoded {
            library_id,
            module_id,
        } = import.form
        else {
            continue;
        };
        // The byte order is measured, not assumed: both orders are tried and the one that
        // agreed is counted.
        let theirs_nid = theirs.nid.value();
        if import.nid == theirs_nid {
            tally.same_order += 1;
        } else if import.nid == theirs_nid.swap_bytes() {
            tally.swapped += 1;
        }
        let ours = Triple {
            nid: import.nid,
            library: our_libs.get(&library_id).cloned(),
            module: our_mods.get(&module_id).cloned(),
        };
        let them = Triple {
            nid: theirs_nid,
            library: theirs.library.map(ToOwned::to_owned),
            module: theirs.module.map(ToOwned::to_owned),
        };
        let hash_agrees = ours.nid == them.nid || ours.nid == them.nid.swap_bytes();
        if hash_agrees && ours.library == them.library && ours.module == them.module {
            continue;
        }
        if tally.first.is_none() {
            tally.first = Some((
                import.symbol_index,
                format!("{:#x} {:?} {:?}", ours.nid, ours.library, ours.module),
                format!("{:#x} {:?} {:?}", them.nid, them.library, them.module),
            ));
        }
        tally.mismatched += 1;
    }
    tally
}

/// The relocation census, by type, for both tables.
///
/// Counted by type rather than entry by entry: any difference means one reader walks the table
/// wrongly. The type matters most, since a relocation of the wrong kind writes an address into
/// a slot the guest reads as data, and neither reader reports it.
fn compare_relocations(
    ours: &orbistoun_elf::Container<'_>,
    outer: &[u8],
    our_info: &orbistoun_elf::dynamic::DynamicInfo,
    segment: &[u8],
    their_info: &selfish_elf::dynamic::Info,
    found: &mut Vec<Disagreement>,
) {
    let our_table = |at: u64, size: u64| -> Vec<orbistoun_elf::reloc::Elf64Rela> {
        let Ok(Some(offset)) = ours.table_offset(outer, our_info, at) else {
            return Vec::new();
        };
        let end = offset.saturating_add(usize::try_from(size).unwrap_or(0));
        orbistoun_elf::reloc::parse_table(outer.get(offset..end).unwrap_or_default())
    };
    let theirs = selfish_elf::dynamic::relocations(segment, their_info);
    let census = |kinds: Vec<u32>| -> String {
        let mut counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
        for kind in kinds {
            *counts.entry(kind).or_default() += 1;
        }
        format!("{counts:?}")
    };

    for (which, ours_table, theirs_table) in [
        (
            "rela",
            our_table(our_info.rela, our_info.relasz),
            &theirs.data,
        ),
        (
            "jmprel",
            our_table(our_info.jmprel, our_info.pltrelsz),
            &theirs.plt,
        ),
    ] {
        let ours_census = census(
            ours_table
                .iter()
                .map(orbistoun_elf::reloc::Elf64Rela::kind)
                .collect(),
        );
        let theirs_census = census(
            theirs_table
                .iter()
                .map(selfish_elf::reloc::Rela::kind)
                .collect(),
        );
        // Two empty censuses would compare equal vacuously, so this is said rather than
        // counted as agreement.
        if ours_table.is_empty() && theirs_table.is_empty() {
            found.push(Disagreement {
                units: false,
                field: if which == "rela" {
                    "rela: neither reader found any relocations"
                } else {
                    "jmprel: neither reader found any relocations"
                },
                ours: "0".to_owned(),
                theirs: "0".to_owned(),
            });
            continue;
        }
        if ours_census != theirs_census {
            found.push(Disagreement {
                units: false,
                field: if which == "rela" {
                    "rela census by type"
                } else {
                    "jmprel census by type"
                },
                ours: ours_census,
                theirs: theirs_census,
            });
        }
    }
}
/// The dynamic tags a module actually carries, low ones and vendor ones counted apart.
///
/// When the readers disagree about a file's convention, the file settles it: standard tags
/// and no vendor tags is not a vendor module. Printed beside the disagreement so the other
/// project can act on it (D653).
fn tag_census(bytes: &[u8]) -> String {
    let Ok(elf) = selfish_elf::Elf::parse(bytes) else {
        return "unreadable".to_owned();
    };
    let Ok(entries) = elf.dynamic_entries() else {
        return "no dynamic table".to_owned();
    };
    let mut standard: Vec<u64> = entries
        .iter()
        .map(|(tag, _)| *tag)
        .filter(|tag| *tag < 0x6000_0000)
        .collect();
    let mut vendor: Vec<u64> = entries
        .iter()
        .map(|(tag, _)| *tag)
        .filter(|tag| *tag >= 0x6000_0000)
        .collect();
    standard.sort_unstable();
    standard.dedup();
    vendor.sort_unstable();
    vendor.dedup();
    format!(
        "standard tags {standard:?}, vendor tags {}",
        if vendor.is_empty() {
            "none".to_owned()
        } else {
            format!(
                "{:x?}",
                vendor.iter().map(|t| format!("{t:#x}")).collect::<Vec<_>>()
            )
        }
    )
}
/// Every module the local corpus holds, executables and the modules titles ship.
fn corpus() -> Vec<PathBuf> {
    let Some(root) = titles_root() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let Ok(titles) = std::fs::read_dir(&root) else {
        return found;
    };
    for title in titles.flatten() {
        let dir = title.path();
        let eboot = dir.join("eboot.bin");
        if eboot.is_file() {
            found.push(eboot);
        }
        if let Ok(modules) = std::fs::read_dir(dir.join("sce_module")) {
            found.extend(modules.flatten().map(|m| m.path()));
        }
    }
    found
}

/// Where installed titles live, without depending on the paths crate from here.
fn titles_root() -> Option<PathBuf> {
    let data = std::env::var_os("APPDATA")?;
    let root = Path::new(&data).join("OOPS").join("titles");
    root.is_dir().then_some(root)
}

/// Reports rather than asserts: a disagreement is a defect in one of two repositories, and
/// which one is not this test's to decide.
#[test]
fn both_readers_agree_on_every_module_in_the_corpus() {
    let modules = corpus();
    if modules.is_empty() {
        println!(concat!(
            "differential: skipped - no modules under the OOPS titles directory. ",
            "This test needs installed titles, which a clean checkout has none of."
        ));
        return;
    }
    let mut disagreeing = 0_usize;
    let mut units_only = 0_usize;
    for path in &modules {
        let name = path
            .strip_prefix(titles_root().unwrap_or_default())
            .unwrap_or(path)
            .display()
            .to_string();
        let Ok(bytes) = std::fs::read(path) else {
            println!("{name}: unreadable, skipped");
            continue;
        };
        match compare(&bytes) {
            Ok(found) if found.is_empty() => println!("{name}: agrees"),
            Ok(found) => {
                let real = found.iter().filter(|d| !d.units).count();
                if real > 0 {
                    disagreeing += 1;
                }
                units_only += usize::from(real == 0);
                println!(
                    "{name}: {real} field(s) differ{}",
                    if found.len() > real {
                        format!(", {} unit difference(s)", found.len() - real)
                    } else {
                        String::new()
                    }
                );
                let convention = found.iter().any(|d| d.field == "vendor tag convention");
                for d in found {
                    println!(
                        "    {:<26} orbistoun {:<24} selfish {}",
                        d.field, d.ours, d.theirs
                    );
                }
                if convention {
                    println!("    the file itself:           {}", tag_census(&bytes));
                }
            }
            Err(why) => {
                disagreeing += 1;
                println!("{name}: one reader refused the file - {why}");
            }
        }
    }
    println!(
        "differential: {} module(s), {disagreeing} with a disagreement, {units_only} differing only in units",
        modules.len()
    );
}
