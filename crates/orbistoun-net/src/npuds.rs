//! `libSceNpUniversalDataSystem` - the platform's event and statistics reporting.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceNpUniversalDataSystemInitialize` and
//! `sceNpUniversalDataSystemCreateContext`, `sceNpUniversalDataSystemCreateHandle` and
//! `sceNpUniversalDataSystemCreateEventPropertyObject`, `sceNpUniversalDataSystemCreateEvent`,
//! `sceNpUniversalDataSystemRegisterContext`, `sceNpUniversalDataSystemPostEvent`,
//! `sceNpUniversalDataSystemDestroyEvent` and `sceNpUniversalDataSystemDestroyEventPropertyObject`
//! are implemented, as measured.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceNpUniversalDataSystem" {
        // (param)
        // (&ctx, user, 0, 0)
        "sceNpUniversalDataSystemCreateContext" => 4,
        "sceNpUniversalDataSystemInitialize" => 1,
        // (&handle)
        "sceNpUniversalDataSystemCreateHandle" => 1,
        // (&object)
        "sceNpUniversalDataSystemCreateEventPropertyObject" => 1,
        // (name, prop, &event, 0)
        "sceNpUniversalDataSystemCreateEvent" => 4,
        // (ctx, handle, 0)
        "sceNpUniversalDataSystemRegisterContext" => 3,
        // (ctx, handle, event, 0)
        "sceNpUniversalDataSystemPostEvent" => 4,
        // (event)
        "sceNpUniversalDataSystemDestroyEvent" => 1,
        // (object)
        "sceNpUniversalDataSystemDestroyEventPropertyObject" => 1,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpUniversalDataSystemInitialize", initialize),
        ("sceNpUniversalDataSystemCreateContext", create_context),
        ("sceNpUniversalDataSystemCreateHandle", create_handle),
        (
            "sceNpUniversalDataSystemCreateEventPropertyObject",
            create_event_property_object,
        ),
        ("sceNpUniversalDataSystemCreateEvent", create_event),
        ("sceNpUniversalDataSystemRegisterContext", register_context),
        ("sceNpUniversalDataSystemPostEvent", post_event),
        ("sceNpUniversalDataSystemDestroyEvent", destroy_event),
        (
            "sceNpUniversalDataSystemDestroyEventPropertyObject",
            destroy_event_property_object,
        ),
    ]
}

/// `sceNpUniversalDataSystemInitialize(param)`: with `param` u64 0x10 (its size), u64 0x40000 (a
/// memory size), a pointer and u64 0x10, as PPSA02664's plugin passes it, answers 0 and leaves the
/// block as it was (REQ-np02, two sweeps: `20261011-083754-eboot.obs.log` 2933-2960 and
/// `20261011-1791715554-eboot.obs.log` 2987-3014, after `sceSysmoduleLoadModule(0x0105)`). A null
/// block is unmeasured and answers the placeholder.
fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    0
}

/// The context a fresh process's first `sceNpUniversalDataSystemCreateContext` gave (REQ-np03,
/// `uds-create-context-call1`); each later one is the next.
static NEXT_CONTEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0004_00d3);

/// `sceNpUniversalDataSystemCreateContext(&ctx, user, 0, 0)`: writes the next context number to
/// the low 32 bits of `ctx`, leaving the high half as it was, and answers 0 - `0x400d3` then
/// `0x400d4` on the console (REQ-np03, `000-boot/np-appcontent-startup`, `report-1791758178.txt`
/// 223-266). A null `ctx` is unmeasured and answers the placeholder.
fn create_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let number = NEXT_CONTEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the guest's context slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u32(args[0], number) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The handle a fresh process's first `sceNpUniversalDataSystemCreateHandle` gave (REQ-pu01,
/// `000-boot/pad-ime-uds`, `report-1791801495.txt` 417-421); each later one is the next.
static NEXT_HANDLE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0002_0176);

