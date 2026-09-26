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
pub(crate) fn cmd_imports(service: &Service, path: &std::path::Path) -> Result<()> {
    let survey = service.survey_path(path)?;
    println!("entry {:#x}", survey.entry);
    for i in &survey.imports {
        // Data imports are marked because a thunk is the wrong kind of answer for them: the guest
        // dereferences instruction bytes (D307).
        let data = if i.kind == orbistoun_proto::ImportKind::Object {
            "  [data]"
        } else {
            ""
        };
        println!(
            "{:#018x}  {}  {}{}",
            i.nid,
            i.library.as_deref().unwrap_or("?"),
            i.symbol.as_deref().unwrap_or("<unknown>"),
            data
        );
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
