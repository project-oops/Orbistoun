//! Inspecting a guest module: symbols, imports, exports, load and verify.

use crate::common::percent;
use anyhow::{Context, Result};
use orbistoun_service::Service;

/// `symbols` - everything orbistoun declares.
pub(crate) fn cmd_symbols(service: &Service, filter: Option<&str>) {
    let mut shown = 0_usize;
    let mut implemented = 0_usize;
    for d in service.declared_symbols() {
        let matches = filter.is_none_or(|f| d.library.contains(f) || d.symbol.contains(f));
        if matches {
            // A leading marker lines up down the left edge, so the implemented share is visible at
            // a glance.
            println!(
                "{} {:#018x}  {:<16}  {:<40}  argc={}",
                if d.implemented { "*" } else { " " },
                d.nid,
                d.library,
                d.symbol,
                d.arity
            );
            shown += 1;
            implemented += usize::from(d.implemented);
        }
    }
    eprintln!(
        "{shown} declared, {implemented} implemented (*), {} on stubs",
        shown - implemented
    );
}

/// `inspect` - a container's structure, without executing or fully parsing it.
/// A run's import calls by label, from its `trace.json` or the folder holding it.
fn run_calls(path: &std::path::Path) -> Result<std::collections::BTreeMap<String, u64>> {
    let file = if path.is_dir() {
        path.join("trace.json")
    } else {
        path.to_owned()
    };
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let trace: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
    let calls = trace
        .get("calls")
        .and_then(serde_json::Value::as_array)
        .with_context(|| format!("{} holds no calls", file.display()))?;
    Ok(calls
        .iter()
        .filter_map(|call| {
            Some((
                call.get("label")?.as_str()?.to_owned(),
                call.get("calls")?.as_u64()?,
            ))
        })
        .collect())
}

/// `calls-diff` - how two runs' import calls differ.
pub(crate) fn cmd_calls_diff(first: &std::path::Path, second: &std::path::Path) -> Result<()> {
    let difference =
        orbistoun_service::callsdiff::calls_difference(&run_calls(first)?, &run_calls(second)?);
    if difference.is_empty() {
        println!("the two runs reached the same imports, none of whose shares moved");
        return Ok(());
    }
    for (label, calls) in &difference.only_first {
        println!("only the first   {calls:>10}  {label}");
    }
    for (label, calls) in &difference.only_second {
        println!("only the second  {calls:>10}  {label}");
    }
    for (label, before, after) in &difference.moved {
        println!("moved  {before:>10} -> {after:<10}  {label}");
    }
    Ok(())
}

/// An image offset as a report spells it, `image+0x2b42a40`, or bare, `0x2b42a40`.
fn image_offset(spelled: &str) -> Option<u64> {
    let digits = spelled.trim().trim_start_matches("image+");
    u64::from_str_radix(digits.trim_start_matches("0x"), 16).ok()
}

/// `disasm` - a module's instructions from an image offset, one a line.
pub(crate) fn cmd_disasm(
    service: &Service,
    path: &std::path::Path,
    from: &str,
    (len, words): (usize, bool),
) -> Result<()> {
    let offset = image_offset(from)
        .with_context(|| format!("{from} is not an image offset such as image+0x2b42a40"))?;
    if words {
        let listed = service.data_words_path(path, offset, len)?;
        if listed.is_empty() {
            println!(
                "image+{offset:#x} lies in no loaded segment, or past the bytes the file holds"
            );
        }
        for (at, word) in listed {
            println!("image+{at:#x}  {word:#018x}");
        }
        return Ok(());
    }
    let listed = service.disassemble_path(path, offset, len)?;
    if listed.is_empty() {
        println!("image+{offset:#x} lies in no executable segment");
    }
    for (at, text) in listed {
        println!("image+{at:#x}  {text}");
    }
    Ok(())
}

/// The linkage slot whose import `labels` names `name`, bare or as `library::name`, or by `hash`,
/// the name's import hash, where orbistoun declares no such name and the label is the hash.
fn named_slot(
    labels: &std::collections::BTreeMap<u64, String>,
    name: &str,
    hash: u64,
) -> Option<u64> {
    let hashed = format!("{hash:#018x}");
    labels.iter().find_map(|(&slot, label)| {
        let bare = label.rsplit("::").next().unwrap_or(label);
        (label == name || bare == name || *label == hashed).then_some(slot)
    })
}

