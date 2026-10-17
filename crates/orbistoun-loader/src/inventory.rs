//! What a module's code does that linking does not route: raw `syscall` sites (D724), and
//! instructions only AMD processors execute (D725).
//!
//! Read-only. An import reaches orbistoun through its thunk; a `syscall` instruction in the
//! module's own code enters the kernel directly, and an AMD-only instruction faults on another
//! vendor's host, so the plan lists where each one is before the guest runs rather than
//! learning of it only when one executes.

use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter, Mnemonic};
use std::collections::BTreeMap;

use orbistoun_elf::Container;
use orbistoun_elf::dynamic::DynamicInfo;
use orbistoun_elf::reloc::parse_table;
use orbistoun_hle::Registry;

use crate::LoadError;

/// `PT_LOAD`.
const PT_LOAD: u32 = 1;

/// `PF_X`: the segment is executable.
const PF_X: u32 = 1;

/// Instructions an AMD processor executes and another vendor's does not, from AMD's published
/// instruction reference: `EXTRQ` and `INSERTQ` (SSE4a), `MONITORX`, `MWAITX`, `CLZERO`, `RDPRU`
/// and `MCOMMIT`. Each faults as undefined on an Intel host.
const AMD_ONLY: [Mnemonic; 7] = [
    Mnemonic::Extrq,
    Mnemonic::Insertq,
    Mnemonic::Monitorx,
    Mnemonic::Mwaitx,
    Mnemonic::Clzero,
    Mnemonic::Rdpru,
    Mnemonic::Mcommit,
];

/// What one sweep of a module's executable segments found, by guest address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    /// Every raw `syscall` instruction.
    pub syscalls: Vec<u64>,
    /// Every AMD-only instruction, with its mnemonic.
    pub amd_only: Vec<(u64, String)>,
    /// Every `rdtsc` and `rdtscp`: a read of the host's own time-stamp counter, which runs at the
    /// host's rate, not the one `sceKernelGetTscFrequency` tells the guest.
    pub counter_reads: Vec<u64>,
}

/// Every `syscall` and AMD-only instruction in the executable segments of `whole`, placed at
/// `base`.
///
/// A linear sweep: each executable segment is decoded from its start, and an undecodable byte
/// is skipped. Data embedded in code can decode as a false site, never hide a real one that
/// the sweep reaches aligned.
///
/// # Errors
///
/// When the container cannot be parsed.
pub fn inventory(whole: &[u8], base: u64) -> Result<Inventory, LoadError> {
    let container = Container::parse(whole)?;
    let mut found = Inventory::default();
    for (index, header) in container.program_headers()?.iter().enumerate() {
        if header.p_type.get() != PT_LOAD || header.flags.get() & PF_X == 0 {
            continue;
        }
        let Some(code) = container.segment_data(whole, index)? else {
            continue;
        };
        sweep(code, base.wrapping_add(header.vaddr.get()), &mut found);
    }
    found.syscalls.sort_unstable();
    found.amd_only.sort_unstable();
    found.counter_reads.sort_unstable();
    Ok(found)
}

/// Every `syscall` instruction in the executable segments of `whole`, placed at `base`, by
/// guest address. See [`inventory`].
///
/// # Errors
///
/// When the container cannot be parsed.
pub fn syscall_sites(whole: &[u8], base: u64) -> Result<Vec<u64>, LoadError> {
    Ok(inventory(whole, base)?.syscalls)
}

