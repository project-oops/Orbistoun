//! Thread-local storage: the block layout and where the thread pointer sits.
//!
//! x86-64 uses **variant II**, and the shape of it is the thing to get right: the
//! static TLS block sits *below* the thread pointer, not above it. A variable at
//! offset `x` within a module's TLS image is therefore read at a **negative** offset
//! from the thread pointer. Laying the block out above the pointer instead produces a
//! guest that reads whatever happens to precede its thread control block - plausible
//! values, wrong ones, and no fault to point at the cause.
//!
//! ```text
//!   low                                                              high
//!   +-----------------------------------------+----------------------+
//!   |          static TLS block               |         TCB          |
//!   |  init image  |        .tbss (zero)      |  self ptr, DTV, ...  |
//!   +-----------------------------------------+----------------------+
//!   ^                                         ^
//!   block base                                thread pointer (fs base)
//! ```
//!
//! # The self pointer is not optional
//!
//! The first word at the thread pointer must hold the thread pointer's own value. A
//! segment-relative load cannot produce the segment base any other way, so code that
//! needs its own TLS address reads `fs:0` - and gets zero if this is skipped, which
//! surfaces as a null dereference far from here.
//!
//! # What is deliberately not handled
//!
//! Dynamic TLS - a second module with its own block, reached through a descriptor
//! table - needs module loading that does not exist yet. Relocations that would need
//! it are reported, never guessed at.

use orbistoun_elf::Container;

use crate::LoadError;

/// `PT_TLS`.
pub const PT_TLS: u32 = 7;

/// Bytes reserved at and above the thread pointer for the thread control block.
///
/// The ABI fixes only the first word - the self pointer. Runtimes put a descriptor
/// table pointer and their own bookkeeping after it, so this leaves room rather than
/// sizing to the minimum: over-allocating a few dozen bytes per thread costs nothing,
/// and under-allocating means a guest runtime writes past its block into whatever is
/// next.
pub const TCB_SIZE: u64 = 64;

/// Module id for the main executable in the descriptor table.
///
/// Index zero is reserved as "no module", so the first real one is 1.
pub const MAIN_MODULE_ID: u64 = 1;

/// Where everything sits, computed from a `PT_TLS` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TlsLayout {
    /// Bytes of initialised data in the container.
    pub init_size: u64,
    /// Bytes the block occupies, including `.tbss` that must read as zero.
    pub total_size: u64,
    /// Alignment the header asked for.
    pub align: u64,
    /// `total_size` rounded up to `align` - the distance below the thread pointer.
    pub block_size: u64,
}

impl TlsLayout {
    /// Computes a layout from the three header fields that matter.
    ///
    /// An alignment of zero or one means "no constraint" in the format; both are
    /// normalised to one so the rounding below is a no-op rather than a division by
    /// zero.
    pub fn new(init_size: u64, total_size: u64, align: u64) -> Self {
        let align = if align == 0 { 1 } else { align };
        let block_size = total_size.div_ceil(align).saturating_mul(align);
        Self {
            init_size: init_size.min(total_size),
            total_size,
            align,
            block_size,
        }
    }

    /// Total bytes to reserve for one thread: the block plus its control block.
    pub const fn allocation_size(&self) -> u64 {
        self.block_size.saturating_add(TCB_SIZE)
    }

    /// The thread pointer for a block reserved at `base`.
    ///
    /// Above the block, because variant II grows downwards from here.
    pub const fn thread_pointer(&self, base: u64) -> u64 {
        base.saturating_add(self.block_size)
    }

    /// The thread-pointer-relative offset of a variable at `module_offset`.
    ///
    /// Negative by construction. A positive result would mean the layout is upside
    /// down, which is the mistake this whole module exists to avoid.
    pub const fn tp_offset(&self, module_offset: u64) -> i64 {
        (module_offset as i64).wrapping_sub(self.block_size as i64)
    }

