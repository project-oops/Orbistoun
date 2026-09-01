//! System service HLE - the settings and status a title asks the system about.
//!
//! # Why this exists, and what it is not
//!
//! A title asks the system what language it is set to, which button confirms, what the
//! display looks like. None of that is emulation in any interesting sense; it is a
//! question with an answer, and the answer is a *setting of the console*, not a fact about
//! the guest.
//!
//! Which is the whole problem. **We do not know the values**, and the interface hands them
//! back through an out-pointer rather than a return value - so an unimplemented stub does
//! not merely answer wrongly, it answers *nothing*, and the guest reads whatever the stack
//! happened to hold. That is a different and worse failure than a bad return: a bad return
//! is at least the same wrong answer every run (D171).
//!
//! So the out-pointer is always written, and what is written is a stated placeholder
//! rather than a guess dressed as knowledge.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

pub mod console;

guest_module! {
    "libSceSystemService" {
        // Confirmed by hash against a real import (D167).
        "sceSystemServiceParamGetInt" => 2,
        "sceSystemServiceHideSplashScreen" => 0,
        "sceSystemServiceGetStatus" => 1,
    }
}

/// The user service, which is its own library.
///
/// A nested module because `guest_module!` names its declaration `MODULE`, and a crate
/// serving two libraries needs two of them. Kept here rather than in a crate of its own:
/// three functions that answer which user is signed in are not a subsystem.
pub mod user {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceUserService" {
            "sceUserServiceInitialize" => 1,
            "sceUserServiceTerminate" => 0,
            "sceUserServiceGetInitialUser" => 1,
            "sceUserServiceGetUserName" => 3,
            // Declared so a trace names them, and deliberately not implemented. Each writes
            // a number or a structure whose *meaning* is unmeasured - an age band, an
            // accessibility encoding, a list layout - and a person's answer to "what age
            // level" is not the integer a title reads (D346).
            "sceUserServiceGetLoginUserIdList" => 1,
            "sceUserServiceGetAgeLevel" => 2,
            "sceUserServiceGetGamePresets" => 2,
            "sceUserServiceGetEvent" => 1,
            "sceUserServiceGetAccessibilityVibration" => 2,
            "sceUserServiceGetAccessibilityTriggerEffect" => 2,
            "sceUserServiceGetAccessibilityPressAndHoldDelay" => 2,
            "sceUserServiceGetAccessibilityChatTranscription" => 2,
        }
    }
}

/// Loading system modules - `libSceSysmodule`.
///
/// A nested module for the same reason [`user`] is: one crate, more than one library, and each
/// `guest_module!` names its declaration `MODULE`. This one is the call nearly every title makes
/// first - "bring library X in so I can use it" - and getting it wrong strands a title before it
/// reaches anything interesting.
pub mod sysmodule {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceSysmodule" {
            "sceSysmoduleLoadModule" => 1,
            "sceSysmoduleUnloadModule" => 1,
            "sceSysmoduleIsLoaded" => 1,
        }
    }
}

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// What an unknown system parameter answers.
///
/// **Zero, and it is a placeholder rather than a value.** Nothing here knows what any of
/// these parameters mean - they are console settings, and no lawful source describes the
/// identifiers.
///
/// Zero is chosen because it is the answer least likely to send a guest somewhere
/// surprising: a parameter read as an index lands on the first entry, one read as a flag
/// reads as off, one read as a count reads as none. All of those are ordinary states a
/// title must already handle. A non-zero guess would be picking a specific behaviour out
/// of the air and calling it a default.
const UNKNOWN_PARAMETER: u64 = 0;

/// Writes a machine word into guest memory.
///
/// The mapping is identity, so a guest address is a host address (D014).
fn write_word(address: u64, value: u64) -> bool {
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    if at == 0 {
        return false;
    }
    // SAFETY: the guest supplied this destination, which is the same contract the real
    // call has. Written unaligned because the guest's alignment is its own business, and
    // an address it has not mapped faults here exactly as it would have in the guest.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at),
            value as u32,
        );
    }
    true
}

