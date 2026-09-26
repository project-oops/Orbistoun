//! Thread-local storage: the block layout and where the thread pointer sits.
//!
//! x86-64 uses variant II: the static TLS block sits below the thread pointer, so a
//! variable at offset `x` in a module's TLS image is read at a negative offset from it.
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
//! The first word at the thread pointer holds the thread pointer's own value, since a
//! segment-relative load cannot produce the segment base any other way; code needing its
//! own TLS address reads `fs:0`. Relocations that need another module's dynamic TLS block
//! are reported, never guessed at.

use orbistoun_elf::Container;

use crate::LoadError;

/// `PT_TLS`.
pub const PT_TLS: u32 = 7;

/// Bytes reserved at and above the thread pointer for the thread control block.
///
/// The ABI fixes only the first word, the self pointer. Runtimes put a descriptor-table
/// pointer and their own bookkeeping after it, so this leaves room rather than sizing to
/// the minimum.
pub const TCB_SIZE: u64 = 64;

/// Module id for the main executable in the descriptor table.
///
/// Index zero means "no module", so the first real one is 1.
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
    /// `total_size` rounded up to `align`: the distance below the thread pointer.
    pub block_size: u64,
}

impl TlsLayout {
    /// Computes a layout from the three header fields that matter.
    ///
    /// An alignment of zero or one means "no constraint" in the format; both normalise to
    /// one so the rounding is a no-op rather than a division by zero.
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
    /// Negative by construction; variant II places the block below the pointer.
    pub const fn tp_offset(&self, module_offset: u64) -> i64 {
        (module_offset as i64).wrapping_sub(self.block_size as i64)
    }

    /// Bytes of `.tbss`: declared but not present in the container, so zeroed.
    pub const fn zero_fill(&self) -> u64 {
        self.total_size.saturating_sub(self.init_size)
    }

    /// Lays out one thread's initial storage into a freshly reserved block, and returns the
    /// thread pointer to install.
    ///
    /// `dest` is the [`Self::allocation_size`] bytes reserved at guest address `base`;
    /// `tdata` is the container's `PT_TLS` init image. The init image sits at the bottom,
    /// `.tbss` is zeroed above it, and the thread pointer is at `block_size`, so a variable
    /// at module offset `m` read as `fs:[m - block_size]` lands at `base + m`. The self
    /// pointer at `[tp]` is written so `fs:[0]` reads the pointer. Everything else is zeroed,
    /// including the rest of the control block. A `dest` too short is filled as far as it
    /// reaches rather than panicking.
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

/// Finds a container's `PT_TLS` header, if it declares one, as `(layout, header index, init
/// vaddr)`.
///
/// `None` is a normal answer: many images use no thread-local storage. The third element is
/// the header's `p_vaddr`, where the `.tdata` init image sits; the runtime copies it from
/// the placed image at `image base + vaddr` rather than decoding the container again.
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

    /// The static block sits below the thread pointer (variant II).
    #[test]
    fn the_block_sits_below_the_thread_pointer() {
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

    /// The block rounds up to the declared alignment.
    #[test]
    fn the_block_is_rounded_up_to_the_declared_alignment() {
        // An under-aligned block misplaces every variable in it.
        let layout = TlsLayout::new(0, 100, 32);
        assert_eq!(layout.block_size, 128);
        assert_eq!(layout.thread_pointer(0x2000), 0x2000 + 128);
    }

    /// An alignment of 0 or 1 means unconstrained and never divides.
    #[test]
    fn a_zero_alignment_is_treated_as_unconstrained_not_as_a_divisor() {
        for align in [0, 1] {
            let layout = TlsLayout::new(8, 24, align);
            assert_eq!(layout.block_size, 24, "align {align}");
        }
    }

    /// A rendered block holds the init image, zeroed `.tbss` and the self pointer.
    #[test]
    fn a_rendered_block_carries_the_init_image_the_zero_fill_and_the_self_pointer() {
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

    /// A short destination is filled as far as it reaches without panicking.
    #[test]
    fn rendering_into_a_short_block_fills_what_it_reaches_rather_than_panicking() {
        let layout = TlsLayout::new(8, 32, 8);
        let mut dest = vec![0u8; 4];
        let _ = layout.render_block(0x1000, &mut dest, &[0xAB; 8]);
        assert_eq!(dest, vec![0xAB, 0xAB, 0xAB, 0xAB]);
    }

    /// `.tbss` is the declared size not present in the container.
    #[test]
    fn tbss_is_the_part_with_no_bytes_in_the_container() {
        let layout = TlsLayout::new(16, 64, 8);
        assert_eq!(layout.zero_fill(), 48);
    }

    /// An init size larger than the total is clamped, so a corrupt header cannot copy past
    /// the allocation.
    #[test]
    fn an_init_size_larger_than_the_total_is_clamped_rather_than_trusted() {
        let layout = TlsLayout::new(999, 64, 8);
        assert_eq!(layout.init_size, 64);
        assert_eq!(layout.zero_fill(), 0);
    }

    /// The allocation leaves room for the control block above the thread pointer.
    #[test]
    fn the_allocation_leaves_room_for_the_control_block() {
        let layout = TlsLayout::new(0, 64, 8);
        assert_eq!(layout.allocation_size(), 64 + TCB_SIZE);
    }

    /// The main module id is not the "no module" id zero.
    #[test]
    fn the_main_module_is_not_module_zero() {
        assert_eq!(MAIN_MODULE_ID, 1);
    }
}