/// Each procedure linkage slot of the module `whole`, by image offset, with the import it holds:
/// `library::name` where `registry` knows it, its hash otherwise. A `DT_JMPREL` entry names the
/// slot and the symbol; the survey names the symbol.
///
/// # Errors
///
/// When the container cannot be parsed.
pub fn slot_labels(whole: &[u8], registry: &Registry) -> Result<BTreeMap<u64, String>, LoadError> {
    let container = Container::parse(whole)?;
    let Some(dyn_bytes) = container.dynamic_bytes(whole)? else {
        return Ok(BTreeMap::new());
    };
    let info = DynamicInfo::parse(dyn_bytes);
    let names: BTreeMap<u32, String> = crate::survey(whole, registry)?
        .imports
        .into_iter()
        .map(|import| {
            let label = match (import.library, import.name) {
                (Some(library), Some(name)) => format!("{library}::{name}"),
                (_, Some(name)) => name,
                (_, None) => format!("{:#018x}", import.nid.as_raw()),
            };
            (import.symbol_index, label)
        })
        .collect();
    let mut labels = BTreeMap::new();
    if info.jmprel == 0 || info.pltrelsz == 0 {
        return Ok(labels);
    }
    let Some(at) = container.table_offset(whole, &info, info.jmprel)? else {
        return Ok(labels);
    };
    let len = usize::try_from(info.pltrelsz).unwrap_or(0);
    for entry in parse_table(whole.get(at..at.saturating_add(len)).unwrap_or_default()) {
        if let Some(label) = names.get(&entry.symbol_index()) {
            labels.insert(entry.offset.get(), label.clone());
        }
    }
    Ok(labels)
}

/// The instructions of the module `whole` from image offset `from`, at most `len` bytes of them,
/// each at its image offset in Intel syntax, a call or jump through a linkage stub annotated with
/// the import `labels` names for its slot: what a report's `image+0x...` names, read with the
/// decoder the plan's sweep uses. Empty when `from` lies in no executable segment.
///
/// # Errors
///
/// When the container cannot be parsed.
pub fn disassemble(
    whole: &[u8],
    from: u64,
    len: usize,
    labels: &BTreeMap<u64, String>,
) -> Result<Vec<(u64, String)>, LoadError> {
    let container = Container::parse(whole)?;
    for (index, header) in container.program_headers()?.iter().enumerate() {
        let (start, size) = (header.vaddr.get(), header.memsz.get());
        if header.p_type.get() != PT_LOAD
            || header.flags.get() & PF_X == 0
            || from < start
            || from >= start.saturating_add(size)
        {
            continue;
        }
        let Some(code) = container.segment_data(whole, index)? else {
            continue;
        };
        return Ok(disassemble_code((code, start), from, len, labels));
    }
    Ok(Vec::new())
}

/// The eight-byte words of the module `whole` from image offset `from`, at most `len` bytes of them,
/// each at its image offset: a vtable or a table a report's `image+0x...` names in data. Empty when
/// `from` lies in no loaded segment, and short where the file holds fewer bytes than the segment
/// spans (the rest is zero-filled at load, and listed as nothing).
///
/// # Errors
///
/// When the container cannot be parsed.
pub fn data_words(whole: &[u8], from: u64, len: usize) -> Result<Vec<(u64, u64)>, LoadError> {
    let container = Container::parse(whole)?;
    for (index, header) in container.program_headers()?.iter().enumerate() {
        let (start, size) = (header.vaddr.get(), header.memsz.get());
        if header.p_type.get() != PT_LOAD || from < start || from >= start.saturating_add(size) {
            continue;
        }
        let Some(bytes) = container.segment_data(whole, index)? else {
            continue;
        };
        let skip = usize::try_from(from - start).unwrap_or(usize::MAX);
        let Some(rest) = bytes.get(skip..) else {
            return Ok(Vec::new());
        };
        return Ok(rest
            .chunks_exact(8)
            .take(len.div_ceil(8))
            .zip((from..).step_by(8))
            .map(|(word, at)| (at, u64::from_le_bytes(word.try_into().unwrap_or_default())))
            .collect());
    }
    Ok(Vec::new())
}

/// The slot a linkage stub at `target` jumps through - `jmp [rip+rel32]`, `ff 25` - where `target`
/// lies in `segment`, which starts at `start`.
fn stub_slot((segment, start): (&[u8], u64), target: u64) -> Option<u64> {
    let at = usize::try_from(target.checked_sub(start)?).ok()?;
    let bytes = segment.get(at..at + 6)?;
    if bytes[..2] != [0xff, 0x25] {
        return None;
    }
    let rel = i32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
    (target + 6).checked_add_signed(i64::from(rel))
}