/// `sceSystemServiceParamGetInt(param, out)`.
///
/// # The failure this prevents
///
/// Unimplemented, this wrote nothing at all, and the guest read whatever its stack held at
/// that address. Every other unimplemented call in this project answers *wrongly but
/// consistently*; this one answered differently on every run, because the value depended
/// on what had last been in that stack slot.
///
/// **An out-pointer that is never written is worse than a wrong return value**, and it is a
/// failure mode with no signature - there is no placeholder to recognise in a trace,
/// because nothing was written to recognise.
///
/// Answering `OK` rather than an error is deliberate. A guest that checks the return takes
/// its error path and skips whatever the setting was for; one that does not check reads the
/// value regardless, which is why the value has to be written either way. Writing it *and*
/// reporting success is the combination that leaves a guest in a state it can handle.
fn param_get_int(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[1];
    // A measured answer if the console has one, and the documented placeholder otherwise -
    // counted either way, so a run can say how many questions it did not understand
    // (`console::summarise`). An identifier too large to be one is simply not one, and takes
    // the same path as an identifier nobody has measured.
    let value = u32::try_from(args[0])
        .ok()
        .and_then(console::parameter)
        .map_or(UNKNOWN_PARAMETER, |answer| {
            // Through the bytes rather than by `as`: a parameter is an `int` and a negative
            // measured value must reach the guest as the bit pattern it was measured as.
            u64::from(u32::from_ne_bytes(answer.to_ne_bytes()))
        });
    if !write_word(out, value) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// The user the console is signed in as.
///
/// **A fixed identifier, and that is an assumption rather than a fact.** A real console
/// numbers its users and a title uses the value to key save data, so the specific number
/// matters to anything that has stored something under a different one. One is chosen
/// because it is the first identifier a one-user console would hand out, and because zero
/// is what a caller reads as "nobody" (D274).
const INITIAL_USER: u64 = 1;

/// `sceUserServiceInitialize(params)` - starts the user service.
///
/// Accepts whatever parameters it is given. Nothing here reads them, and refusing the call
/// would stop a title before it could ask anything more interesting.
fn user_service_initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceUserServiceTerminate()`.
fn user_service_terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Longest name this will write, however large a buffer says it is.
///
/// **A ceiling on our own trust, not on the caller's buffer.** The size argument is only
/// believed up to here; anything larger is treated as a value that is not a size at all,
/// which is what a wrong argument position looks like. A name longer than this is truncated
/// rather than refused, because a shortened name is a cosmetic problem and a stack overwrite
/// is not.
const MAX_USER_NAME: usize = 64;

/// `sceUserServiceGetUserName(user, out, size)` - the name a person chose, for a guest.
///
/// # The one call where a console setting reaches a title unencoded
///
/// Everything else this crate answers is a number whose meaning is a measurement. A name is
/// a string: the owner types it in the shell, the guest reads it, and nothing in between has
/// to be guessed. That is the whole argument for a shell holding settings, with a real
/// caller attached (D346).
///
/// # Why the size is checked rather than trusted
///
/// `size` being the third argument is an **assumption**, and a wrong one writes past the end
/// of a caller's buffer - the failure `sceUserServiceGetInitialUser` already carries a
/// warning about (D210, D272). So it is believed only within [`MAX_USER_NAME`]; a value
/// outside that is not a size, and the call is refused rather than acted on. A refusal is
/// recoverable and a smashed stack is not.
fn user_service_get_user_name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(id) = u32::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Some(name) = console::settings().user(id).map(|user| user.name.clone()) else {
        // A title asking about a user this machine does not have is answered as such, not
        // handed the signed-in user's name - which would be a different person.
        return u64::from(GuestError::InvalidArgument.as_raw());
    };

    let out = args[1];
    let Ok(size) = usize::try_from(args[2]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // Room for at least one byte of name and its terminator, and no more than this shim is
    // willing to believe. Both ends refuse rather than guess.
    if !(2..=MAX_USER_NAME).contains(&size) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(at) = usize::try_from(out) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if at == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    // Truncated on a character boundary, not a byte one: cutting a multi-byte character in
    // half hands the guest a string that is not text.
    let room = size - 1;
    let mut keep = room.min(name.len());
    while keep > 0 && !name.is_char_boundary(keep) {
        keep -= 1;
    }

    let destination = std::ptr::with_exposed_provenance_mut::<u8>(at);
    // SAFETY: the guest supplied this destination and its size, which is the same contract
    // the real call has. `keep` is at most `size - 1` and `size` was bounded above, so the
    // write stays inside the buffer the caller described. The mapping is identity, so a guest
    // address is a host address (D014), and an address the guest has not mapped faults here
    // exactly as it would have in the guest. The name is owned locally and cannot overlap it.
    unsafe {
        std::ptr::copy_nonoverlapping(name.as_ptr(), destination, keep);
    }
    // SAFETY: `keep < size` and the buffer is `size` bytes, so one past the copied text is
    // still within it - which is precisely where the terminator belongs.
    let terminator = unsafe { destination.add(keep) };
    // SAFETY: the same byte, shown above to be inside the caller's buffer.
    unsafe {
        terminator.write(0);
    }
    OK
}

/// `sceUserServiceGetInitialUser(out)` - the user a title should start as.
fn user_service_get_initial_user(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(at) = usize::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // Four bytes: a user identifier is an `int`, and writing a whole word through an int
    // pointer takes the caller's next variable with it (D210, D272).
    // SAFETY: a guest-supplied `int *` under the identity mapping (D014).
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at),
            // The user somebody is actually signed in as, rather than a fixed one. A machine
            // with a deleted signed-in user answers the placeholder, which is the honest
            // answer to "who is signed in" when nobody is (D346).
            console::settings()
                .current()
                .map_or(INITIAL_USER as u32, |user| user.id),
        );
    }
    OK
}

/// `sceSysmoduleLoadModule(id)` - bring a system module in so its functions can be called.
///
/// **Always succeeds, and that is the honest answer here.** On a console this pages a library into
/// the process; in orbistoun every library a title imports is already resolved by the loader - its
/// functions are stubs or implementations in this process before the guest runs - so the module a
/// guest asks to load is one it can already call. Answering `0` states exactly that. Left to the
/// stub-everything resolver it returned `0x7fff_0001`, a positive placeholder a caller reads as a
/// loaded-module handle it never asked for, and a title that stored and dereferenced it faulted a
/// few calls later on something with no relation to the load (D125).
fn sysmodule_load_module(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceSysmoduleUnloadModule(id)` - the counterpart, and a no-op here.
///
/// Nothing was paged in, so nothing is paged out; the functions stay resolved either way. Success,
/// because a title that unloads a module and checks the result should not be told it failed.
fn sysmodule_unload_module(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `SCE_SYSMODULE_ERROR_UNLOADED` - answered for an identifier that names no loaded module.
///
/// **Measured.** obSCEne's `060-module/sysmodule-query` called `sceSysmoduleIsLoaded(0)` on hardware
/// and was answered `0x805a1000` - the sysmodule error base `0x805a_0000` with its unloaded code, not
/// "loaded". A guest checking against `SCE_SYSMODULE_ERROR_*` never matches a kernel-base code or a
/// placeholder.
const SYSMODULE_UNLOADED: u64 = 0x805a_1000;

/// `sceSysmoduleIsLoaded(id)` - whether a module is loaded.
///
/// Every real module a title names is resolved by the loader, so a valid identifier answers `0`
/// (loaded), the same fact [`sysmodule_load_module`] states from the other side. But **identifier 0 is
/// not a loadable module** - hardware answers the unloaded error for it, not success - so that case is
/// answered with the measured [`SYSMODULE_UNLOADED`] rather than reporting a non-module as present.
fn sysmodule_is_loaded(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return SYSMODULE_UNLOADED;
    }
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// Names rather than hashes: the hash is derived from the name, so a table written in
/// hashes could not be read by a person or checked against the declarations above.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceUserServiceInitialize", user_service_initialize),
        ("sceUserServiceTerminate", user_service_terminate),
        (
            "sceUserServiceGetInitialUser",
            user_service_get_initial_user,
        ),
        ("sceUserServiceGetUserName", user_service_get_user_name),
        ("sceSystemServiceParamGetInt", param_get_int),
        ("sceSysmoduleLoadModule", sysmodule_load_module),
        ("sceSysmoduleUnloadModule", sysmodule_unload_module),
        ("sceSysmoduleIsLoaded", sysmodule_is_loaded),
    ]
}

