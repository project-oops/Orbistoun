//! Where a placed module's unwind tables are, for the unwinder that asks by address (D768).
//!
//! A module carries its frame-description entries in `.eh_frame` and a sorted index to them in
//! `.eh_frame_hdr`, which the `PT_GNU_EH_FRAME` header locates. The index's second field is the
//! address of `.eh_frame`, encoded as the header states; every module in the corpus encodes it
//! pc-relative as a signed four-byte value (`DW_EH_PE_pcrel | DW_EH_PE_sdata4`).

use orbistoun_elf::Container;

use crate::Image;

/// `PT_GNU_EH_FRAME`.
pub const PT_GNU_EH_FRAME: u32 = 0x6474_e550;

/// `PT_LOAD`.
const PT_LOAD: u32 = 1;

/// The one version of `.eh_frame_hdr` there is.
const HDR_VERSION: u8 = 1;

/// `DW_EH_PE_pcrel | DW_EH_PE_sdata4`: a signed four-byte offset from the field's own address.
const PCREL_SDATA4: u8 = 0x1b;

/// `DW_EH_PE_datarel | DW_EH_PE_sdata4`: a signed four-byte offset from the header's start.
const DATAREL_SDATA4: u8 = 0x3b;

/// `DW_EH_PE_udata8` and `DW_EH_PE_absptr`: the address itself, eight bytes.
const ABSOLUTE_8: [u8; 2] = [0x04, 0x00];

/// A placed module's unwind tables and code, as runtime addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnwindSections {
    /// `.eh_frame_hdr`, the index.
    pub eh_frame_hdr: u64,
    /// `.eh_frame`, the frame descriptions.
    pub eh_frame: u64,
    /// How many bytes of `.eh_frame` there are.
    pub eh_frame_len: u64,
    /// The first loadable segment, the module's code.
    pub text: u64,
    /// That segment's size in memory.
    pub text_len: u64,
}

/// The address of `.eh_frame` a header at `hdr` states, from the header's first eight bytes.
///
/// `None` for a version other than 1 or an encoding no module in the corpus uses.
#[must_use]
pub fn eh_frame_from_hdr(hdr: u64, bytes: &[u8]) -> Option<u64> {
    let [version, encoding, ..] = *bytes.get(..4)? else {
        return None;
    };
    if version != HDR_VERSION {
        return None;
    }
    let four = |at: usize| -> Option<i64> {
        let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(i64::from(i32::from_le_bytes(raw)))
    };
    match encoding {
        PCREL_SDATA4 => Some(hdr.wrapping_add(4).wrapping_add_signed(four(4)?)),
        DATAREL_SDATA4 => Some(hdr.wrapping_add_signed(four(4)?)),
        e if ABSOLUTE_8.contains(&e) => {
            let raw: [u8; 8] = bytes.get(4..12)?.try_into().ok()?;
            Some(u64::from_le_bytes(raw))
        }
        _ => None,
    }
}

/// A placed module's unwind sections, or `None` when it has no `PT_GNU_EH_FRAME`.
///
/// `.eh_frame`'s length is not in the index. It runs to the index when the index follows it, as in
/// every module in the corpus, and otherwise to the end of the segment that holds it; a parser stops
/// at the terminating zero entry either way.
#[must_use]
pub fn sections_of(image: &Image, whole: &[u8]) -> Option<UnwindSections> {
    let container = Container::parse(whole).ok()?;
    let headers = container.program_headers().ok()?;
    let base = image.base();
    let hdr = base.saturating_add(
        headers
            .iter()
            .find(|h| h.p_type.get() == PT_GNU_EH_FRAME)?
            .vaddr
            .get(),
    );
    let loads: Vec<(u64, u64)> = headers
        .iter()
        .filter(|h| h.p_type.get() == PT_LOAD)
        .map(|h| (base.saturating_add(h.vaddr.get()), h.memsz.get()))
        .collect();
    let (text, text_len) = *loads.first()?;
    let eh_frame = eh_frame_from_hdr(hdr, &image.space().read(hdr, 12)?)?;
    let eh_frame_len = if eh_frame < hdr {
        hdr - eh_frame
    } else {
        let (start, len) = loads
            .iter()
            .find(|(start, len)| (*start..start.saturating_add(*len)).contains(&eh_frame))?;
        start.saturating_add(*len) - eh_frame
    };
    Some(UnwindSections {
        eh_frame_hdr: hdr,
        eh_frame,
        eh_frame_len,
        text,
        text_len,
    })
}

#[cfg(test)]
mod tests {
    /// The header PPSA02664's own libc carries: pc-relative, `.eh_frame` `0x1b388` below the
    /// field at `+4`.
    #[test]
    fn a_pc_relative_header_points_back_to_its_frames() {
        let hdr = 0x4800_0294_4164;
        let mut bytes = vec![1, 0x1b, 0x03, 0x3b];
        bytes.extend_from_slice(&(-0x1b388_i32).to_le_bytes());
        assert_eq!(
            super::eh_frame_from_hdr(hdr, &bytes),
            Some(hdr + 4 - 0x1b388)
        );
    }

    /// An unknown version or encoding is not guessed at.
    #[test]
    fn an_unknown_version_or_encoding_answers_nothing() {
        assert_eq!(
            super::eh_frame_from_hdr(0x1000, &[2, 0x1b, 3, 0x3b, 0, 0, 0, 0]),
            None
        );
        assert_eq!(
            super::eh_frame_from_hdr(0x1000, &[1, 0x1c, 3, 0x3b, 0, 0, 0, 0]),
            None
        );
        assert_eq!(super::eh_frame_from_hdr(0x1000, &[1, 0x1b]), None);
    }
}
