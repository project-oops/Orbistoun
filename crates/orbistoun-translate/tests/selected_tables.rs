//! Guest programs read from files, traced and translated as a dispatch's are, to see which of their
//! buffer accesses select a descriptor from a table at run time (D966), how many buffers they bind,
//! and whether they translate with those tables read through the global buffer of pages.
//!
//! `ORBISTOUN_TABLE_PROGRAMS` lists `path|user words|group id components|thread id
//! components|scratch bytes a wave` entries separated by `;`, each file the program's bytes from its
//! first instruction. Nothing runs without it: guest bytes are not part of the tree.
//!
//! Each program is translated through the crate's own entry, as the pipeline translates a dispatch,
//! so a form the entry refuses before translating - a modifier word, an unlaid instruction - is
//! refused here too, and the model chosen is the one a run on a host subgroup of
//! `ORBISTOUN_HOST_SUBGROUP` lanes chooses (D979).

use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::Width;
use orbistoun_translate::draw_buffers::{BufferSource, DrawBuffers};

/// Says which instructions the program holds that the translator does not name, each with how many
/// times it occurs: what stands between it and a translation, all at once rather than a refusal at
/// a time.
fn census(decoded: &orbistoun_shader::Decode, encodings: &EncodingTable) {
    let mut missing: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for instruction in &decoded.instructions {
        let name = instruction
            .encoding
            .and_then(|index| encodings.encodings().get(usize::from(index)))
            .and_then(|family| {
                encodings
                    .mnemonic_for(&family.name, instruction.opcode)
                    .map(str::to_owned)
                    .or_else(|| Some(format!("{} opcode {:#x}", family.name, instruction.opcode)))
            })
            .unwrap_or_else(|| "an unnamed word".to_owned());
        if !orbistoun_translate::model::SUPPORTED.contains(&name.as_str()) {
            *missing.entry(name).or_default() += 1;
        }
    }
    let listed: Vec<String> = missing
        .iter()
        .map(|(name, count)| format!("{name} x{count}"))
        .collect();
    println!("  not named by the translator: {}", listed.join(", "));
}

/// Each slot's descriptor's fourth word as a format access through it would find it: 32-bit floats,
/// as many as the most channels any format access through the slot names, with the identity
/// selects and `OOB_SELECT` 3. A format store's channels must be its format's, and the formats a
/// dispatch's descriptors carry are not in the program's bytes.
fn float_formats(
    decoded: &orbistoun_shader::Decode,
    encodings: &EncodingTable,
    traced: &DrawBuffers,
) -> BufferFormats {
    // `BUF_FMT_32_FLOAT`, `_32_32_`, `_32_32_32_` and `_32_32_32_32_FLOAT`.
    const CODES: [u32; 4] = [22, 64, 74, 77];
    let mut channels = [1_usize; 64];
    for instruction in &decoded.instructions {
        let Some(name) = instruction
            .encoding
            .and_then(|index| encodings.encodings().get(usize::from(index)))
            .and_then(|family| encodings.mnemonic_for(&family.name, instruction.opcode))
        else {
            continue;
        };
        let (Some((_, letters)), Some(&slot)) = (
            name.rsplit_once("_format_"),
            traced.served.get(&instruction.offset),
        ) else {
            continue;
        };
        if let Some(most) = channels.get_mut(slot as usize) {
            *most = (*most).max(letters.len().clamp(1, 4));
        }
    }
    let mut formats = BufferFormats::default();
    for (slot, &count) in channels.iter().enumerate() {
        formats.set(slot, Some(0x3100_0fac | CODES[count - 1] << 12));
    }
    formats
}

/// Says what the program at `path` traced to: how many buffers, how many computed, each table and
/// the accesses reading through its records, and each computed buffer that is not a table's.
fn report(path: &str, traced: &DrawBuffers) {
    let computed = traced
        .sources
        .iter()
        .filter(|source| matches!(source, BufferSource::Computed(_)))
        .count();
    println!(
        "{path}: {} buffers, {computed} computed, {} tables",
        traced.sources.len(),
        traced.tables.len()
    );
    for (slot, table) in &traced.tables {
        let at: Vec<String> = traced
            .served
            .iter()
            .filter(|&(_, served)| served == slot)
            .map(|(at, _)| format!("{at:#x}"))
            .collect();
        let several = if traced.several.contains(slot) {
            ", several"
        } else {
            ""
        };
        println!(
            "  slot {slot}: the table {:?} at +{:#x}{several}, read at {}",
            table.words,
            table.element,
            at.join(" ")
        );
    }
    for (slot, source) in traced.sources.iter().enumerate() {
        if let BufferSource::Computed(computed) = source
            && !traced
                .tables
                .contains_key(&u32::try_from(slot).expect("a slot"))
        {
            println!(
                "  slot {slot}: computed at {:#x}, {:?}, offset code {}",
                computed.at, computed.through, computed.soffset
            );
        }
    }
}
use orbistoun_translate::wavefront::{
    BufferFormats, ComputeInputs, MeshPrimitive, Stage, UserData, Window, draw_buffers_for,
};