#[cfg(test)]
mod tests {
    use super::{MAX_USER_NAME, UNKNOWN_PARAMETER, implementations, param_get_int};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Arguments for a name request against a real buffer.
    fn name_call(user: u64, buffer: &mut [u8], size: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut args = [0; GUEST_ARG_REGISTERS];
        args[0] = user;
        args[1] = buffer.as_mut_ptr() as u64;
        args[2] = size;
        args
    }

    /// **A size this shim does not believe is refused, not acted on.**
    ///
    /// The whole safety story for this call. That the size is the third argument is an
    /// assumption, and a wrong argument position would write past a caller's buffer - so a
    /// claimed size outside what a name could plausibly need is treated as a value that is
    /// not a size at all. Asserted on the refusal, because the failure is the point (D346).
    #[test]
    fn a_size_this_shim_does_not_believe_is_refused() {
        let mut buffer = [0xAA_u8; 8];

        for claimed in [0, 1, MAX_USER_NAME as u64 + 1, u64::MAX] {
            let refused = super::user_service_get_user_name(&name_call(1, &mut buffer, claimed));
            assert_ne!(
                refused,
                super::OK,
                "a claimed size of {claimed} was acted on"
            );
        }
        assert!(
            buffer.iter().all(|byte| *byte == 0xAA),
            "and nothing was written to the buffer while refusing"
        );
    }

