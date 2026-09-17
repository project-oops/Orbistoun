//! Two readers over one corpus, and every field where they disagree.
//!
//! # Why a second reader is worth a dev-dependency
//!
//! `orbistoun-elf` and `selfish-elf` are the same knowledge written twice - SELFish's reader was
//! built from this one and both have moved since. A parser has no oracle: hostile bytes produce a
//! plausible answer whatever it does, and the only cheap instrument for its own bugs is another
//! parser that was written from the same facts and has drifted. obSCEne's migration to these
//! crates found four defects that way, two of them in SELFish and two in obSCEne, and none of them
//! visible to either project alone (SELFish REQ-20260909T1452Z-b91d, D653).
//!
//! **A differential, not a migration.** Nothing here replaces orbistoun's reader; the test reports
//! disagreements and asserts nothing about which side is right, because deciding that needs a
//! person and a third source.
//!
//! # It skips when the corpus is absent, and says so
//!
//! The modules are installed titles, not repository fixtures, so a clean checkout has none. A
//! skip prints what it looked for - a silent pass would be indistinguishable from agreement.

use std::path::{Path, PathBuf};

/// One field, as each reader read it.
struct Disagreement {
    field: &'static str,
    ours: String,
    theirs: String,
    /// Whether this is a difference in *units* rather than in what the two readers found.
    ///
    /// **Counted apart, because the headline is what anybody reads.** Every module with encoded
    /// imports differs on NID byte order, and reporting that as 28 disagreements says the two
    /// readers disagree about 28 modules when they agree about all of them. The line still
    /// prints - a convention nobody wrote down is how three of D654's four errors happened.
    units: bool,
}

