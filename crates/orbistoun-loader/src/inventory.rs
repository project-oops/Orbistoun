//! What a module's code does that linking does not route: raw `syscall` sites (D724), and
//! instructions only AMD processors execute (D725).
//!
//! Read-only. An import reaches orbistoun through its thunk; a `syscall` instruction in the
//! module's own code enters the kernel directly, and an AMD-only instruction faults on another
//! vendor's host, so the plan lists where each one is before the guest runs rather than
//! learning of it only when one executes.

use iced_x86::{Decoder, DecoderOptions, Mnemonic};
use orbistoun_elf::Container;

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

/// Decodes `code`, which starts at guest address `at`, into `found`.
fn sweep(code: &[u8], at: u64, found: &mut Inventory) {
    let mut decoder = Decoder::with_ip(64, code, at, DecoderOptions::NONE);
    for instruction in &mut decoder {
        let mnemonic = instruction.mnemonic();
        if mnemonic == Mnemonic::Syscall {
            found.syscalls.push(instruction.ip());
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

    /// Code with no `syscall` lists none.
    #[test]
    fn code_without_a_syscall_lists_none() {
        assert!(syscall_sites_in(&[0x90, 0x90, 0xc3], 0x1000).is_empty());
    }
}
