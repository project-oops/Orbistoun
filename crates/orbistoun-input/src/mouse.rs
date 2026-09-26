//! `libSceMouse` - mouse input.
//!
//! obSCEne's `101-input-ext/mouse-read` typed and called each function with no mouse attached,
//! giving `sceMouseInit()`, `sceMouseOpen(user, _, _, _)`, `sceMouseRead(handle, into, count)`
//! and `sceMouseClose(handle)`; a trailing argument nothing passes is not ruled out. Open
//! answered a handle and read answered `0x0` having written zero bytes, so an empty read
//! reports no queued events. That is all that is modelled: a queue that is always empty rather
//! than a call that refuses.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn, Handle, HandleAllocator};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceMouse" {
        "sceMouseClose" => 1,
        "sceMouseInit" => 0,
        "sceMouseOpen" => 4,
        "sceMouseRead" => 3,
    }
}

/// Successful return, as a guest reads it.
const OK: u64 = 0;

/// Handles this shim has issued, in its own allocator so a mouse handle passed to a pad call
/// is caught.
static HANDLES: std::sync::Mutex<HandleAllocator> = std::sync::Mutex::new(HandleAllocator::new());

/// The highest handle issued, for recognising one back.
static ISSUED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Records that a handle was issued, so [`ours`] recognises it later.
///
/// `fetch_max`, because two threads opening at once can finish out of order, and a plain
/// store would move the mark below a live handle.
fn remember(handle: Handle) {
    ISSUED.fetch_max(handle.as_raw(), std::sync::atomic::Ordering::Relaxed);
}
/// Whether a raw value is a handle this shim gave out.
fn ours(raw: u64) -> Option<Handle> {
    let raw = u32::try_from(raw).ok()?;
    let handle = Handle::from_raw(raw)?;
    (raw <= ISSUED.load(std::sync::atomic::Ordering::Relaxed)).then_some(handle)
}

/// `sceMouseInit()` - start the mouse service.
fn mouse_init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceMouseOpen(user, type, index, param)` - open a mouse and answer a handle.
///
/// Opens with nothing plugged in, as the hardware does: a guest finds the queue empty.
fn mouse_open(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mut allocator = HANDLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(handle) = allocator.alloc() else {
        return u64::from(GuestError::NoMemory.as_raw());
    };
    remember(handle);
    u64::from(handle.as_raw())
}

/// `sceMouseRead(handle, into, count)` - take queued events.
///
/// Answers zero events and writes nothing to `into`, as measured; a guest keeping a previous
/// sample there keeps it.
fn mouse_read(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // No events and no bytes, so a null destination is never read.
    OK
}

/// `sceMouseClose(handle)` - close a mouse.
///
/// The number is not recycled: reuse turns a stale handle into a read of another device.
fn mouse_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    OK
}

/// Implementations this module provides, by symbol name.
///
/// No failure from `libSceMouse` has been measured, so a bad handle answers
/// `GuestError::InvalidHandle` rather than an invented vendor error code.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceMouseInit", mouse_init),
        ("sceMouseOpen", mouse_open),
        ("sceMouseRead", mouse_read),
        ("sceMouseClose", mouse_close),
    ]
}

#[cfg(test)]
mod tests {

    /// Recording handles out of order never lowers the mark.
    #[test]
    fn remembering_a_handle_out_of_order_never_lowers_the_mark() {
        let high = orbistoun_core::Handle::from_raw(4096).expect("a handle");
        let low = orbistoun_core::Handle::from_raw(4095).expect("a handle");
        super::remember(high);
        super::remember(low);
        assert!(
            super::ours(4096).is_some(),
            "a live handle stayed recognised after a lower one was recorded"
        );
    }
    use orbistoun_core::GUEST_ARG_REGISTERS;

    fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
        let (_, function) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not served, so no guest can reach it"));
        function(&args)
    }

    /// A mouse opens with nothing plugged in, and reading it answers zero events and no bytes.
    #[test]
    fn an_empty_queue_reads_as_zero_events_rather_than_a_failure() {
        let handle = call("sceMouseOpen", [0, 0, 0, 0, 0, 0]);
        assert!((handle as i64) > 0, "a handle, even with no mouse");

        let mut into = [0xAA_u8; 64];
        assert_eq!(
            call(
                "sceMouseRead",
                [
                    handle,
                    into.as_mut_ptr().expose_provenance() as u64,
                    8,
                    0,
                    0,
                    0
                ]
            ),
            0,
            "zero events read"
        );
        assert!(
            into.iter().all(|b| *b == 0xAA),
            concat!(
                "and not one byte written - the console's extent was zero, so clearing ",
                "the caller's buffer would be doing something it was measured not doing"
            )
        );
        assert_eq!(call("sceMouseClose", [handle, 0, 0, 0, 0, 0]), 0);
    }

    /// A handle nobody opened is refused everywhere it is taken.
    #[test]
    fn a_handle_nobody_opened_is_refused() {
        const NOBODYS: u64 = 0x7FFF;
        assert_ne!(call("sceMouseRead", [NOBODYS, 0, 8, 0, 0, 0]), 0);
        assert_ne!(call("sceMouseClose", [NOBODYS, 0, 0, 0, 0, 0]), 0);
    }

    /// Every name served is declared, or a guest can never reach it.
    #[test]
    fn every_implementation_is_also_declared() {
        for (name, _) in super::implementations() {
            assert!(
                MODULE.imports.iter().any(|i| i.name == *name),
                "{name} is served but not declared"
            );
        }
    }

    use super::MODULE;
}