    /// Bytes of `.tbss` - declared but not present in the container, so zeroed.
    pub const fn zero_fill(&self) -> u64 {
        self.total_size.saturating_sub(self.init_size)
    }

    /// Lays out one thread's initial storage into a freshly reserved block, and returns the thread
    /// pointer to install.
    ///
    /// `dest` is the [`Self::allocation_size`] bytes reserved at guest address `base`; `tdata` is the
    /// container's `PT_TLS` init image. Variant II, the layout every offset in the image was
    /// relocated against (a variable at module offset `m` reads `fs:[m - block_size]`, which lands at
    /// `base + m`): the init image sits at the bottom, `.tbss` is zeroed above it, and the thread
    /// pointer is at `block_size`. The one field the ABI fixes - the self-pointer at `[tp]`, which is
    /// what `fs:[0]` reads - is written, so a guest reading its own thread pointer gets the pointer
    /// rather than the zero an uninitialised block would give (the wall PPSA28061 hit).
    ///
    /// Everything the init image does not cover is zeroed, including the rest of the control block, so
    /// a runtime that reads its descriptor-table slot before setting it reads zero rather than rubble.
    /// A `dest` shorter than the init image or the self-pointer slot is filled as far as it reaches
    /// rather than panicking - the caller reserved it and a partial block is still better diagnosed by
    /// what the guest then does than by a fault in here.
    pub fn render_block(&self, base: u64, dest: &mut [u8], tdata: &[u8]) -> u64 {
        let tp = self.thread_pointer(base);
        dest.fill(0);
        let init = (self.init_size as usize).min(tdata.len()).min(dest.len());
        dest[..init].copy_from_slice(&tdata[..init]);
        let tp_slot = self.block_size as usize;
        if let Some(slot) = dest.get_mut(tp_slot..tp_slot + 8) {
            slot.copy_from_slice(&tp.to_le_bytes());
        }
        tp
    }
}

/// Finds a container's `PT_TLS` header, if it declares one, as `(layout, header index, init vaddr)`.
///
/// `None` is a normal answer, not a failure: plenty of images use no thread-local
/// storage at all, and one real commercial executable examined here is among them.
///
/// The third element is the header's `p_vaddr` - where the `.tdata` init image sits, which the
/// runtime needs to copy it into a thread's block. It reads from the placed image (at
/// `image base + vaddr`), where the loader has already put those bytes, rather than re-reading the
/// container, so the wrapper decode is not repeated.
pub fn layout_of(whole: &[u8]) -> Result<Option<(TlsLayout, usize, u64)>, LoadError> {
    let container = Container::parse(whole)?;
    let headers = container.program_headers()?;
    Ok(headers
        .iter()
        .enumerate()
        .find(|(_, ph)| ph.p_type.get() == PT_TLS)
        .map(|(index, ph)| {
            (
                TlsLayout::new(ph.filesz.get(), ph.memsz.get(), ph.align.get()),
                index,
                ph.vaddr.get(),
            )
        }))
}

#[cfg(test)]
mod tests {
    use super::{MAIN_MODULE_ID, TCB_SIZE, TlsLayout};

    #[test]
    fn the_block_sits_below_the_thread_pointer() {
        // Variant II, and the single most consequential fact in this module. Getting
        // it upside down makes a guest read whatever precedes its control block:
        // plausible values, wrong ones, and no fault to point at the cause.
        let layout = TlsLayout::new(16, 64, 8);
        assert_eq!(layout.thread_pointer(0x1000), 0x1000 + 64);
        assert!(
            layout.tp_offset(0) < 0,
            "a variable must be at a negative offset from the thread pointer"
        );
        assert_eq!(layout.tp_offset(0), -64);
        assert_eq!(
            layout.tp_offset(64),
            0,
            "the end of the block is the pointer"
        );
    }