/// The instructions in `segment`, which starts at `start`, from `from` for at most `len` bytes, in
/// Intel syntax, a call or jump through a stub annotated with its slot's import. An instruction
/// that starts inside them is listed whole.
fn disassemble_code(
    (segment, start): (&[u8], u64),
    from: u64,
    len: usize,
    labels: &BTreeMap<u64, String>,
) -> Vec<(u64, String)> {
    let Some(code) = from
        .checked_sub(start)
        .and_then(|skip| usize::try_from(skip).ok())
        .and_then(|skip| segment.get(skip..))
    else {
        return Vec::new();
    };
    let end = from.saturating_add(u64::try_from(len).unwrap_or(u64::MAX));
    let mut decoder = Decoder::with_ip(64, code, from, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let mut listed = Vec::new();
    for instruction in &mut decoder {
        if instruction.ip() >= end {
            break;
        }
        let mut text = String::new();
        formatter.format(&instruction, &mut text);
        let through = matches!(instruction.mnemonic(), Mnemonic::Call | Mnemonic::Jmp)
            .then(|| stub_slot((segment, start), instruction.near_branch_target()))
            .flatten()
            .and_then(|slot| labels.get(&slot));
        if let Some(label) = through {
            text.push_str("  ; ");
            text.push_str(label);
        }
        listed.push((instruction.ip(), text));
    }
    listed
}

/// One place a module names an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Where, as an image offset: the instruction, or the data word a relocation fills.
    pub at: u64,
    /// The instruction in Intel syntax, or what the data word is.
    pub how: String,
    /// The start of the function holding the instruction - the first instruction after the
    /// `int3` padding before it - or `at` itself for a data word.
    pub within: u64,
}

/// Every place the module `whole` names image offset `target`: a call, jump or branch to it, an
/// instruction taking it by `rip`-relative address, and a data word a `RELATIVE` relocation fills
/// with it (a vtable or callback table). What reaches a function that never runs, walked one caller
/// at a time.
///
/// # Errors
///
/// When the container cannot be parsed.
pub fn references(whole: &[u8], target: u64) -> Result<Vec<Reference>, LoadError> {
    let container = Container::parse(whole)?;
    let mut found = Vec::new();
    for (index, header) in container.program_headers()?.iter().enumerate() {
        if header.p_type.get() != PT_LOAD || header.flags.get() & PF_X == 0 {
            continue;
        }
        if let Some(code) = container.segment_data(whole, index)? {
            found.extend(references_in((code, header.vaddr.get()), target));
        }
    }
    if let Some(dyn_bytes) = container.dynamic_bytes(whole)? {
        let info = DynamicInfo::parse(dyn_bytes);
        if info.rela != 0
            && info.relasz != 0
            && let Some(at) = container.table_offset(whole, &info, info.rela)?
        {
            let len = usize::try_from(info.relasz).unwrap_or(0);
            let table = whole.get(at..at.saturating_add(len)).unwrap_or_default();
            found.extend(
                parse_table(table)
                    .iter()
                    .filter(|entry| {
                        entry.kind() == orbistoun_elf::reloc::kind::RELATIVE
                            && u64::try_from(entry.addend.get()).ok() == Some(target)
                    })
                    .map(|entry| Reference {
                        at: entry.offset.get(),
                        how: "a data word relocated to it".to_owned(),
                        within: entry.offset.get(),
                    }),
            );
        }
    }
    Ok(found)
}