/// The host subgroup a run offers a dispatch (`ORBISTOUN_HOST_SUBGROUP`, `none` for none): 32
/// where unset, the width of the device the runs this oracle answers for have used.
fn host_subgroup() -> Option<u32> {
    match std::env::var("ORBISTOUN_HOST_SUBGROUP").as_deref() {
        Ok("none") => None,
        Ok(width) => Some(width.parse().expect("a subgroup width")),
        Err(_) => Some(32),
    }
}

/// `decoded` translated as the pipeline translates a dispatch: through the crate's entry, which
/// refuses a modifier word or an unlaid instruction before anything is translated, left to choose
/// its model with the host's subgroup offered (D880, D884, D973). The module's length in words, and
/// the model it chose.
fn translated_as_dispatched(
    decoded: &orbistoun_shader::Decode,
    encodings: &EncodingTable,
    width: Width,
    user_data: &UserData,
) -> Result<(usize, orbistoun_translate::Fidelity), orbistoun_translate::TranslateError> {
    orbistoun_translate::translate_with_user_data(
        decoded,
        encodings,
        orbistoun_translate::Strategy::Predicated {
            fidelity: orbistoun_translate::Fidelity::Auto,
            width,
        },
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(0x7400_0000_0000, 64).expect("a window"),
        UserData {
            per_invocation: host_subgroup(),
            ..*user_data
        },
    )
    .map(|translated| {
        // Where each texture slot's descriptor comes from: a table selected at run time (D983),
        // found by running the program (D960), or a fixed place.
        for source in &translated.textures {
            match source.computed {
                Some(computed) => match computed.selected {
                    Some(selected) => println!(
                        "  texture {}: selected from the table {:?} at +{:#x}, first at {:#x}, sampler {:?}",
                        source.slot,
                        selected.table.words,
                        selected.table.element,
                        computed.at,
                        selected.sampler
                    ),
                    None => println!(
                        "  texture {}: computed at {:#x} in s{}",
                        source.slot, computed.at, computed.register
                    ),
                },
                None => println!(
                    "  texture {}: table offset {:?}, user data {:?}",
                    source.slot, source.table_offset, source.user_data
                ),
            }
        }
        (translated.module.len(), translated.fidelity)
    })
}

/// An entry's fields, `path|user words|ids|components|scratch`, and its wave width: an optional
/// sixth field, `32` or `64`, as the dispatch's own initiator sets it; sixty-four where absent.
fn entry_fields(entry: &str) -> ([&str; 5], Width) {
    let fields: Vec<&str> = entry.split('|').collect();
    let width = match fields.get(5).copied() {
        None | Some("64") => Width::Wave64,
        Some("32") => Width::Wave32,
        Some(other) => panic!("a wave is 32 or 64 lanes, not {other}: {entry}"),
    };
    let named: Option<[&str; 5]> = fields.get(..5).and_then(|five| five.try_into().ok());
    match named {
        Some(named) if fields.len() <= 6 => (named, width),
        _ => panic!("an entry is path|user words|ids|components|scratch[|32 or 64]: {entry}"),
    }
}