/// `sceNpUniversalDataSystemCreateHandle(&handle)`: writes the next handle number to the low 32
/// bits of `handle`, leaving the high half as it was, and answers 0 - `0x20176` then `0x20177` on
/// the console, after a context was made. A null `handle` is unmeasured and answers the
/// placeholder.
fn create_handle(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let number = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the guest's handle slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u32(args[0], number) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// The answer for a null object slot (REQ-rf02).
const INVALID_ARGUMENT: u64 = 0x8055_3102;

/// Bytes behind an event property object or an event. Assumed: the console's object is its own,
/// and nothing a title was seen to read from either directly; zeroed, as a fresh object holds
/// nothing.
const OBJECT_BYTES: usize = 0x100;

/// Writes a fresh object's address, all eight bytes, through `slot`, and answers 0, or
/// [`INVALID_ARGUMENT`] for a null slot: what both the property object and the event calls do.
fn new_object(slot: u64) -> u64 {
    if slot == 0 {
        return INVALID_ARGUMENT;
    }
    // Guest code runs in this address space, so a host block is a guest object. Leaked: the
    // destroy calls are not served, and the console's object outlives the call that made it.
    let object = Box::leak(vec![0_u8; OBJECT_BYTES].into_boxed_slice());
    let address = object.as_mut_ptr() as u64;
    // SAFETY: the guest's object slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u64(slot, address) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceNpUniversalDataSystemCreateEventPropertyObject(&object)`: writes a new object's address,
/// all eight bytes, and answers 0; a null slot answers `0x80553102` (REQ-20261013T1930Z-rf02,
/// `000-boot/ux01-calls`, after `Initialize`, `CreateContext` and `CreateHandle`). PPSA02664 and
/// PPSA03416 call it once a frame; the five registers after the slot are left over from the
/// caller, not arguments.
///
/// Before `Initialize` the console answers `0x80553117` (REQ-pd02); orbistoun does not track
/// initialisation, and every title seen initialises first.
fn create_event_property_object(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    new_object(args[0])
}

/// `sceNpUniversalDataSystemCreateEvent(name, prop, &event, 0)`: writes a new event's address to
/// the first eight bytes of `event`, leaving the rest of the block as it was, and answers 0, with a
/// property object or a null one; a null `event` answers `0x80553102`. A second call gives another
/// event (REQ-20261014T0430Z-ue01, `000-boot/uds-create-event`, `report-20261014-batch4.log`
/// 2855-2863). PPSA02664 calls it once a frame.
fn create_event(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    new_object(args[2])
}

/// What `sceNpUniversalDataSystemRegisterContext` answered for a context and handle just made,
/// both times it was called (REQ-ue01, `000-boot/uds-create-event` 2851-2852).
const REGISTER_CONTEXT_ANSWER: u64 = 0x8055_3104;

/// `sceNpUniversalDataSystemRegisterContext(ctx, handle, 0)`: answers `0x80553104`, as the console
/// did for the context and handle it had just made, a second time too. Every commercial title in
/// the corpus calls it.
const fn register_context(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    REGISTER_CONTEXT_ANSWER
}

/// `sceNpUniversalDataSystemPostEvent(ctx, handle, event, 0)`: answers `0x80553104` for an event
/// `CreateEvent` made, as the console did - the answer `RegisterContext` gives on the same console
/// (REQ-20261014T1110Z-rf05, `000-boot/uds-create-event`, `report-20261014-batch6.log` 2181).
/// PPSA02664 posts an event every frame.
const fn post_event(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    REGISTER_CONTEXT_ANSWER
}

/// `sceNpUniversalDataSystemDestroyEvent(event)`: answers 0 (REQ-rf05, batch6 2182). The event's
/// block is not freed: [`new_object`] leaked it, as the console's object is its own.
const fn destroy_event(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceNpUniversalDataSystemDestroyEventPropertyObject(object)`: answers 0 (REQ-rf05, batch6
/// 2183), its block kept as [`destroy_event`] keeps an event's.
const fn destroy_event_property_object(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Handles count up from `0x20176` in the low four bytes, the rest of the slot as it was
    /// (REQ-pu01).
    #[test]
    fn handles_count_up_from_the_measured_first() {
        let mut slot = [0xcc_u8; 8];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = slot.as_mut_ptr() as u64;
        assert_eq!(super::create_handle(&args), 0);
        assert_eq!(slot, [0x76, 0x01, 0x02, 0x00, 0xcc, 0xcc, 0xcc, 0xcc]);
        assert_eq!(super::create_handle(&args), 0);
        assert_eq!(slot[..4], [0x77, 0x01, 0x02, 0x00]);
    }

    /// A new object's address fills all eight bytes of the slot, and a null slot is refused
    /// (REQ-rf02).
    #[test]
    fn event_property_objects_are_written_through_the_slot() {
        let mut slot = [0xcc_u8; 16];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = slot.as_mut_ptr() as u64;
        assert_eq!(super::create_event_property_object(&args), 0);
        let object = u64::from_le_bytes(slot[..8].try_into().unwrap());
        assert_ne!(object, 0);
        assert_eq!(slot[8..], [0xcc; 8]);
        assert_eq!(super::create_event_property_object(&args), 0);
        assert_ne!(u64::from_le_bytes(slot[..8].try_into().unwrap()), object);
        args[0] = 0;
        assert_eq!(super::create_event_property_object(&args), 0x8055_3102);
    }

    /// `CreateEvent(name, prop, &event, 0)` writes a new event's address to the first eight bytes
    /// of `event`, with or without a property object, and refuses a null slot (REQ-ue01). A repeat
    /// gives another event.
    #[test]
    fn events_are_written_through_the_third_argument() {
        let mut event = [0xcc_u8; 0x20];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = c"activityTerminate".as_ptr() as u64;
        args[1] = 0x1000;
        args[2] = event.as_mut_ptr() as u64;
        assert_eq!(super::create_event(&args), 0);
        let first = u64::from_le_bytes(event[..8].try_into().unwrap());
        assert_ne!(first, 0);
        assert_eq!(
            event[8..],
            [0xcc; 0x18],
            "the rest of the block is untouched"
        );
        args[1] = 0;
        assert_eq!(
            super::create_event(&args),
            0,
            "a null property object is accepted"
        );
        assert_ne!(u64::from_le_bytes(event[..8].try_into().unwrap()), first);
        args[2] = 0;
        assert_eq!(super::create_event(&args), 0x8055_3102);
    }

    /// An event made by `CreateEvent` is posted with `0x80553104`, as `RegisterContext` answers on
    /// the same console, and destroyed with 0, as is its property object (REQ-rf05).
    #[test]
    fn events_are_posted_and_destroyed_as_measured() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x4_007b;
        args[1] = 0x2_0098;
        args[2] = 0x1000;
        assert_eq!(super::post_event(&args), 0x8055_3104);
        args[0] = 0x1000;
        assert_eq!(super::destroy_event(&args), 0);
        assert_eq!(super::destroy_event_property_object(&args), 0);
    }

    /// `RegisterContext(ctx, handle, 0)` answers `0x80553104` on the console, a second time too
    /// (REQ-ue01).
    #[test]
    fn registering_a_context_answers_as_measured() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x4_007b;
        args[1] = 0x2_0098;
        assert_eq!(super::register_context(&args), 0x8055_3104);
        assert_eq!(super::register_context(&args), 0x8055_3104);
    }
}
