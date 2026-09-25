//! GPU translation - command streams to Vulkan, shaders to SPIR-V.
//!
//! The hardest remaining problem in emulating this target, and the one place in
//! the whole project with a genuinely cheap correctness oracle: render a frame,
//! diff the framebuffer against a reference, get a number. Nothing else here can
//! be checked that mechanically.
//!
//! Two translations live here and they are quite different jobs:
//!
//! - **Command stream**: vendor packet buffers to Vulkan command buffers. There are
//!   two formats to handle, one per target generation. Structural, high-volume, and
//!   where the hardware features with no Vulkan equivalent hurt.
//! - **Shaders**: vendor shader bytecode to SPIR-V. Pattern-heavy and
//!   differentially verifiable, which makes it the best target for tooling
//!   assistance.
//!
//! # The unified-memory problem
//!
//! The console has one coherent memory pool shared by CPU and GPU. Guests map
//! GPU-visible memory and write it from the CPU with no explicit transfer. A
//! discrete PC GPU across PCIe has no equivalent, so this layer has to detect
//! those writes and synthesise the transfers. That is a semantic gap, not a
//! performance one, and pretending otherwise produces frames that are subtly
//! wrong rather than obviously broken.
//!
//! # The backend seam
//!
//! **This crate names no graphics API and depends on none.** Translation emits
//! [`RenderCommand`]s; a backend crate turns those into whatever its API wants. That
//! boundary is enforced by `cargo` rather than by discipline - there is no `ash`
//! dependency here to leak through.
//!
//! [`RecordingBackend`] is why the seam exists now rather than later: it makes
//! translation testable with no GPU, no window, and no driver.
//!
//! # Instrumentation comes before translation
//!
//! [`walk`] decodes a submitted command buffer into packets without understanding a
//! single command, and `orbistoun-shader` does the same for shader bytecode. Neither
//! translates anything, and both are worth having first: they turn an opaque
//! submission into counts, and counts are what decide where translation effort goes.
//!
//! The pattern is the one the import survey established. "Emulate the operating
//! system" became a frequency-ranked list of functions; "translate the command stream"
//! becomes a frequency-ranked list of packet opcodes, and "translate shaders" becomes
//! a list of instructions ranked by how many shaders each one blocks.
//!
//! # Status
//!
//! Declarations, the backend vocabulary, and packet-level instrumentation. The command stream
//! and shaders translate to backend-neutral render commands here; the sibling `orbistoun-gpu-vulkan`
//! executes compute dispatches and draws on a real device (this crate names no graphics API,
//! principle 12).

mod backend;
pub mod cp;
pub mod packet;
pub mod perf;
pub mod pipeline;
pub mod registers;
mod render;
pub mod tiling;

/// A content hash of guest words - what a shader or texture is, for recognising it again
/// (worklogs 843, 844).
///
/// **A fast hash, not a keyed one**: nothing here is adversarial - a guest cannot choose its way into a
/// collision that matters to it - and the standard library's hasher, built to resist one, cost more
/// than the draws whose textures it hashed. Two words at a time, multiplied and mixed; every word and
/// the length feed it, so an equal hash means equal words to within the same 64-bit chance any content
/// key has.
#[must_use]
pub fn content_hash(words: &[u32]) -> u64 {
    let mut hasher = ContentHasher::new(words.len());
    for word in words {
        hasher.word(*word);
    }
    hasher.finish()
}

/// [`content_hash`], fed a piece at a time - so words that are not contiguous, a pitched texture's
/// rows, hash where they lie, to the same value their gathered copy would (worklog 851).
#[derive(Debug, Clone, Copy)]
pub struct ContentHasher {
    hash: u64,
    pending: Option<u32>,
}

impl ContentHasher {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;

    /// A hasher for `words` words in all.
    #[must_use]
    pub const fn new(words: usize) -> Self {
        Self {
            hash: (words as u64).wrapping_mul(Self::K),
            pending: None,
        }
    }

    /// Feeds one word.
    pub const fn word(&mut self, word: u32) {
        match self.pending.take() {
            None => self.pending = Some(word),
            Some(first) => {
                let pair = first as u64 | ((word as u64) << 32);
                self.hash = (self.hash ^ pair).wrapping_mul(Self::K).rotate_left(29);
            }
        }
    }

