# D473 - Without an unwinder, a C++ throw is a named stop rather than a return

**assumed** - 2026-09-02 (user-directed bulk port, batch 3)

The C++ runtime a guest carries asks for a family of functions whose whole purpose is to throw:
`_Xlength_error`, `_Xout_of_range`, `_Xinvalid_argument`, `_Xbad_alloc`, `_Xbad_function_call`,
`_Throw_C_error`, `_Throw_Cpp_error`, plus `std::terminate`, `__cxa_pure_virtual` and
`_Unwind_Resume`. All of them are declared `[[noreturn]]`, and orbistoun has **no stack
unwinder**.

## Two possible answers, one of them honest

**Returning** is the tempting one, because it is what a stub does and it keeps the guest alive.
It is also wrong in the specific way this project exists to avoid. The caller's code was
generated on the promise that the call does not come back: the instruction after it is
unreachable, no landing pad runs, and whatever half-built object provoked the throw is still
half-built. The guest continues from a point its own compiler proved it could not reach. That is
a silent wrong continuation, and the wall it eventually produces has no visible relationship to
the throw that caused it.

**Stopping** cannot deliver the exception to a `catch` the guest may have had, and this entry
does not pretend otherwise. What it does is say which exception was thrown, with its message,
and end the run there. A guest that would have caught it is now reported as stopping at a throw
it could have handled - a legible gap, and one that names the next piece of work. The
alternative is an illegible one.

So every never-returning entry point goes through one `stop_for` in `orbistoun-libc/src/cxx.rs`,
which reports to stderr and the kernel log and then stops. One place, so none of them can quietly
grow a `return`.

## What this is not

**Not a claim that exceptions are unimplementable here**, and not a reason to avoid them later. A
real unwinder would make all of these ordinary, and this decision is what should be revisited
first when one exists - at which point the throw helpers become thin wrappers over `__cxa_throw`
and this file shrinks. It is recorded as `assumed` rather than `measured` because no guest has
yet been observed reaching one of these: the reasoning is from the ABI's own `[[noreturn]]`, not
from a run.

**Not applied to `__cxa_throw` and its family.** `__cxa_allocate_exception`, `__cxa_begin_catch`,
`__cxa_end_catch`, `__cxa_rethrow`, `__gxx_personality_v0` and the `_ZTV*` vtables are left
**unimplemented** rather than stopped, because they are not all `[[noreturn]]` and pretending
otherwise would be inventing a contract. An unimplemented function is already loud.

Reference: the Itanium C++ ABI (<https://itanium-cxx-abi.github.io/cxx-abi/abi.html>) for
`_Unwind_Resume` and the `__cxa_*` names; ISO/IEC 14882 for `std::terminate` and the allocation
functions. See worklog 304.