/// The function whose callers a reference leads on to: a data word names none; a linkage stub's
/// jump through its slot is the stub itself, which starts at the jump - stubs sit back to back, so
/// the function holding the jump may be the stub before it; any other instruction leads to the
/// function holding it.
fn walked_from(at: u64, how: &str, within: u64) -> Option<u64> {
    if how.starts_with("jmp qword ptr [") {
        Some(at)
    } else if within != at {
        Some(within)
    } else {
        None
    }
}

/// `callers` - what names an image offset, or the linkage slot an import is called through, walked
/// `depth` levels up through the functions holding each reference: the slot's first level is its
/// linkage stub, the next the title's calls. A function reached at an earlier level is not walked
/// again.
pub(crate) fn cmd_callers(
    service: &Service,
    path: &std::path::Path,
    at: &str,
    depth: usize,
) -> Result<()> {
    let target = if let Some(offset) = image_offset(at) {
        offset
    } else {
        let hash = service.hash_name(at).as_raw();
        let slot = named_slot(&service.slot_labels_path(path)?, at, hash).with_context(|| {
            format!(
                concat!(
                    "{} is neither an image offset such as image+0x2b42a40 nor an import the ",
                    "module calls through a linkage slot"
                ),
                at
            )
        })?;
        println!("{at} is called through the linkage slot at image+{slot:#x}");
        slot
    };
    let mut level = vec![target];
    let mut walked = std::collections::BTreeSet::from([target]);
    for indent in 0..depth.max(1) {
        let mut next = Vec::new();
        for function in level {
            let found = service.references_path(path, function)?;
            let pad = "  ".repeat(indent);
            if found.is_empty() {
                println!("{pad}image+{function:#x}: nothing names it directly");
            }
            for reference in found {
                println!(
                    "{pad}image+{function:#x} <- image+{:#x}  {}  (in image+{:#x})",
                    reference.at, reference.how, reference.within
                );
                if let Some(walk) = walked_from(reference.at, &reference.how, reference.within)
                    && walked.insert(walk)
                {
                    next.push(walk);
                }
            }
        }
        level = next;
    }
    Ok(())
}

pub(crate) fn cmd_inspect(service: &Service, path: &std::path::Path) -> Result<()> {
    let info = service.inspect_path(path)?;
    let wrapper = match info.wrapper {
        orbistoun_service::WrapperInfo::None => "none (bare ELF)".to_owned(),
        orbistoun_service::WrapperInfo::Wrapped {
            previous_generation,
            segment_count,
            stated_size,
        } => {
            let generation = if previous_generation {
                "previous generation"
            } else {
                "current generation"
            };
            format!("{generation}, {segment_count} segments, stated size {stated_size}")
        }
    };
    println!("wrapper {wrapper}");
    println!("elf offset {}", info.elf_offset);
    println!("entry {:#x}", info.entry);
    println!("e_type {:#06x}", info.e_type);
    println!("machine {:#06x}", info.machine);
    println!(
        "osabi {}{}",
        info.osabi,
        if info.osabi == 9 { " (FreeBSD)" } else { "" }
    );
    println!("program headers  {}", info.program_headers);
    println!("vendor segments  {}", info.vendor_segments);
    println!(
        "mapped segments  {:?}  (headers the wrapper locates data for)",
        info.mapped_segments
    );
    match info.proc_param.as_ref() {
        None => println!("proc param       none"),
        Some(p) => {
            println!(
                "proc param       size {:#x}  magic {}  entries {}  sdk {:#010x}",
                p.size,
                if p.magic_ok { "ORBI" } else { "absent" },
                p.entry_count,
                p.sdk_version,
            );
            println!(
                "  pointers       libc {:#x}  mem {:#x}  third {:#x}",
                p.libc_param_vaddr, p.mem_param_vaddr, p.third_param_vaddr
            );
            if p.mem_param_vaddr == 0 {
                println!("  mem param      none");
            } else {
                match p.mem_param_size {
                    Some(size) => println!(
                        "  mem param      vaddr {:#x}  size {:#x}",
                        p.mem_param_vaddr, size
                    ),
                    None => println!(
                        "  mem param      vaddr {:#x}  (maps to no segment)",
                        p.mem_param_vaddr
                    ),
                }
                // Raw, not interpreted: no citable source establishes the field layout inside the
                // block, so a value is shown at its offset and named nothing.
                for (offset, value) in &p.mem_param_nonzero {
                    println!("    +{offset:#04x}       {value:#x}");
                }
                if p.mem_param_size.is_some() && p.mem_param_nonzero.is_empty() {
                    println!("    (all zero past the size field)");
                }
            }
        }
    }
    Ok(())
}