    /// Feeds little-endian words; a trailing partial word is ignored.
    pub fn bytes(&mut self, bytes: &[u8]) {
        let mut rest = &bytes[..bytes.len() - bytes.len() % 4];
        // A word left over from the last piece completes its pair first; then eight bytes at a time.
        if self.pending.is_some()
            && let Some((w, after)) = rest.split_first_chunk::<4>()
        {
            self.word(u32::from_le_bytes(*w));
            rest = after;
        }
        let mut pairs = rest.chunks_exact(8);
        for p in &mut pairs {
            let pair = u64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]]);
            self.hash = (self.hash ^ pair).wrapping_mul(Self::K).rotate_left(29);
        }
        for w in pairs.remainder().chunks_exact(4) {
            self.word(u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
        }
    }

    /// The hash.
    #[must_use]
    pub const fn finish(self) -> u64 {
        let mut hash = self.hash;
        if let Some(last) = self.pending {
            hash = (hash ^ last as u64).wrapping_mul(Self::K).rotate_left(29);
        }
        hash ^ (hash >> 32)
    }
}

#[cfg(test)]
mod content_hash_tests {
    use super::{ContentHasher, content_hash};

    /// The hash as it was computed before [`ContentHasher`] existed - the oracle the streaming form
    /// must agree with, because the backend keys uploaded textures by it.
    fn original(words: &[u32]) -> u64 {
        const K: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut hash = (words.len() as u64).wrapping_mul(K);
        let mut pairs = words.chunks_exact(2);
        for pair in &mut pairs {
            let word = u64::from(pair[0]) | (u64::from(pair[1]) << 32);
            hash = (hash ^ word).wrapping_mul(K).rotate_left(29);
        }
        if let [last] = pairs.remainder() {
            hash = (hash ^ u64::from(*last)).wrapping_mul(K).rotate_left(29);
        }
        hash ^ (hash >> 32)
    }

    /// **Streaming gives the same hash however the words are split** (worklog 851): whole, word by
    /// word, and as rows of odd widths fed as bytes, for even and odd totals.
    #[test]
    fn streaming_matches_the_original_however_it_is_fed() {
        for total in [0usize, 1, 2, 7, 64, 99] {
            let words: Vec<u32> = (0..total as u32)
                .map(|i| i.wrapping_mul(0x0101_0101) ^ 0xdead_beef)
                .collect();
            assert_eq!(content_hash(&words), original(&words), "{total} words");
            for row in [1usize, 3, 5] {
                let mut hasher = ContentHasher::new(total);
                for chunk in words.chunks(row) {
                    let bytes: Vec<u8> = chunk.iter().flat_map(|w| w.to_le_bytes()).collect();
                    hasher.bytes(&bytes);
                }
                assert_eq!(
                    hasher.finish(),
                    original(&words),
                    "{total} words in rows of {row}"
                );
            }
        }
        assert_ne!(content_hash(&[1, 2]), content_hash(&[2, 1]));
    }
}

pub use backend::{
    BackendError, RecordingBackend, Rect, RenderBackend, RenderCommand, Resource, ResourceId,
    ShaderStage, USER_DATA_BLOCK_OFFSETS, USER_DATA_BLOCK_WORDS, USER_DATA_WORDS,
};
pub use packet::{Packet, PacketKind, PacketWalk, walk};
pub use registers::{
    BlendControl, BlendFactor, BufferDescriptor, ColourTarget, ColourTargetExtent, CombineFunc,
    CompareFunc, DepthControl, DispatchCall, DrawCall, DrawCorrelation, DrawKind, DrawOrDispatch,
    ImageDescriptor, PrimitiveTopology, RegisterWrite, Scissor, ShaderCandidate, StencilControl,
    StencilOp, SwizzleMode, TargetMask, ViewportTransform, Vocabulary, VocabularyError,
    blend_control_at, buffer_descriptor_at, colour_swizzle_mode_at, colour_target_at,
    colour_target_extent_at, correlate_draws, decode_blend_control, decode_blend_factor,
    decode_buffer_descriptor, decode_colour_swizzle_mode, decode_colour_target_extent,
    decode_combine_func, decode_compare_func, decode_depth_control, decode_image_descriptor,
    decode_primitive_topology, decode_scissor, decode_stencil_control, decode_stencil_op,
    decode_swizzle_mode, decode_target_mask, depth_control_at, dispatch_calls, draw_calls,
    primitive_topology_at, register_writes, scissor_at, shader_candidates, stencil_control_at,
    target_mask_at,
};
pub use render::{FrameOutcome, drive};
pub use tiling::{
    DetileError, Surface, SurfaceError, detile_64kb_rx_bpp4, detile_colour_target, detile_image,
    detile_texture, tiled_byte_offset_64kb_rx_bpp4,
};

use orbistoun_hle::guest_module;

pub mod agc;
pub mod agc_driver;
pub mod ampr;