/// The instructions in `segment`, which starts at `start`, that name `target`, each with the start
/// of the function holding it: the nearest instruction before it that follows `int3` padding or
/// that a direct call in the segment targets, since a function can follow another unpadded.
fn references_in((segment, start): (&[u8], u64), target: u64) -> Vec<Reference> {
    let called: std::collections::HashSet<u64> =
        Decoder::with_ip(64, segment, start, DecoderOptions::NONE)
            .into_iter()
            .filter(|instruction| instruction.mnemonic() == Mnemonic::Call)
            .map(|instruction| instruction.near_branch_target())
            .filter(|&to| to != 0)
            .collect();
    let mut decoder = Decoder::with_ip(64, segment, start, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    let (mut within, mut after_padding) = (start, true);
    let mut found = Vec::new();
    for instruction in &mut decoder {
        let padding = instruction.mnemonic() == Mnemonic::Int3;
        if (after_padding && !padding) || called.contains(&instruction.ip()) {
            within = instruction.ip();
        }
        after_padding = padding;
        let names = instruction.near_branch_target() == target
            || (instruction.is_ip_rel_memory_operand()
                && instruction.ip_rel_memory_address() == target);
        if names {
            let mut how = String::new();
            formatter.format(&instruction, &mut how);
            found.push(Reference {
                at: instruction.ip(),
                how,
                within,
            });
        }
    }
    found
}

/// Decodes `code`, which starts at guest address `at`, into `found`.
fn sweep(code: &[u8], at: u64, found: &mut Inventory) {
    let mut decoder = Decoder::with_ip(64, code, at, DecoderOptions::NONE);
    for instruction in &mut decoder {
        let mnemonic = instruction.mnemonic();
        if mnemonic == Mnemonic::Syscall {
            found.syscalls.push(instruction.ip());
        } else if matches!(mnemonic, Mnemonic::Rdtsc | Mnemonic::Rdtscp) {
            found.counter_reads.push(instruction.ip());
        } else if AMD_ONLY.contains(&mnemonic) {
            found
                .amd_only
                .push((instruction.ip(), format!("{mnemonic:?}").to_lowercase()));
        }
    }
}

/// The `syscall` instructions in `code`, which starts at guest address `at`.
#[cfg(test)]
fn syscall_sites_in(code: &[u8], at: u64) -> Vec<u64> {
    let mut found = Inventory::default();
    sweep(code, at, &mut found);
    found.syscalls
}

#[cfg(test)]
mod tests {
    use super::syscall_sites_in;

    /// A `syscall` is found at its own address, and the same two bytes inside another
    /// instruction's immediate are not.
    #[test]
    fn a_syscall_is_found_where_it_starts() {
        let code = [
            0xb8, 0x14, 0x00, 0x00, 0x00, // mov eax, 0x14
            0x0f, 0x05, // syscall
            0x48, 0xb8, 0x0f, 0x05, 0x0f, 0x05, 0x00, 0x00, 0x00, 0x00, // mov rax, imm64
            0xc3, // ret
        ];
        assert_eq!(
            syscall_sites_in(&code, 0x4000_0000_1000),
            vec![0x4000_0000_1005]
        );
    }

    /// Each AMD-only instruction is found by its published encoding, and an ordinary one beside
    /// them is not.
    #[test]
    fn each_amd_only_instruction_is_found() {
        let code = [
            0x66, 0x0f, 0x79, 0xc1, // extrq xmm0, xmm1
            0xf2, 0x0f, 0x79, 0xc1, // insertq xmm0, xmm1
            0x0f, 0x01, 0xfa, // monitorx
            0x0f, 0x01, 0xfb, // mwaitx
            0x0f, 0x01, 0xfc, // clzero
            0x0f, 0x01, 0xfd, // rdpru
            0xf3, 0x0f, 0x01, 0xfa, // mcommit
            0x90, // nop
        ];
        let mut found = super::Inventory::default();
        super::sweep(&code, 0x1000, &mut found);
        let names: Vec<&str> = found.amd_only.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "extrq", "insertq", "monitorx", "mwaitx", "clzero", "rdpru", "mcommit"
            ]
        );
        assert_eq!(found.amd_only[0].0, 0x1000);
        assert!(found.syscalls.is_empty());
    }

    /// A call to the address and an instruction taking it by `rip`-relative address are each named,
    /// with the start of the function holding them - the first instruction after the `int3`
    /// padding before it, or a call's target where a function follows another with no padding; a
    /// jump elsewhere is not.
    #[test]
    fn calls_and_address_takes_of_an_address_are_found() {
        let code = [
            0xcc, 0xcc, // int3 padding
            0xe8, 0xf9, 0x0f, 0x00, 0x00, // 0x1002: call 0x2000
            0x48, 0x8d, 0x05, 0xf2, 0x0f, 0x00, 0x00, // 0x1007: lea rax, [rip+0xff2] = 0x2000
            0xc3, // 0x100e: ret
            0xcc, // int3 padding
            0xe9, 0xeb, 0x1f, 0x00, 0x00, // 0x1010: jmp 0x3000
            0xc3, // 0x1015: ret, and no padding
            0xe8, 0xfb, 0xff, 0xff, 0xff, // 0x1016: call 0x1016, so a call names it
            0x48, 0x8d, 0x05, 0xde, 0x0f, 0x00, 0x00, // 0x101b: lea rax, [rip+0xfde] = 0x2000
            0xc3,
        ];
        let found = super::references_in((&code, 0x1000), 0x2000);
        let sites: Vec<(u64, u64)> = found.iter().map(|r| (r.at, r.within)).collect();
        assert_eq!(
            sites,
            [(0x1002, 0x1002), (0x1007, 0x1002), (0x101b, 0x1016)]
        );
        assert!(found[0].how.starts_with("call"), "{}", found[0].how);
        assert_eq!(
            super::references_in((&code, 0x1000), 0x3000)[0].within,
            0x1010
        );
    }

    /// `rdtsc` and `rdtscp` are found where they start: each reads the host's own counter, which
    /// runs at the host's rate rather than the one the guest is told.
    #[test]
    fn each_counter_read_is_found() {
        let code = [
            0x0f, 0x31, // rdtsc
            0x48, 0xb8, 0x0f, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // mov rax, imm64
            0x0f, 0x01, 0xf9, // rdtscp
            0xc3, // ret
        ];
        let mut found = super::Inventory::default();
        super::sweep(&code, 0x1000, &mut found);
        assert_eq!(found.counter_reads, [0x1000, 0x100c]);
    }

    /// Code is listed an instruction a line, each at its own address, in Intel syntax: what a
    /// report's `image+0x...` points into, read without a scratch decoder.
    #[test]
    fn code_is_listed_an_instruction_a_line() {
        let code = [0xb8, 0x14, 0x00, 0x00, 0x00, 0x0f, 0x05, 0xc3, 0x90];
        let listed = super::disassemble_code(
            (&code, 0x1000),
            0x1000,
            8,
            &std::collections::BTreeMap::new(),
        );
        assert_eq!(
            listed,
            [
                (0x1000, "mov eax,14h".to_owned()),
                (0x1005, "syscall".to_owned()),
                (0x1007, "ret".to_owned()),
            ]
        );
    }

    /// A call to a procedure linkage stub is annotated with the import its slot holds: the stub
    /// `jmp [rip+rel32]` names the slot, and the slot the import.
    #[test]
    fn a_call_through_a_stub_names_its_import() {
        // 0x1000: call 0x1010; ret; padding to 0x1010; 0x1010: jmp [rip+0xfea] -> slot 0x2000.
        let mut code = vec![0xe8, 0x0b, 0x00, 0x00, 0x00, 0xc3];
        code.resize(0x10, 0xcc);
        code.extend([0xff, 0x25, 0xea, 0x0f, 0x00, 0x00]);
        let labels =
            std::collections::BTreeMap::from([(0x2000, "libScePad::scePadReadState".to_owned())]);
        let listed = super::disassemble_code((&code, 0x1000), 0x1000, 6, &labels);
        assert!(
            listed[0].1.ends_with("; libScePad::scePadReadState"),
            "{listed:?}"
        );
        assert_eq!(listed[1].1, "ret");
    }

    /// Code with no `syscall` lists none.
    #[test]
    fn code_without_a_syscall_lists_none() {
        assert!(syscall_sites_in(&[0x90, 0x90, 0xc3], 0x1000).is_empty());
    }
}