/// `report` - survey a module, persist a run report, and show the delta against the last one.
pub(crate) fn cmd_report(service: &Service, path: &std::path::Path) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let out = service.survey_and_report(path, now)?;

    println!("run {}", out.report.run_id);
    println!("title {}", out.report.inputs.title_hash);
    println!("reached {:?}", out.report.reached);
    println!(
        "imports {} unresolved of {}",
        out.report.counts.distinct_unresolved,
        out.report
            .survey
            .as_ref()
            .map_or(0, orbistoun_service::SurveySummary::total)
    );
    if let Some(first) = out.report.first_unmet.as_ref() {
        println!(
            "first gap {:#018x}  {}",
            first.nid,
            first.symbol.as_deref().unwrap_or("<unknown>")
        );
    }

    match out.diff.as_ref() {
        None => println!("diff none - first run for this title"),
        Some(d) => {
            println!("diff vs {} ({:?})", d.previous, d.phase_change);
            if !d.same_inputs {
                println!("          inputs changed - a difference may be config drift");
            }
            println!(
                "          {} newly resolved, {} newly unresolved",
                d.newly_resolved.len(),
                d.newly_unresolved.len()
            );
        }
    }
    if let Some(at) = out.written_to.as_ref() {
        println!("written {}", at.display());
    }
    Ok(())
}

/// `exports` - what a guest module provides, without executing it.
pub(crate) fn cmd_exports(
    service: &Service,
    path: &std::path::Path,
    matching: Option<&str>,
) -> Result<()> {
    let found = service.exports_path(path)?;
    let mut shown = 0_usize;
    for e in &found {
        let name = e.symbol.as_deref().unwrap_or("<unnamed>");
        if let Some(wanted) = matching {
            if !name.contains(wanted) && !format!("{:#018x}", e.nid).contains(wanted) {
                continue;
            }
        }
        // Data exports are marked: an importer that binds a data export as a function gets a thunk
        // where it expects a value (D307).
        let data = if e.kind == orbistoun_proto::ImportKind::Object {
            "  [data]"
        } else {
            ""
        };
        println!("{:#018x}  +{:#x}  {name}{data}", e.nid, e.offset);
        shown += 1;
    }
    eprintln!("{shown} shown of {} exports", found.len());
    Ok(())
}

/// `imports --own` - the modules a title ships that answer its own imports.
pub(crate) fn cmd_title_modules(
    service: &Service,
    path: &std::path::Path,
    placed: bool,
    linked: bool,
) -> Result<()> {
    let found = service.title_modules_for(path)?;
    for module in &found {
        println!("{:<28}  {}", module.library, module.path.display());
    }
    eprintln!(
        "{} of the title's own modules answer its imports",
        found.len()
    );
    if linked {
        return report_linked(service, path);
    }
    if !placed {
        return Ok(());
    }
    report_placed(service, path, &found)
}

/// The `--linked` half: place the title's own modules and relocate them.
///
/// A placed module's internal pointers are still link-time offsets; a relocated one is code a guest
/// could be sent into.
fn report_linked(service: &Service, path: &std::path::Path) -> Result<()> {
    let bases = orbistoun_service::TitleBases {
        modules: orbistoun_worker::TITLE_MODULE_BASE,
        thunks: orbistoun_worker::THUNK_TABLE_BASE,
        data: orbistoun_worker::DATA_BLOCK_BASE,
    };
    let linked =
        service.link_title_modules(path, bases, &orbistoun_nid::SymbolDbFile::builtin())?;
    for slot in &linked.slots {
        let name = if slot.label.is_empty() {
            "<the executable>"
        } else {
            &slot.label
        };
        println!(
            "slots {:<28} {:>6}..{:<6} ({} symbols)",
            name,
            slot.offset,
            slot.offset + slot.count,
            slot.count
        );
    }
    // Each segment's file-backed and zeroed halves, the `.data`/`.bss` split. An address inside the
    // copied part came from the file; one past it is `.bss` and zero.
    for (library, image) in linked.placed.images() {
        for segment in image.segments() {
            println!(
                "segment {library:<24} #{} {:#014x}  {:#x} copied  {:#x} zeroed  flags {:#x}",
                segment.index, segment.address, segment.copied, segment.zeroed, segment.flags
            );
        }
    }
    for (library, tally) in &linked.tallies {
        println!(
            "relocated {library:<24} {} applied, {} unresolved, {} tls-deferred, {} unsupported",
            tally.applied, tally.unresolved, tally.tls_deferred, tally.unsupported
        );
    }
    for note in &linked.shared_data {
        println!("shared data import: {note}");
    }
    let unresolved: usize = linked.tallies.iter().map(|(_, t)| t.unresolved).sum();
    eprintln!(
        "{} of the title's own modules linked, {unresolved} relocation(s) unresolved",
        linked.tallies.len()
    );
    Ok(())
}

