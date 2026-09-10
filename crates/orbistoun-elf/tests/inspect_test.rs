use std::path::Path;
use orbistoun_elf::{Container, reloc::{parse_table, kind}};
use orbistoun_elf::dynamic::DynamicInfo;

fn inspect_file(path: &Path) {
    println!("\n=== Inspecting {:?} ===", path);
    if !path.is_file() {
        println!("File not found");
        return;
    }
    let whole = std::fs::read(path).unwrap();
    let container = Container::parse(&whole).unwrap();
    let dyn_bytes = match container.dynamic_bytes(&whole).unwrap() {
        Some(d) => d,
        None => { println!("No dynamic section"); return; }
    };
    let info = DynamicInfo::parse(dyn_bytes);
    let hasher = orbistoun_nid::NidHasher::new(orbistoun_nid::default_suffix());
    let absent_target_nid = hasher.hash("obs_census_control_absent").as_raw();

    let sym_count = container.symbol_count(&whole).unwrap();
    println!("Total symbols: {}", sym_count);

    let sym_offset = container.table_offset(&whole, &info, info.symtab).unwrap().unwrap();
    let str_offset = container.table_offset(&whole, &info, info.strtab).unwrap().unwrap();
    let stride = if info.syment == 0 { 24 } else { info.syment as usize };

    let mut bind_counts = std::collections::BTreeMap::new();
    let mut absent_sym_idx = None;
    for idx in 0..sym_count as usize {
        let at = sym_offset + idx * stride;
        let entry = &whole[at..at + 24];
        let name_off = u32::from_le_bytes(entry[..4].try_into().unwrap()) as usize;
        let st_info = entry[4];
        let st_shndx = u16::from_le_bytes(entry[6..8].try_into().unwrap());
        let st_value = u64::from_le_bytes(entry[8..16].try_into().unwrap());

        let str_bytes = &whole[str_offset + name_off..];
        let name_end = str_bytes.iter().position(|&b| b == 0).unwrap_or(0);
        let name = std::str::from_utf8(&str_bytes[..name_end]).unwrap_or("");

        let bind = st_info >> 4;
        let kind_code = st_info & 0xf;
        *bind_counts.entry(bind).or_insert(0) += 1;

        if bind == 2 || name.contains("absent") || name.contains("census") {
            println!("  [WEAK/CENSUS] Sym {}: name='{}', bind={}, type={}, shndx={}, val={:#x}",
                idx, name, bind, kind_code, st_shndx, st_value);
        }
        if let Some(dec) = orbistoun_nid::decode_symbol_name(name) {
            if dec.nid.as_raw() == absent_target_nid {
                println!("  [MATCHED NID] Sym {}: name='{}', bind={}, type={}, shndx={}, val={:#x}",
                    idx, name, bind, kind_code, st_shndx, st_value);
                absent_sym_idx = Some(idx as u32);
            }
        }
    }
    println!("Bind counts: {:?}", bind_counts);

    for (table_name, addr, size) in [("RELA", info.rela, info.relasz), ("JMPREL", info.jmprel, info.pltrelsz)] {
        if addr == 0 || size == 0 { continue; }
        let at = container.table_offset(&whole, &info, addr).unwrap().unwrap();
        let len = size as usize;
        let table_bytes = &whole[at..at + len];
        let table = parse_table(table_bytes);
        for (i, r) in table.iter().enumerate() {
            let sym_idx = r.symbol_index();
            if Some(sym_idx) == absent_sym_idx {
                println!("  Reloc {} in {}: offset={:#x}, kind={}, sym_idx={}, addend={}",
                    i, table_name, r.offset.get(), r.kind(), sym_idx, r.addend.get());
            }
        }
    }
}

#[test]
fn find_three_unresolved_relocations() {
    // Derive the corpus root from %APPDATA% rather than hard-coding a home directory
    // (matches differential.rs); resolves to <APPDATA>/OOPS/titles/PPSA99980/eboot.bin.
    let appdata = std::env::var_os("APPDATA").expect("APPDATA is set on this platform");
    let path = Path::new(&appdata).join("OOPS/titles/PPSA99980/eboot.bin");
    let whole = std::fs::read(&path).unwrap();
    let container = Container::parse(&whole).unwrap();
    let dyn_bytes = container.dynamic_bytes(&whole).unwrap().unwrap();
    let info = DynamicInfo::parse(dyn_bytes);

    let symbols_db_path = Path::new("symbols/generated.json");
    let text = std::fs::read_to_string(symbols_db_path).unwrap();
    let db = orbistoun_nid::SymbolDbFile::from_json(&text).unwrap();

    let service = orbistoun_service::Service::new(orbistoun_service::ServiceConfig::default());
    let labels = service.import_labels_with(&whole, &db).unwrap();

    let refused: std::collections::BTreeSet<usize> = labels
        .iter()
        .enumerate()
        .filter(|(_, label)| {
            label
                .rsplit("::")
                .next()
                .is_some_and(|name| name.starts_with("0x"))
        })
        .map(|(index, _)| index)
        .collect();

    println!("Refused count: {}", refused.len());
    for &idx in &refused {
        println!("  Refused idx {}: label='{}'", idx, labels[idx]);
    }

    // Now let's link the title like worker does
    let database = db.to_database();
    let linked = orbistoun_service::link_the_title(&service, path, &database).unwrap();

    let stubs = orbistoun_loader::relocate::ImportResolver {
        thunks: &linked.thunks,
        data: &linked.data,
        refuse: Some(&refused),
    };
    let resolver = orbistoun_loader::relocate::TitleResolver {
        bound: &linked.bound,
        inner: &stubs,
    };

    use orbistoun_loader::relocate::SymbolResolver;

    // Check all relocations in RELA and JMPREL
    let mut unresolved_list = Vec::new();
    for (table_name, addr, size) in [("RELA", info.rela, info.relasz), ("JMPREL", info.jmprel, info.pltrelsz)] {
        if addr == 0 || size == 0 { continue; }
        let at = container.table_offset(&whole, &info, addr).unwrap().unwrap();
        let len = size as usize;
        let table_bytes = &whole[at..at + len];
        let table = parse_table(table_bytes);
        for (i, r) in table.iter().enumerate() {
            let res = orbistoun_loader::relocate::value_for(r, 0x400000000000, &resolver, None);
            if let Err(e) = res {
                let sym_idx = r.symbol_index() as usize;
                let sym_label = labels.get(sym_idx).map(|s| s.as_str()).unwrap_or("<no label>");
                unresolved_list.push((table_name, i, r.offset.get(), r.kind(), sym_idx, sym_label, e));
            }
        }
    }

    println!("Total unapplied: {}", unresolved_list.len());
    for item in &unresolved_list {
        println!("  Unapplied in {}: idx={}, offset={:#x}, kind={}, sym_idx={}, label={}, err={:?}",
            item.0, item.1, item.2, item.3, item.4, item.5, item.6);
    }
}