    /// A user this machine does not have is refused rather than answered with somebody else.
    #[test]
    fn an_unknown_user_is_not_answered_with_the_signed_in_ones_name() {
        let mut buffer = [0_u8; 32];
        let refused = super::user_service_get_user_name(&name_call(9999, &mut buffer, 32));

        assert_ne!(refused, super::OK);
        assert_eq!(buffer[0], 0, "nothing was written");
    }

    /// **The name arrives NUL-terminated and inside the buffer it was given.**
    ///
    /// Uses a buffer smaller than the default name so the truncating path is the one under
    /// test - the untruncated case cannot show that the bound is respected.
    #[test]
    fn a_name_is_written_terminated_and_never_past_the_size_it_was_given() {
        // The shipped default user is "player", six bytes. Four bytes of room means three
        // characters and a terminator.
        let mut buffer = [0xAA_u8; 16];
        let answered = super::user_service_get_user_name(&name_call(1, &mut buffer, 4));

        assert_eq!(answered, super::OK);
        assert_eq!(&buffer[..3], b"pla");
        assert_eq!(buffer[3], 0, "terminated inside the claimed size");
        assert_eq!(
            buffer[4], 0xAA,
            "and nothing beyond the claimed size was touched"
        );
    }

    #[test]
    fn every_implementation_is_also_declared() {
        // An implementation nobody declared can never be reached: resolution goes through
        // the declared symbol list, so the two drifting apart would leave code that looks
        // written and never runs.
        // **Both modules.** This crate serves two libraries, and checking only the first
        // would report the user service as undeclared while it worked.
        let declared: Vec<&str> = super::MODULE
            .imports
            .iter()
            .chain(super::user::MODULE.imports.iter())
            .chain(super::sysmodule::MODULE.imports.iter())
            .map(|i| i.name)
            .collect();
        for (name, _) in implementations() {
            assert!(
                declared.contains(name),
                "{name} is implemented but not declared in guest_module!"
            );
        }
    }

    #[test]
    fn the_destination_is_always_written() {
        // The whole point. Unwritten, the guest reads whatever its stack held - which is
        // a different answer every run and has no signature to recognise in a trace.
        let mut value: u32 = 0xDEAD_BEEF;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = std::ptr::addr_of_mut!(value) as usize as u64;

        assert_eq!(param_get_int(&args), 0, "reports success");
        assert_eq!(
            u64::from(value),
            UNKNOWN_PARAMETER,
            "and the destination no longer holds what was there before"
        );
    }

    #[test]
    fn a_request_with_nowhere_to_answer_is_refused() {
        // Writing to address zero is the alternative, and it faults inside the emulator
        // rather than naming the guest's mistake.
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_ne!(param_get_int(&args), 0);
    }

    #[test]
    fn only_four_bytes_are_written() {
        // The interface answers an `int`. Writing eight would corrupt whatever the guest
        // put next to it - which on a stack is usually another local.
        let mut pair: [u32; 2] = [0xAAAA_AAAA, 0xBBBB_BBBB];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = std::ptr::addr_of_mut!(pair[0]) as usize as u64;

        assert_eq!(param_get_int(&args), 0);
        assert_eq!(u64::from(pair[0]), UNKNOWN_PARAMETER);
        assert_eq!(pair[1], 0xBBBB_BBBB, "the neighbour is untouched");
    }
}