/// The `--placed` half: place them, and say which imports would bind into them.
///
/// Placed, not relocated: this reports and hands nothing to a guest.
fn report_placed(
    service: &Service,
    path: &std::path::Path,
    found: &[orbistoun_service::titlemodules::TitleModule],
) -> Result<()> {
    let modules = service.place_title_modules(found, orbistoun_worker::TITLE_MODULE_BASE)?;
    for (library, image) in modules.images() {
        let (start, len) = image.span();
        println!("placed {library:<22}  {start:#x}  {len} bytes");
    }
    for clash in modules.collisions() {
        println!(
            "collision {:#018x} in {} kept {} dropped {}",
            clash.nid, clash.library, clash.kept, clash.dropped
        );
    }
    let bytes = std::fs::read(path)?;
    let imports = service.raw_imports_of(&bytes)?;
    let (libraries, _modules) = service.module_tables(&bytes)?;
    let resolution = modules.resolve(&imports, &libraries);
    for b in &resolution.bound {
        println!(
            "binds {:<24} {:#018x} {:<44} {:#x}",
            b.library, b.nid, b.name, b.address
        );
    }
    for bad in &resolution.mismatches {
        println!(
            "kind mismatch {} wants {:?}, {} has {:?} - left on its stub",
            bad.name, bad.wanted, bad.library, bad.found
        );
    }
    for bad in &resolution.ambiguous {
        println!(
            "ambiguous {} answered by {} - left unbound",
            bad.name,
            bad.answered_by.join(", ")
        );
    }
    for (library, count) in resolution.by_library() {
        eprintln!("  {library:<24} answers {count}");
    }
    eprintln!(
        "{} of {} imports would bind into the title's own modules ({} exported, {} kind mismatches)",
        resolution.addresses.len(),
        imports.len(),
        modules.export_count(),
        resolution.mismatches.len(),
    );
    Ok(())
}

/// `imports --libraries` - the vendor tables an encoded import name indexes.
pub(crate) fn cmd_module_tables(service: &Service, path: &std::path::Path) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let (libraries, modules) = service.module_tables(&bytes)?;
    println!(
        "libraries ({}), indexed by an import's library id:",
        libraries.len()
    );
    for (id, name) in &libraries {
        println!("  {id:>3}  {name}");
    }
    println!(
        "modules ({}), indexed by an import's module id:",
        modules.len()
    );
    for (id, name) in &modules {
        println!("  {id:>3}  {name}");
    }
    Ok(())
}

/// `imports` - what a guest module needs, without executing it.
/// One import as `imports` prints it: hash, library, name, then its marks.
///
/// Data imports are marked because a thunk is the wrong kind of answer for them: the guest
/// dereferences instruction bytes (D307). Weak imports are marked because one nothing answers binds
/// to zero rather than to a stub (D676).
fn import_line(i: &orbistoun_proto::ImportRecord) -> String {
    let data = if i.kind == orbistoun_proto::ImportKind::Object {
        "  [data]"
    } else {
        ""
    };
    let weak = if i.binding == orbistoun_proto::ImportBinding::Weak {
        "  [weak]"
    } else {
        ""
    };
    format!(
        "{:#018x}  {}  {}{data}{weak}",
        i.nid,
        i.library.as_deref().unwrap_or("?"),
        i.symbol.as_deref().unwrap_or("<unknown>"),
    )
}

pub(crate) fn cmd_imports(service: &Service, path: &std::path::Path) -> Result<()> {
    let survey = service.survey_path(path)?;
    println!("entry {:#x}", survey.entry);
    for i in &survey.imports {
        println!("{}", import_line(i));
    }
    let data = survey
        .imports
        .iter()
        .filter(|i| i.kind == orbistoun_proto::ImportKind::Object)
        .count();
    eprintln!(
        "{} imports, {} unresolved",
        survey.total(),
        survey.unresolved()
    );
    if data > 0 {
        // The loader gives each data import its own zeroed page, consulted before the thunk table
        // (D323).
        eprintln!(
            concat!(
                "{} of them name data, not a function - a thunk is the wrong kind of answer ",
                "there, so each is given a zeroed page of its own instead (D323)"
            ),
            data
        );
    }
    Ok(())
}