#[test]
fn listed_programs_select_from_tables() {
    let Ok(listed) = std::env::var("ORBISTOUN_TABLE_PROGRAMS") else {
        return;
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let mut refused = Vec::new();
    for entry in listed.split(';').filter(|entry| !entry.is_empty()) {
        let ([path, count, ids, components, scratch], width) = entry_fields(entry);
        let number = |text: &str| text.parse::<u32>().expect("a number");
        let bytes = std::fs::read(path).expect("the program's bytes");
        let decoded = decode_program(&bytes, &encodings, &operands);
        let ids = number(ids);
        let inputs = ComputeInputs {
            workgroup_ids: [ids > 0, ids > 1, ids > 2],
            thread_id_components: number(components),
            threads: [64, 1, 1],
            scratch_wave_bytes: number(scratch),
            ..ComputeInputs::default()
        };
        let user_data = UserData {
            count: number(count),
            compute: Some(inputs),
            draw_buffers: true,
            ..UserData::default()
        };
        let traced = match draw_buffers_for(&decoded, &encodings, Stage::Compute, user_data) {
            Ok(traced) => traced,
            Err(why) => {
                println!("{path}: not traced: {why}");
                refused.push(path);
                continue;
            }
        };
        report(path, &traced);
        census(&decoded, &encodings);
        // Translated with every table read through the pages, every slot's format four 32-bit
        // floats with the identity selects (`BUF_FMT_32_32_32_32_FLOAT`, `OOB_SELECT` 3), which
        // every format access's channels name: the formats a dispatch's descriptors carry are not
        // in the program's bytes.
        let mask = traced
            .tables
            .keys()
            .filter(|&&slot| slot < 64)
            .fold(0_u64, |mask, &slot| mask | 1 << slot);
        let formats = float_formats(&decoded, &encodings, &traced);
        let translated = translated_as_dispatched(
            &decoded,
            &encodings,
            width,
            &UserData {
                compute: Some(ComputeInputs {
                    table_buffers: mask,
                    ..inputs
                }),
                local_words: Some(16384),
                buffer_formats: Some(formats),
                dx10_clamp: Some(true),
                ..user_data
            },
        );
        match translated {
            Ok((words, fidelity)) => println!("  translates: {words} words, {fidelity}"),
            Err(why) => {
                println!("  refused: {why}");
                refused.push(path);
            }
        }
    }
    assert!(refused.is_empty(), "refused: {refused:?}");
}

/// For each bound slot a format access goes through, the most channels any of them moves.
fn format_widths(
    decoded: &orbistoun_shader::Decode,
    encodings: &EncodingTable,
    traced: &DrawBuffers,
) -> std::collections::BTreeMap<u32, u32> {
    let mut widths = std::collections::BTreeMap::new();
    for instruction in &decoded.instructions {
        let Some(name) = instruction
            .encoding
            .and_then(|index| encodings.encodings().get(usize::from(index)))
            .and_then(|family| encodings.mnemonic_for(&family.name, instruction.opcode))
        else {
            continue;
        };
        let (Some((_, channels)), Some(&slot)) = (
            name.rsplit_once("_format_"),
            traced.served.get(&instruction.offset),
        ) else {
            continue;
        };
        let channels = u32::try_from(channels.len()).unwrap_or(4);
        let widest = widths.entry(slot).or_insert(channels);
        *widest = (*widest).max(channels);
    }
    widths
}

/// Guest programs read from files translated device-addressed (D964), as the command processor
/// prepares a dispatch whose ranges the host cannot know: nothing of the program is run on the
/// host, and every instruction the translator does not name is listed at once.
/// `ORBISTOUN_DEVICE_PROGRAMS` lists entries as `ORBISTOUN_TABLE_PROGRAMS` does.
#[test]
fn listed_programs_translate_device_addressed_alone() {
    let Ok(listed) = std::env::var("ORBISTOUN_DEVICE_PROGRAMS") else {
        return;
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let mut refused = Vec::new();
    for entry in listed.split(';').filter(|entry| !entry.is_empty()) {
        let ([path, count, ids, components, scratch], width) = entry_fields(entry);
        let number = |text: &str| text.parse::<u32>().expect("a number");
        let bytes = std::fs::read(path).expect("the program's bytes");
        let decoded = decode_program(&bytes, &encodings, &operands);
        println!(
            "{path}: {} instructions, trustworthy {}",
            decoded.instructions.len(),
            decoded.is_trustworthy()
        );
        census(&decoded, &encodings);
        let ids = number(ids);
        let formats = BufferFormats::default();
        let user_data = UserData {
            count: number(count),
            compute: Some(ComputeInputs {
                workgroup_ids: [ids > 0, ids > 1, ids > 2],
                thread_id_components: number(components),
                threads: [64, 1, 1],
                scratch_wave_bytes: number(scratch),
                device_addressed: true,
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            local_words: Some(16384),
            buffer_formats: Some(formats),
            dx10_clamp: Some(true),
            // Where the program was loaded, from a file named `mem-<address>.bin`, as the host
            // tells a program that reads its own address (D842, D914).
            program_address: std::path::Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.strip_prefix("mem-"))
                .and_then(|hex| u64::from_str_radix(hex, 16).ok()),
            ..UserData::default()
        };
        // Each bound slot's format as many 32-bit floats as the widest format access through it
        // moves, `X Y Z W` in order: the host reads the real one from its descriptor. A descriptor
        // the program computes converts by its own as the module runs (D967).
        let mut formats = BufferFormats::default();
        match draw_buffers_for(&decoded, &encodings, Stage::Compute, user_data) {
            Ok(traced) => {
                report(path, &traced);
                for (slot, channels) in format_widths(&decoded, &encodings, &traced) {
                    // `32_FLOAT`, `32_32_FLOAT`, `32_32_32_FLOAT`, `32_32_32_32_FLOAT`.
                    let code = [22, 64, 74, 77][channels.clamp(1, 4) as usize - 1];
                    formats.set(slot as usize, Some(0x3100_0fac | code << 12));
                }
            }
            Err(why) => println!("  not traced: {why}"),
        }
        let translated = translated_as_dispatched(
            &decoded,
            &encodings,
            width,
            &UserData {
                buffer_formats: Some(formats),
                ..user_data
            },
        );
        match translated {
            Ok((words, fidelity)) => println!("  translates: {words} words, {fidelity}"),
            Err(why) => {
                println!("  refused: {why}");
                refused.push(path);
            }
        }
    }
    assert!(refused.is_empty(), "refused: {refused:?}");
}
