//! `libSceAvPlayer` - the bundled media player, which decodes and presents a stream for a title.
//!
//! The player's lifecycle is served as obSCEne measured it (`000-boot/avplayer-lifecycle`,
//! REQ-20261012T2011Z-625e, `reports/hardware/report-20261014-batch2.log` 1427-1452): `InitEx`
//! takes the player's memory through the caller's allocate callback and hands back the pointer it
//! answered as the handle, `IsActive` and `StreamCount` answer for a player with no source, and
//! `Close` frees through the deallocate callback. Adding a source, `PostInit`, and everything that
//! decodes are declared only: no media stream is measured yet.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceAvPlayer" {
        "sceAvPlayerAddSourceEx" => 6,
        "sceAvPlayerChangeStream" => 6,
        "sceAvPlayerClose" => 6,
        "sceAvPlayerCurrentTime" => 6,
        "sceAvPlayerEnableStream" => 6,
        "sceAvPlayerGetAudioData" => 6,
        "sceAvPlayerGetStreamInfo" => 6,
        "sceAvPlayerGetStreamInfoEx" => 6,
        "sceAvPlayerGetVideoDataEx" => 6,
        "sceAvPlayerInitEx" => 6,
        "sceAvPlayerIsActive" => 6,
        "sceAvPlayerJumpToTime" => 6,
        "sceAvPlayerPause" => 6,
        "sceAvPlayerPostInit" => 6,
        "sceAvPlayerResume" => 6,
        "sceAvPlayerSetAvSyncMode" => 6,
        "sceAvPlayerSetLogCallback" => 6,
        "sceAvPlayerSetLooping" => 6,
        "sceAvPlayerSetTrickSpeed" => 6,
        "sceAvPlayerStart" => 6,
        "sceAvPlayerStop" => 6,
        "sceAvPlayerStreamCount" => 6,
    }
}

const OK: u64 = 0;

/// What the player answers an init block it refuses: one smaller than [`INIT_BLOCK_BYTES`]
/// (`InitEx-size-0x100`) or with no allocate callback (`InitEx-null-alloc-cb`).
const INVALID_ARGUMENT: u64 = 0x806a_0001;

/// The size word at +0 of the init block the measured `InitEx` accepted.
const INIT_BLOCK_BYTES: u64 = 0x230;

/// Where the init block keeps the word every callback receives first (the probe stored it at +0x08
/// and its allocate callback received it as its first argument).
const OBJECT_AT: u64 = 0x08;

/// The allocate callback's slot: the one `InitEx` called, once (`InitEx-first-cb0`, `id 0`).
const ALLOCATE_AT: u64 = 0x10;

/// The deallocate callback's slot, next to it. `Close` made one callback (`Close-first|callbacks
/// 1`); which slot is not rowed, and this one is the pair to the allocator by position.
const DEALLOCATE_AT: u64 = 0x18;

/// The allocate callback's second argument, as `InitEx` passed it (`InitEx-first-cb0|a1|0x20`).
const ALLOCATE_SECOND: u64 = 0x20;

/// The allocate callback's third argument. Not measured - the probe's callback logged it and the
/// check did not row it (re-filed as REQ-20261014T1300Z-rf03 item 4) - so this is an assumption: a
/// page, which reads as a valid size or a valid alignment, whichever the third argument is.
const ALLOCATE_THIRD: u64 = 0x1000;

/// An open player: the word its callbacks receive first and its deallocate callback.
#[derive(Clone, Copy)]
struct Player {
    object: u64,
    deallocate: u64,
}

/// Players opened by `InitEx`, by handle.
static PLAYERS: Mutex<BTreeMap<u64, Player>> = Mutex::new(BTreeMap::new());

fn players() -> std::sync::MutexGuard<'static, BTreeMap<u64, Player>> {
    PLAYERS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn placeholder() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// `sceAvPlayerInitEx(init, handle_out)`: checks the init block, takes the player's memory through
