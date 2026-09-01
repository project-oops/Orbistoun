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
//! Declarations, the backend vocabulary, and packet-level instrumentation. Nothing is
//! translated to Vulkan yet.

mod backend;
pub mod packet;
pub mod pipeline;
pub mod registers;

pub use backend::{
    BackendError, RecordingBackend, Rect, RenderBackend, RenderCommand, ResourceId, ShaderStage,
};
pub use packet::{Packet, PacketKind, PacketWalk, walk};
pub use registers::{
    RegisterWrite, ShaderCandidate, Vocabulary, VocabularyError, register_writes, shader_candidates,
};

use orbistoun_hle::guest_module;

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
