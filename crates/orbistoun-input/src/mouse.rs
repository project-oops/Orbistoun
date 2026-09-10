//! `libSceMouse` - mouse input.
//!
//! **4 names, all served.** They come from PPSA02664's own import table (4).
//!
//! # Arities, measured
//!
//! They were all `6` - the trampoline's full capture, which is not a claim about how many
//! arguments a function takes (D504). obSCEne types each one and calls it, and the console
//! answered, so the first N arguments are these: `sceMouseInit()`, `sceMouseOpen(user, _, _,
//! _)`, `sceMouseRead(handle, into, count)`, `sceMouseClose(handle)`. That a trailing argument
//! nothing passes exists is still not ruled out - a call that works cannot see one.
//!
//! # What a read answers when nothing is plugged in
//!
//! **Nothing, and it says so with success.** obSCEne's `101-input-ext/mouse-read` opened a
//! mouse on a console with none attached: `sceMouseOpen` answered a handle, `sceMouseRead`
//! answered `0x0`, and the extent it wrote was **zero bytes**. So an empty read is not an
//! error on this platform - it is a report that no events are queued, which is exactly the
//! state orbistoun is in permanently (D673).
//!
//! That is the whole of what is modelled here. There is no mouse behind it and nothing
//! pretends there is; what the shim provides is the *shape* a guest polling for events sees,
//! which is a queue that is always empty rather than a call that refuses.

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

/// Handles this shim has issued.
///
/// Its own allocator rather than the pad's, for the reason each subsystem has one: a mouse
/// handle and a pad handle sharing a number space would hide the bug where a guest passes one
/// to the other.
static HANDLES: std::sync::Mutex<HandleAllocator> = std::sync::Mutex::new(HandleAllocator::new());

/// The highest handle issued, for recognising one back.
static ISSUED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Records that a handle was issued, so [`ours`] recognises it later.
///
/// **`fetch_max`, not `store`.** A plain store lets the mark move *backwards*: two guest
/// threads opening at once can have the second allocate the higher number and the first write
/// its lower one afterwards, leaving a live handle above the mark and refused by every call
/// that takes it. The allocator is behind a lock and the mark was not, so the two could
/// disagree - which is a race a sequential test can never show, and which turned up as an
/// intermittent `cargo test --workspace` failure inside the gate that passed on every direct
/// re-run (D674).
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
/// **Opens even with nothing plugged in**, which is what a console does: obSCEne asked on a
/// machine with no mouse attached and got a handle back (`0xc70700`). Refusing would be the
/// stricter-looking answer and the wrong one - a guest is told there is a device and finds the
/// queue empty, which is the true state of affairs.
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
/// **Answers zero and writes nothing**, which is measured rather than convenient: the console
/// answered `0x0` with an extent of zero bytes. Zero is the number of events read, so a guest
/// polling gets "nothing happened" and loops, rather than an error it has to interpret.
///
/// Nothing is written to `into` at all - not zeroed, not touched. A shim that cleared the
/// buffer would be doing something the console was measured not doing, and a guest keeping a
/// previous sample there would find it wiped.
fn mouse_read(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // No events, and no bytes. Nothing measured what a console answers for a null destination,
    // so nothing here treats one specially - with no events to write, it never reads it.
    OK
}

/// `sceMouseClose(handle)` - close a mouse.
///
/// The number is not handed back to the allocator: reuse is what turns a stale handle into a
/// silent read of another device.
fn mouse_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if ours(args[0]).is_none() {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    OK
}

/// Implementations this module provides, by symbol name.
///
/// **The error codes are placeholders and that is deliberate.** Every other subsystem here
/// carries its own measured error base - the pad's `0x8092_0000`, audio's `0x8026_0000` -
/// because obSCEne provoked a failure and read one back. Nothing has provoked one from
/// `libSceMouse`, so a bad handle earns `GuestError::InvalidHandle` rather than a
/// `0x80xx_xxxx` invented to look like the others (D673).
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

    /// **Recording a handle never lowers the mark.**
    ///
    /// The bug this pins is a race and a sequential test cannot show it: two guest threads
    /// opening at once can have the second allocate the higher number and the first store its
    /// lower one afterwards, leaving a live handle above the mark and refused by every call
    /// that takes it. `ISSUED.store` did exactly that.
    ///
    /// So the *property* is tested instead of the interleaving - remember out of order, and the
    /// mark must still be the highest. Deterministic, and it fails on the old code (D674).
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

    /// **A mouse opens with nothing plugged in, and reading it answers zero events.**
    ///
    /// Measured on a console with no mouse attached: `sceMouseOpen` answered a handle and
    /// `sceMouseRead` answered `0x0` having written **no bytes**. An empty read is a report,
    /// not an error, which is what lets a guest poll in a loop.
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

    /// A handle nobody opened is refused, everywhere it is taken.
    ///
    /// The negative case: a shim answering zero events for any handle would pass the test
    /// above while telling a guest a device it never opened is sitting there quiet.
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