/// its allocate callback, and writes the pointer that answered as the handle - the two are equal
/// (`InitEx-handle|equals-alloc-ptr 1`, `offset-from-alloc 0`). A block smaller than 0x230 and a
/// null allocate slot are refused with [`INVALID_ARGUMENT`], as measured; a larger block, a null
/// pointer and an allocation that fails are not measured and answer the placeholder.
pub(crate) fn init_ex(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (init, handle_out) = (args[0], args[1]);
    if init == 0 || handle_out == 0 {
        return placeholder();
    }
    // SAFETY: the guest's init block and its fields, read through the checked accessor.
    let size = unsafe { guest::read_u64(init) };
    // SAFETY: as above.
    let object = unsafe { guest::read_u64(init + OBJECT_AT) };
    // SAFETY: as above.
    let allocate = unsafe { guest::read_u64(init + ALLOCATE_AT) };
    // SAFETY: as above.
    let deallocate = unsafe { guest::read_u64(init + DEALLOCATE_AT) };
    let (Some(size), Some(object), Some(allocate), Some(deallocate)) =
        (size, object, allocate, deallocate)
    else {
        return placeholder();
    };
    if size < INIT_BLOCK_BYTES || allocate == 0 {
        return INVALID_ARGUMENT;
    }
    if size > INIT_BLOCK_BYTES {
        return placeholder();
    }
    // SAFETY: `allocate` is the callback the guest put in its own init block, in its mapped code.
    let handle = unsafe {
        orbistoun_kernel::thread::call_guest(allocate, [object, ALLOCATE_SECOND, ALLOCATE_THIRD])
    };
    let Some(handle) = handle.filter(|&handle| handle != 0) else {
        return placeholder();
    };
    // SAFETY: the guest's out-parameter, written through the checked accessor.
    if !unsafe { guest::write_u64(handle_out, handle) } {
        return placeholder();
    }
    players().insert(handle, Player { object, deallocate });
    OK
}

/// `sceAvPlayerClose(handle)`: frees the player's memory through its deallocate callback, once, and
/// answers 0 (`Close-first`, `Close-second`). A handle `InitEx` did not open answers the
/// placeholder.
pub(crate) fn close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let handle = args[0];
    let Some(player) = players().remove(&handle) else {
        return placeholder();
    };
    if player.deallocate != 0 {
        // SAFETY: the deallocate callback the guest put in its init block, in its mapped code,
        // handed the pointer its own allocate callback answered.
        let _ = unsafe {
            orbistoun_kernel::thread::call_guest(player.deallocate, [player.object, handle, 0])
        };
    }
    OK
}

/// `sceAvPlayerIsActive(handle)`: a player with no source added answers true - the low byte 1
/// (`IsActive-before-source` and `-after-source`, `0x15aa801` with the handle `0x15aa800`). A handle
/// `InitEx` did not open answers false.
pub(crate) fn is_active(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(players().contains_key(&args[0]))
}

/// `sceAvPlayerStreamCount(handle)`: no source, no streams (`StreamCount-before-source 0`). A handle
/// `InitEx` did not open answers the placeholder.
pub(crate) fn stream_count(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if players().contains_key(&args[0]) {
        0
    } else {
        placeholder()
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The init block the measured `InitEx` accepted: its size word, the object word at +0x08 and a
    /// callback in each slot named here.
    fn block(size: u64, allocate: u64, deallocate: u64) -> Vec<u8> {
        let mut block = vec![0_u8; 0x230];
        block[..8].copy_from_slice(&size.to_le_bytes());
        block[8..16].copy_from_slice(&0x5a5a_0000_u64.to_le_bytes());
        block[0x10..0x18].copy_from_slice(&allocate.to_le_bytes());
        block[0x18..0x20].copy_from_slice(&deallocate.to_le_bytes());
        block
    }

    fn init(block: &[u8], handle: &mut u64) -> u64 {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = block.as_ptr() as u64;
        args[1] = std::ptr::from_mut(handle).expose_provenance() as u64;
        super::init_ex(&args)
    }

    /// A block smaller than 0x230, and one with no allocate callback, are refused with
    /// `0x806a0001` before anything is allocated, and the handle is left as it was.
    #[test]
    fn a_short_block_or_a_missing_allocator_is_refused() {
        let mut handle = 0xcc_u64;
        assert_eq!(init(&block(0x100, 0x1234, 0), &mut handle), 0x806a_0001);
        assert_eq!(init(&block(0x230, 0, 0), &mut handle), 0x806a_0001);
        assert_eq!(handle, 0xcc);
    }

    /// A handle `InitEx` never opened is not active, has no stream count and does not close.
    #[test]
    fn an_unopened_handle_is_inactive() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0xdead_0000;
        assert_eq!(super::is_active(&args), 0);
        assert_ne!(super::stream_count(&args), 0);
        assert_ne!(super::close(&args), 0);
    }
}
