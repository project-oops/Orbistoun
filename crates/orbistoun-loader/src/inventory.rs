//! What a module's code does that linking does not route: raw `syscall` sites (D724).
//!
//! Read-only. An import reaches orbistoun through its thunk; a `syscall` instruction in the
//! module's own code enters the kernel directly, so the plan lists where each one is before
//! the guest runs rather than learning of it only when one executes.

use iced_x86::{Decoder, DecoderOptions, Mnemonic};
use orbistoun_elf::Container;

use crate::LoadError;

/// `PT_LOAD`.
const PT_LOAD: u32 = 1;

/// `PF_X`: the segment is executable.
const PF_X: u32 = 1;

/// Every `syscall` instruction in the executable segments of `whole`, placed at `base`, by
/// guest address.
///
/// A linear sweep: each executable segment is decoded from its start, and an undecodable byte
/// is skipped. Data embedded in code can decode as a false site, never hide a real one that
/// the sweep reaches aligned.
///
/// # Errors
///
/// When the container cannot be parsed.
pub fn syscall_sites(whole: &[u8], base: u64) -> Result<Vec<u64>, LoadError> {
    let container = Container::parse(whole)?;
    let mut sites = Vec::new();
    for (index, header) in container.program_headers()?.iter().enumerate() {
        if header.p_type.get() != PT_LOAD || header.flags.get() & PF_X == 0 {
            continue;
        }
        let Some(code) = container.segment_data(whole, index)? else {
            continue;
        };
        let at = base.wrapping_add(header.vaddr.get());
        sites.extend(syscall_sites_in(code, at));
    }
    sites.sort_unstable();
    Ok(sites)
}

/// The `syscall` instructions in `code`, which starts at guest address `at`.
fn syscall_sites_in(code: &[u8], at: u64) -> Vec<u64> {
    let mut decoder = Decoder::with_ip(64, code, at, DecoderOptions::NONE);
    let mut sites = Vec::new();
    for instruction in &mut decoder {
        if instruction.mnemonic() == Mnemonic::Syscall {
            sites.push(instruction.ip());
        }
    }
    sites
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

    /// Code with no `syscall` lists none.
    #[test]
    fn code_without_a_syscall_lists_none() {
        assert!(syscall_sites_in(&[0x90, 0x90, 0xc3], 0x1000).is_empty());
    }
}