    #[test]
    fn the_block_is_rounded_up_to_the_declared_alignment() {
        // An under-aligned block puts every variable in it at the wrong address, and
        // aligned loads on some of them fault.
        let layout = TlsLayout::new(0, 100, 32);
        assert_eq!(layout.block_size, 128);
        assert_eq!(layout.thread_pointer(0x2000), 0x2000 + 128);
    }

    #[test]
    fn a_zero_alignment_is_treated_as_unconstrained_not_as_a_divisor() {
        // The format allows 0 and 1 to mean "no constraint". Rounding by zero panics.
        for align in [0, 1] {
            let layout = TlsLayout::new(8, 24, align);
            assert_eq!(layout.block_size, 24, "align {align}");
        }
    }

    #[test]
    fn a_rendered_block_carries_the_init_image_the_zero_fill_and_the_self_pointer() {
        // The three things a thread needs before it touches `fs:`: its `.tdata` at the bottom, its
        // `.tbss` zeroed, and the self-pointer at the thread pointer so `fs:[0]` reads the pointer.
        let layout = TlsLayout::new(4, 16, 8);
        assert_eq!(layout.block_size, 16);
        let base = 0x4000_0000_0000_u64;
        let mut dest = vec![0xCC_u8; layout.allocation_size() as usize];
        let tdata = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let tp = layout.render_block(base, &mut dest, &tdata);

        assert_eq!(tp, base + 16, "thread pointer is above the block");
        // Only `init_size` bytes of the init image are taken, not all of `tdata`.
        assert_eq!(&dest[..4], &[0x11, 0x22, 0x33, 0x44]);
        // `.tbss` between the init image and the pointer reads as zero, not the fill.
        assert!(dest[4..16].iter().all(|&b| b == 0), "tbss must be zeroed");
        // The self-pointer at `[tp]` is the thread pointer itself, little-endian.
        assert_eq!(&dest[16..24], &tp.to_le_bytes());
        // The rest of the control block is zeroed, not left as the fill byte.
        assert!(
            dest[24..].iter().all(|&b| b == 0),
            "the control block above the self-pointer must be zeroed"
        );
    }

    #[test]
    fn rendering_into_a_short_block_fills_what_it_reaches_rather_than_panicking() {
        // The caller reserves the block; a mismatch is diagnosed by what the guest does, not by a
        // panic in a fault-adjacent path.
        let layout = TlsLayout::new(8, 32, 8);
        let mut dest = vec![0u8; 4];
        let _ = layout.render_block(0x1000, &mut dest, &[0xAB; 8]);
        assert_eq!(dest, vec![0xAB, 0xAB, 0xAB, 0xAB]);
    }

    #[test]
    fn tbss_is_the_part_with_no_bytes_in_the_container() {
        // Declared but not present, and the guest is entitled to read it as zero.
        let layout = TlsLayout::new(16, 64, 8);
        assert_eq!(layout.zero_fill(), 48);
    }

    #[test]
    fn an_init_size_larger_than_the_total_is_clamped_rather_than_trusted() {
        // A corrupt header claiming more initialised bytes than the block holds would
        // otherwise copy past the end of the allocation.
        let layout = TlsLayout::new(999, 64, 8);
        assert_eq!(layout.init_size, 64);
        assert_eq!(layout.zero_fill(), 0);
    }

    #[test]
    fn the_allocation_leaves_room_for_the_control_block() {
        // The self pointer and the descriptor table pointer live at and above the
        // thread pointer. Sizing to the block alone means the guest runtime writes
        // past its allocation into whatever is next.
        let layout = TlsLayout::new(0, 64, 8);
        assert_eq!(layout.allocation_size(), 64 + TCB_SIZE);
    }

    #[test]
    fn the_main_module_is_not_module_zero() {
        // Index zero means "no module" in the descriptor table, so a main executable
        // recorded as zero is indistinguishable from an unresolved one.
        assert_eq!(MAIN_MODULE_ID, 1);
    }
}
