//! The parts of the C++ runtime that can be answered without an unwinder.
//!
//! `operator new` in its aligned and `nothrow` forms is the ordinary heap under a C++ name.
//! `std::terminate`, a pure virtual call and the runtime's throw helpers never return, so they
//! end the run and name which one it was: their callers are compiled on the `[[noreturn]]`
//! promise, and returning would continue from a point the compiler proved unreachable. A guest
//! that would have caught the exception is reported as stopping at the throw (D473).
//! `__cxa_throw`, `__cxa_begin_catch`, the personality routine and the type-info objects need a
//! stack unwinder and are not implemented here.
//!
//! Reference: the Itanium C++ ABI (<https://itanium-cxx-abi.github.io/cxx-abi/abi.html>) for
//! the `__cxa_*` names and `_Unwind_Resume`, and ISO/IEC 14882 for `std::terminate` and the
//! allocation functions.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use crate::{c_len, ptr};

/// Ends the run, naming the C++ operation that could not be completed.
///
/// One place, so every never-returning entry point reports the same way.
fn stop_for(what: &str, detail: Option<String>) -> u64 {
    let line = detail.map_or_else(
        || format!("the guest reached {what}, which does not return"),
        |text| format!("the guest reached {what}: {text}"),
    );
    tracing::warn!("{line}");
    orbistoun_core::klog::note(&line);
    orbistoun_core::stop(orbistoun_core::StopReason::Aborted, 0)
}

/// The message a throw helper was given, when it takes one.
fn message(address: u64) -> Option<String> {
    if address == 0 {
        return None;
    }
    // SAFETY: a guest-supplied string under the identity mapping, bounded.
    let len = unsafe { c_len(address) };
    // SAFETY: `c_len` established `len` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(address).cast_const(), len) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// `operator new(size, nothrow_t)` - allocation that answers null rather than throwing.
///
/// Reference: ISO/IEC 14882 [new.delete.single]. The one `operator new` whose caller checks
/// for null.
fn operator_new_nothrow(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    crate::allocate(usize::try_from(args[0]).unwrap_or(usize::MAX), 0)
}

/// `operator new(size, align_val_t)` - allocation with an explicit alignment.
///
/// Reference: ISO/IEC 14882 [new.delete.single], as extended by over-aligned allocation.
fn operator_new_aligned(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let size = usize::try_from(args[0]).unwrap_or(usize::MAX);
    let align = usize::try_from(args[1]).unwrap_or(0);
    crate::allocate(size, align)
}

/// `operator delete(pointer, align_val_t)` - the counterpart to the aligned `new`.
///
/// The alignment is ignored, as a sized delete's size is: the heap records what it handed out.
fn operator_delete_aligned(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    crate::free(args)
}

/// `std::get_new_handler()` - the handler `operator new` calls when it cannot allocate.
///
/// Reference: ISO/IEC 14882 [alloc.errors]. Null is the correct answer: the initial handler
/// is null, and `set_new_handler` is not imported.
fn get_new_handler(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `std::terminate()` - ends the program.
///
/// Reference: ISO/IEC 14882 [exception.terminate].
fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stop_for("std::terminate", None)
}

/// `__cxa_pure_virtual()` - a pure virtual function was called.
///
/// Reference: Itanium C++ ABI 3.2.6. It never returns; the object being called through was
/// part-constructed or part-destroyed, so there is nothing to continue with.
fn pure_virtual(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stop_for("a pure virtual call", None)
}

/// `_Unwind_Resume(exception)` - continues an unwind already in progress.
///
/// Reference: Itanium C++ ABI 2.5.3. Reached from a landing pad that finished its cleanup;
/// returning would resume the guest inside a frame already torn down.
fn unwind_resume(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stop_for("_Unwind_Resume with no unwinder to resume into", None)
}

/// The runtime's throw helpers that carry a message.
fn throw_with_message(args: &[u64; GUEST_ARG_REGISTERS], what: &str) -> u64 {
    stop_for(what, message(args[0]))
}

/// `std::_Xlength_error(what)` - throws `std::length_error`.
fn xlength_error(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    throw_with_message(args, "a throw of std::length_error")
}

/// `std::_Xout_of_range(what)` - throws `std::out_of_range`.
fn xout_of_range(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    throw_with_message(args, "a throw of std::out_of_range")
}

/// `std::_Xinvalid_argument(what)` - throws `std::invalid_argument`.
fn xinvalid_argument(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    throw_with_message(args, "a throw of std::invalid_argument")
}

/// `std::_Xbad_alloc()` - throws `std::bad_alloc`.
fn xbad_alloc(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stop_for("a throw of std::bad_alloc", None)
}

/// `std::_Xbad_function_call()` - throws `std::bad_function_call`.
fn xbad_function_call(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stop_for("a throw of std::bad_function_call", None)
}

/// `std::_Throw_C_error(code)` - throws the `std::system_error` for a C error number.
///
/// The raw number is reported, not translated: the mapping to a condition is the runtime's
/// own.
fn throw_c_error(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stop_for(
        "a throw of std::system_error",
        Some(format!("C error {}", args[0] as i32)),
    )
}

/// `std::_Throw_Cpp_error(code)` - throws the `std::system_error` for a C++ error condition.
fn throw_cpp_error(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    stop_for(
        "a throw of std::system_error",
        Some(format!("C++ error {}", args[0] as i32)),
    )
}

/// Everything here, by symbol name.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("_ZnwmRKSt9nothrow_t", operator_new_nothrow),
        ("_ZnwmSt11align_val_t", operator_new_aligned),
        ("_ZdlPvSt11align_val_t", operator_delete_aligned),
        ("_ZSt15get_new_handlerv", get_new_handler),
        ("_ZSt9terminatev", terminate),
        ("__cxa_pure_virtual", pure_virtual),
        ("_Unwind_Resume", unwind_resume),
        ("_ZSt14_Xlength_errorPKc", xlength_error),
        ("_ZSt14_Xout_of_rangePKc", xout_of_range),
        ("_ZSt18_Xinvalid_argumentPKc", xinvalid_argument),
        ("_ZSt11_Xbad_allocv", xbad_alloc),
        ("_ZSt19_Xbad_function_callv", xbad_function_call),
        ("_ZSt14_Throw_C_errori", throw_c_error),
        ("_ZSt16_Throw_Cpp_errori", throw_cpp_error),
    ]
}