guest_module! {
    "libSceGnmDriver" {
        "sceGnmSubmitCommandBuffers" => 5,
        "sceGnmSubmitAndFlipCommandBuffers" => 7,
        "sceGnmAreSubmitsAllowed" => 0,
        "sceGnmSubmitDone" => 0,
        "sceGnmDispatchInitDefaultHardwareState" => 2,
        "sceGnmDispatchDirect" => 6,
    }
}

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// A rejected argument, as a caller that tests `< 0` sees it.
///
/// `sceGnmDispatch*` are `int32` calls whose failure is negative (obSCEne: "negative on a rejected
/// argument"), unlike the `0x8002_00xx` a kernel call returns - these are a library's own
/// convention, and the one thing measured about them is the sign.
const REJECTED: u64 = -1_i64 as u64;

/// Writes a run of dwords into a guest command buffer, little-endian.
///
/// # Safety
///
/// `at` must be a guest address with room for `dwords.len()` dwords, under the identity mapping
/// (D014) - which is the contract every command builder has: the guest hands the buffer and its
/// size, and a builder writes within the size it was given.
unsafe fn write_dwords(at: usize, dwords: &[u32]) {
    for (index, dword) in dwords.iter().enumerate() {
        // SAFETY: the caller guarantees `at` addresses `dwords.len()` dwords of guest-owned buffer;
        // `index` is within that by construction. Unaligned because a command buffer promises no
        // more than dword granularity.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u32>(at + index * 4),
                *dword,
            );
        }
    }
}

/// How many dwords `sceGnmDispatchInitDefaultHardwareState` reserves for the default compute state.
///
/// `0x100`, the size obSCEne records the call returning. The call writes that much and hands the
/// count back so a guest knows where its own commands may begin.
const DEFAULT_HW_STATE_DWORDS: u64 = 0x100;

/// `sceGnmDispatchInitDefaultHardwareState(cmdbuf, size_dwords)`.
///
/// Writes the default compute hardware state into the command buffer and returns the number of
/// dwords it reserved (`0x100`), or `0` if the buffer is too small - the contract obSCEne's
/// `165-gnm/dispatch-init` measures.
///
/// **The reservation is honest; its contents are placeholder.** The exact register sequence a
/// console's builder emits for the default state is not documented by anything lawful here, so the
/// reserved space is filled with valid **no-op** packets (type-2 fillers) rather than an invented
/// state. A guest that submits it gets a preamble that does nothing and walks cleanly; when the GPU
/// translation reaches compute state, the fillers become the real writes. Returning `0x100` without
/// writing anything would be the D125 shape - a count a caller trusts, backed by nothing.
fn dispatch_init_default_hardware_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (cmdbuf, size) = (args[0], args[1]);
    if cmdbuf == 0 || size < DEFAULT_HW_STATE_DWORDS {
        return 0;
    }
    let Ok(at) = usize::try_from(cmdbuf) else {
        return 0;
    };
    let fillers = vec![packet::build::filler(); DEFAULT_HW_STATE_DWORDS as usize];
    // SAFETY: the guest declared `size` dwords of buffer and `size >= DEFAULT_HW_STATE_DWORDS`, so
    // writing that many stays within it (D014).
    unsafe { write_dwords(at, &fillers) };
    DEFAULT_HW_STATE_DWORDS
}

/// `sceGnmDispatchDirect(cmdbuf, size_dwords, x, y, z, flags)`.
///
/// Writes a direct compute dispatch of `x`·`y`·`z` thread groups into the command buffer as PM4 and
/// answers `0`, or a negative code when the buffer cannot hold it - obSCEne's `165-gnm/dispatch-
/// direct`. `flags` is accepted and not modelled: nothing observed reads it back, and the dispatch
/// it would modify is one this does not execute.
///
/// It writes the documented `DISPATCH_DIRECT` packet - five dwords. A console's own builder writes
/// more around it (the surrounding hardware state), which is why the call requires a larger buffer
/// than the packet alone; that surrounding state is not modelled, so this writes the dispatch and
/// leaves the rest of the buffer as the guest prepared it, rather than inventing an encoding.
fn dispatch_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (cmdbuf, size) = (args[0], args[1]);
    let packet = packet::build::dispatch_direct(args[2] as u32, args[3] as u32, args[4] as u32);
    if cmdbuf == 0 || size < packet.len() as u64 {
        return REJECTED;
    }
    let Ok(at) = usize::try_from(cmdbuf) else {
        return REJECTED;
    };
    // SAFETY: `size >= packet.len()` dwords of guest buffer were declared by the guest (D014).
    unsafe { write_dwords(at, &packet) };
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// The command **builders** - the calls that write PM4 into a guest buffer without touching a GPU.
/// The submit and flip calls stay declared-only for now: they are where translation to Vulkan
/// begins, and a stub that claimed a submission had happened would be the worst kind of plausible
/// output (this crate's whole first job is to *not* pretend a frame was drawn).
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        (
            "sceGnmDispatchInitDefaultHardwareState",
            dispatch_init_default_hardware_state,
        ),
        ("sceGnmDispatchDirect", dispatch_direct),
    ]
}
