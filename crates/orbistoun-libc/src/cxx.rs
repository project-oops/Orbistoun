//! The parts of the C++ runtime that can be answered without an unwinder.
//!
//! # The dividing line
//!
//! A guest's C++ runtime asks this layer for three quite different things, and only two of
//! them can be given honestly:
//!
//! - **Allocation.** `operator new` in its aligned and `nothrow` forms is the ordinary heap
//!   wearing a C++ name, and is implemented here as such.
//! - **Ending the program.** `std::terminate`, a pure virtual call, and the runtime's family
//!   of throw helpers all **never return**. They can therefore be answered exactly, by ending
//!   the run and saying which one it was - see below.
//! - **Exceptions themselves** - `__cxa_throw`, `__cxa_begin_catch`, the personality routine,
//!   the vtables and type-info objects. These need a stack unwinder, which this project does
//!   not have, and they are deliberately left unimplemented (see the worklog for the list).
//!
//! # Why a throw is a stop rather than a return
//!
//! The runtime's `_Xlength_error`, `_Xout_of_range`, `_Xbad_alloc` and their siblings exist to
//! throw a specific exception. Their C++ declarations are `[[noreturn]]`, and the caller's code
//! is generated on that promise: the instruction after the call is unreachable, and the state
//! the caller was in is not one it expects to continue from.
//!
//! So there are two possible answers and only one of them is honest. **Returning** lets the
//! guest carry on from a point its own compiler proved unreachable, with whatever half-built
//! object provoked the throw - a silent wrong continuation, which is the failure principle 3
//! exists to prevent. **Stopping** cannot deliver the exception to a `catch` the guest may
//! have had, and says so plainly instead. A guest that would have caught it is reported as
//! stopping at a throw it could have handled, which is a legible gap; the alternative is an
//! illegible one (D473).
//!
//! Reference: the Itanium C++ ABI (<https://itanium-cxx-abi.github.io/cxx-abi/abi.html>) for
//! the `__cxa_*` names and `_Unwind_Resume`, and ISO/IEC 14882 for `std::terminate` and the
//! allocation functions.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

use crate::{c_len, ptr};

/// Ends the run, naming the C++ operation that could not be completed.
///
/// One place, so every never-returning entry point reports the same way and none of them can
/// quietly grow a `return` instead.
fn stop_for(what: &str, detail: Option<String>) -> u64 {
    let line = detail.map_or_else(
        || format!("the guest reached {what}, which does not return"),
        |text| format!("the guest reached {what}: {text}"),
    );
    eprintln!("orbistoun: {line}");
    orbistoun_core::klog::note(&line);
    orbistoun_core::stop(orbistoun_core::StopReason::Aborted, 0)
}

/// The message a throw helper was given, when it takes one.
fn message(address: u64) -> Option<String> {
    if address == 0 {
        return None;
    }
    // SAFETY: a guest-supplied string under the identity mapping (D014), bounded.
    let len = unsafe { c_len(address) };
    // SAFETY: `c_len` established `len` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr(address).cast_const(), len) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// `operator new(size, nothrow_t)` - allocation that answers null rather than throwing.
///
/// Reference: ISO/IEC 14882 [new.delete.single]. **The one `operator new` that is allowed to
/// answer null**, which is why it is worth having separately: the throwing form's caller does
/// not check, and this form's caller does.
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
/// The alignment is ignored for the same reason a sized delete's size is: the heap records
/// what it handed out, and the caller's figure is at best a duplicate of it.
fn operator_delete_aligned(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    crate::free(args)
}

/// `std::get_new_handler()` - the handler `operator new` calls when it cannot allocate.
///
/// Reference: ISO/IEC 14882 [alloc.errors]. **Null is the correct answer**, not a placeholder:
/// the standard says the initial handler is a null pointer, and nothing in this guest has
/// called `set_new_handler` because that function is not imported. Answering an address would
/// be handing the runtime something to call.
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
/// Reference: Itanium C++ ABI 2.5.3. Reached only from a landing pad that has finished its
/// cleanup and wants the unwind to carry on. There is no unwinder here to carry it on with,
/// and returning would resume the guest **inside a frame that has already been torn down**.
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
/// The number is reported rather than translated: the mapping from it to a condition is the
/// runtime's own, and naming a condition this layer has not established would be inventing
/// one. The raw value is what a reader can look up.
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