/// Compares one module and returns every field the two readers answer differently.
/// The tables, once both readers have found one.
///
/// Split from [`compare`] so each half stays under the line limit and, more usefully, so the two
/// questions stay apart: whether a file *has* a vendor dynamic table, and what is *in* it. The
/// first is where the two readers actually differ.
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
    // **Legacy, not `is_some`, and that was the third harness error.** The two fields have
    // different meanings: orbistoun's says the values it holds came from *vendor* tags, SELFish's
    // says which convention the file follows - and `Current` means the standard tables come from
    // standard tags, which is exactly when orbistoun answers `false`. Compared as `is_some` every
    // module in the corpus disagreed, and none of them actually did (D654).
    note(
        "table values came from vendor tags",
        our_info.vendor_tables.to_string(),
        matches!(their_info.table, Some(selfish_elf::dynamic::Table::Orbis)).to_string(),
    );
    // Rebased into orbistoun's units before comparing, per the same request: SELFish measures
    // from the slice it returns, orbistoun from the image.
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
    // The packed entries themselves, not only how many: two tables of equal length holding
    // different ids answer every count identically and every lookup differently.
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
/// **The units, which are the third thing this harness got wrong.** SELFish returns table
/// positions as offsets into the byte slice it hands back beside them; orbistoun returns virtual
/// addresses. Comparing them raw reported six disagreements on every module and one suspicious
/// constant - the constant being this number (SELFish REQ-20260909T1730Z-5f28, D654).
///
/// Found by matching the slice's file offset against the program headers, so it is read from the
/// file rather than assumed from the one module where the constant was noticed.
fn slice_base(elf: &selfish_elf::Elf<'_>, whole: &[u8], slice: &[u8]) -> u64 {
    let at = (slice.as_ptr() as usize).saturating_sub(whole.as_ptr() as usize) as u64;
    elf.program_headers()
        .iter()
        .find(|p| p.offset.get() == at)
        .map_or(0, |p| p.vaddr.get())
}
/// The bytes to hand SELFish's reader, for a module inside a signed container.
///
/// **Spliced, not extracted, and that was the correction.** The first version of this test pulled
/// the inner ELF out of the container with orbistoun's own wrapper and handed SELFish the view.
/// The view has program headers describing segments whose payloads live in the container's entry
/// list, so 22 of 29 modules came back refused. `selfish_container` splices those payloads back
/// in and returns an ELF that reads all the way through - so the answer was not to unwrap more
/// carefully but to not unwrap at all (SELFish REQ-20260909T1730Z-5f28, D654).
fn readable_by_selfish(bytes: &[u8]) -> Option<Vec<u8>> {
    selfish_container::Container::parse(bytes)
        .ok()?
        .to_elf()
        .ok()
}
fn compare(outer: &[u8]) -> Result<Vec<Disagreement>, String> {
    // **Each reader is handed the bytes its own entry point expects.** Orbistoun's `Container`
    // unwraps the signed container itself; SELFish's `Elf` wants an ELF whose segment payloads are
    // present, which `selfish_container` produces by splicing. Handing both the same *file* is
    // what makes this a differential; handing both the same *slice* is what made the first two
    // runs measure the harness (D654).
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

    // **The no-vendor-tables case first, because it is the one already known to differ.** A plain
    // freestanding ELF has no vendor dynamic table at all, and a reader that finds one there is
    // reading a standard tag as a vendor tag - the exact defect obSCEne's migration found.
    let (our_info, their_info, base, segment) = match (our_dyn, their_tables) {
        (Some(dyn_bytes), Some((base, segment, their_info))) => (
            orbistoun_elf::dynamic::DynamicInfo::parse(dyn_bytes),
            their_info,
            base,
            segment,
        ),
        (ours_had, theirs_had) => {
            // **Like for like, and this was the fourth harness error.** `dynamic_bytes` answers
            // "is there a `PT_DYNAMIC`", which a plain freestanding ELF has; `tables()` answers
            // "are there resolvable vendor tables", which it has not. Compared directly, the one
            // module in the corpus that is a plain ELF looked like orbistoun over-reporting.
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
/// # Three meanings checked before anything was compared
///
/// D654 cost four rounds to the same mistake - two fields sharing a name and not a meaning - so
/// each of these was established from the two crates' own source before a line of comparison was
/// written, and each is stated here because the next reader cannot see that work:
///
/// - **The NID is byte-reversed between the two.** Orbistoun prints the big-endian read of the
///   hash's first eight bytes, SELFish the little-endian one. `0x53bbd82b51d172db` here is
///   `0xdb72d1512bd8bb53` there. Compared raw, every import in the corpus would differ.
/// - **SELFish's `imports` returns encoded names only.** A plain undefined symbol - what an open
///   toolchain emits - is skipped there and carried here as `NameForm::Plain`. Counting both
///   lists would report a difference on every module that has one, so the plain ones are counted
///   apart and reported rather than compared.
/// - **The library and module come out differently.** Orbistoun carries the raw ids and resolves
///   them through its own tables; SELFish resolves them for you. The *names* are the comparable
///   thing, and they are also the thing worth comparing: a wrong id attributes an import to the
///   wrong module and produces a name that fits and means nothing, which is D117.
fn compare_symbols(
    ours: &orbistoun_elf::Container<'_>,
    outer: &[u8],
    segment: &[u8],
    their_info: &selfish_elf::dynamic::Info,
    found: &mut Vec<Disagreement>,
) -> Result<(usize, usize), String> {
    // The shipped suffix, so a plain name hashes to what the rest of this project would compute.
    // Only the encoded imports are compared below, where the hash comes from the name rather than
    // from this - but a wrong suffix here would silently change the plain count.
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

    // **Two derivations of one number, which is why it is worth comparing at all.** Orbistoun
    // takes the symbol count from the hash table's `nchain`; SELFish divides `symtabsz` by
    // `syment`. They are independent readings of the same file and a disagreement means one of
    // them is wrong.
    match (ours.symbol_count(outer), their_info.symbol_count()) {
        (Ok(our_count), Some(their_count)) => note(
            "dynamic symbol count",
            our_count.to_string(),
            their_count.to_string(),
        ),
        // **Said rather than skipped.** A comparison that never ran is indistinguishable from one
        // that agreed, which is the failure this whole test exists to avoid one level down.
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

    // Joined on the symbol index, because that is what a relocation names and the only thing that
    // makes the two lists the same list.
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
    // Reported as a field only when the two orders are mixed within one module, which would mean
    // neither reader has a convention and something is wrong in one of them. A module that is
    // wholly one or wholly the other is a units fact, stated once by the caller.
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
/// Split from [`compare_symbols`] to keep each under the line limit, and because the join is the
/// part with the three unit differences in it - it deserves to be read on its own.
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
        // **The byte order is measured, not assumed.** Orbistoun's hasher documents a
        // little-endian read and SELFish's answer to REQ-20260909T1250Z-1f74 said orbistoun prints
        // these reversed. Rather than pick one and be wrong in the D654 way, both are tried and
        // which agreed is counted.
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
/// **Counted by type rather than compared entry by entry**, because a difference in one entry and
/// a difference in a thousand mean the same thing to whoever has to look: one of the two readers
/// is walking the table wrongly. The type is also the field that matters most - a relocation
/// resolved as the wrong kind writes an address into a slot the guest reads as data, and neither
/// reader would report that as a failure.
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
        // **An empty census agrees with an empty census.** Two tables nobody located compare
        // equal and report nothing, which is the same vacuous pass as a comparison that never
        // ran - so say it rather than count it as agreement.
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
/// **Evidence, not another opinion.** When the two readers disagree about which convention a file
/// uses, the file itself settles it: a module with standard tags and no vendor tags is not a
/// vendor module however a detector reads it. Printing this beside the disagreement turns "these
/// differ" into something the other project can act on without re-deriving it (D653).
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

/// **The differential.** Reports rather than asserts: a disagreement is a defect in one of two
/// repositories and which one is not this test's to decide.
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
