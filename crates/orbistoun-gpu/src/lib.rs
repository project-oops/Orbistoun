//! GPU translation - command streams to Vulkan, shaders to SPIR-V.
//!
//! The command stream (vendor packet buffers) decodes into backend-neutral [`RenderCommand`]s;
//! shaders (vendor shader bytecode) translate to SPIR-V. This crate names no graphics API and
//! depends on none: a backend crate such as `orbistoun-gpu-vulkan` turns render commands into
//! device work, and [`RecordingBackend`] makes translation testable with no GPU.
//!
//! The hardware has one coherent memory pool shared by CPU and GPU, and guests write GPU-visible
//! memory from the CPU with no explicit transfer. A discrete host GPU has no equivalent, so this
//! layer detects those writes and synthesises the transfers.

mod backend;
pub mod cp;
pub mod packet;
pub mod perf;
pub mod pipeline;
pub mod registers;
mod render;
pub mod tiling;

/// A content hash of guest words - what a shader or texture is, for recognising it again.
///
/// A fast, unkeyed hash: a guest cannot usefully choose a collision, and a keyed hasher costs more
/// than the draws whose textures it hashes. Every word and the length feed it, so equal hashes
/// mean equal words to within the usual 64-bit chance.
#[must_use]
pub fn content_hash(words: &[u32]) -> u64 {
    let mut hasher = ContentHasher::new(words.len());
    for word in words {
        hasher.word(*word);
    }
    hasher.finish()
}

/// [`content_hash`], fed a piece at a time, so non-contiguous words such as a pitched texture's
/// rows hash where they lie, to the value their gathered copy would.
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
        // A word left over from the last piece completes its pair first; then eight bytes at a
        // time.
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

    /// The reference hash the streaming form must agree with, because the backend keys uploaded
    /// textures by it.
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

    /// Streaming gives the same hash however the words are split: whole, word by word, and as rows
    /// of odd widths fed as bytes, for even and odd totals.
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
/// `sceGnmDispatch*` are `int32` calls whose failure is negative, unlike the `0x8002_00xx` a
/// kernel call returns. The sign is the one measured fact about them.
const REJECTED: u64 = -1_i64 as u64;

/// Writes a run of dwords into a guest command buffer, little-endian.
///
/// # Safety
///
/// `at` must be an identity-mapped guest address with room for `dwords.len()` dwords: the guest
/// hands the buffer and its size, and a builder writes within the size it was given.
unsafe fn write_dwords(at: usize, dwords: &[u32]) {
    for (index, dword) in dwords.iter().enumerate() {
        // SAFETY: the caller guarantees `at` addresses `dwords.len()` dwords of guest-owned buffer;
        // `index` is within that by construction. Unaligned because a command buffer promises only
        // dword granularity.
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
/// `0x100`, the size obSCEne records the call returning, so a guest knows where its own commands
/// begin.
const DEFAULT_HW_STATE_DWORDS: u64 = 0x100;

/// `sceGnmDispatchInitDefaultHardwareState(cmdbuf, size_dwords)`.
///
/// Reserves the default compute hardware state and returns the dwords reserved (`0x100`), or `0`
/// if the buffer is too small, as obSCEne's `165-gnm/dispatch-init` measures. The register
/// sequence the hardware's builder emits is undocumented here, so the space is filled with type-2
/// no-op packets rather than an invented state; a returned count always has written dwords behind
/// it (D010).
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
    // the write stays within it.
    unsafe { write_dwords(at, &fillers) };
    DEFAULT_HW_STATE_DWORDS
}

/// `sceGnmDispatchDirect(cmdbuf, size_dwords, x, y, z, flags)`.
///
/// Writes a direct compute dispatch of `x` by `y` by `z` thread groups as PM4 and answers `0`, or
/// a negative code when the buffer cannot hold it (obSCEne's `165-gnm/dispatch-direct`). `flags`
/// is accepted and not modelled.
///
/// Writes the documented five-dword `DISPATCH_DIRECT` packet. The hardware's builder writes
/// surrounding state as well, which is why the call requires a larger buffer; that state is not
/// modelled, and the rest of the buffer is left as the guest prepared it.
fn dispatch_direct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (cmdbuf, size) = (args[0], args[1]);
    let packet = packet::build::dispatch_direct(args[2] as u32, args[3] as u32, args[4] as u32);
    if cmdbuf == 0 || size < packet.len() as u64 {
        return REJECTED;
    }
    let Ok(at) = usize::try_from(cmdbuf) else {
        return REJECTED;
    };
    // SAFETY: the guest declared at least `packet.len()` dwords of buffer.
    unsafe { write_dwords(at, &packet) };
    OK
}

/// Implementations this crate provides, by symbol name: the command builders, which write PM4
/// into a guest buffer without touching a GPU.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        (
            "sceGnmDispatchInitDefaultHardwareState",
            dispatch_init_default_hardware_state,
        ),
        ("sceGnmDispatchDirect", dispatch_direct),
    ]
}