/// `load` - reserve the address space a module demands, without executing it.
pub(crate) fn cmd_load(service: &Service, path: &std::path::Path, base: u64) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let layout = service.load_layout(&bytes, base)?;

    println!("base {:#x}", layout.base);
    println!(
        "span {:#x} .. {:#x}  ({} KiB)",
        layout.span_base,
        layout.span_base.saturating_add(layout.span_len),
        layout.span_len / 1024
    );
    for s in &layout.segments {
        let perms = format!(
            "{}{}{}",
            if s.read { 'r' } else { '-' },
            if s.write { 'w' } else { '-' },
            if s.execute { 'x' } else { '-' }
        );
        println!(
            "  [{:2}] {:#014x} {:>10}  {perms}",
            s.index, s.vaddr, s.memsz
        );
    }
    match layout.reservation_failure.as_deref() {
        None => eprintln!(
            "span placed: {} segments fit at {:#x}",
            layout.segments.len(),
            layout.span_base
        ),
        Some(why) => eprintln!("span NOT placed: {why}"),
    }
    Ok(())
}

/// `verify` - how much of a module's import list a symbol database can name.
pub(crate) fn cmd_verify(service: &Service, path: &std::path::Path) -> Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let (explained, total) = service.explain_imports(&bytes)?;
    println!(
        "{explained} of {total} imports named ({:.1}%)",
        percent(explained, total)
    );
    if service.symbol_db_len().is_none() {
        eprintln!("no --symbols-db given, so nothing could be named");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// An import line carries its hash, library and name, then a mark for data and for weak.
    #[test]
    fn an_import_line_marks_weak_and_data_imports() {
        let mut import = orbistoun_proto::ImportRecord {
            nid: 0x1234,
            library: Some("libc".to_owned()),
            symbol: Some("malloc".to_owned()),
            known: true,
            kind: orbistoun_proto::ImportKind::Function,
            binding: orbistoun_proto::ImportBinding::Global,
        };
        assert_eq!(
            super::import_line(&import),
            "0x0000000000001234  libc  malloc"
        );
        import.binding = orbistoun_proto::ImportBinding::Weak;
        import.kind = orbistoun_proto::ImportKind::Object;
        assert_eq!(
            super::import_line(&import),
            "0x0000000000001234  libc  malloc  [data]  [weak]"
        );
    }

    /// A stub's jump through its slot leads to the stub itself, even where the stub before it holds
    /// it; a call leads to the function holding it; a data word to nothing.
    #[test]
    fn a_walk_starts_from_the_stub_not_its_neighbour() {
        let stub = super::walked_from(0x0339_1b70, "jmp qword ptr [3CB07B8h]", 0x0339_1b60);
        assert_eq!(stub, Some(0x0339_1b70));
        let call = super::walked_from(0x02d3_bb95, "call 0000000003391B70h", 0x02d3_b660);
        assert_eq!(call, Some(0x02d3_b660));
        assert_eq!(
            super::walked_from(0x5000, "a data word relocated to it", 0x5000),
            None
        );
    }

    /// An import named bare, with its library, or by the hash of an undeclared name finds the
    /// linkage slot it is called through.
    #[test]
    fn an_import_name_finds_its_linkage_slot() {
        let labels = std::collections::BTreeMap::from([
            (0x10, "libSceNpTrophy2::sceNpTrophy2GetGameInfo".to_owned()),
            (
                0x18,
                "libSceNpTrophy2::sceNpTrophy2GetGameInfoArray".to_owned(),
            ),
            (0x20, "0x00000000deadbeef".to_owned()),
        ]);
        assert_eq!(
            super::named_slot(&labels, "sceNpTrophy2GetGameInfo", 1),
            Some(0x10)
        );
        assert_eq!(
            super::named_slot(&labels, "libSceNpTrophy2::sceNpTrophy2GetGameInfoArray", 1),
            Some(0x18)
        );
        assert_eq!(
            super::named_slot(&labels, "anUndeclaredName", 0xdead_beef),
            Some(0x20)
        );
        assert_eq!(super::named_slot(&labels, "GetGameInfo", 1), None);
    }
}
