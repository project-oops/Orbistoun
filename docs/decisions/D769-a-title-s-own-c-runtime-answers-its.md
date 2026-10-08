# D769 - A title's own C++ runtime answers its exceptions

**Status:** decided
**Date:** 2026-10-08

When a module the title ships exports one of the C++ runtime's unwinding or throwing entry points,
every import of it binds to that module, even though orbistoun implements the name. The names are
`_Unwind_Resume`, `std::terminate`, `__cxa_pure_virtual`, and the runtime's throw helpers
(`std::_Xlength_error`, `_Xout_of_range`, `_Xinvalid_argument`, `_Xbad_alloc`,
`_Xbad_function_call`, `_Throw_C_error`, `_Throw_Cpp_error`). A title that ships no such module
keeps orbistoun's, which end the run and name the operation (D473).

**Why:** orbistoun has no unwinder, so its stand-ins can only stop. A title's own `libc.prx` has
one (D768), and an unwind it began must be continued by the same runtime. A landing pad's
`_Unwind_Resume` that reached orbistoun ended PPSA02664 in the middle of a cleanup. Bound to the
title's libc, the same run goes on from 1.475M calls to 3.137M and a second GPU submission.

**Rejected:**
- Removing the stand-ins: a homebrew title links no C++ runtime of its own and still needs a
  named stop.
- Yielding every name a title module exports: orbistoun's own implementations (the heap, files,
  time) are where its behaviour is measured, and a title's libc reaching the kernel through them is
  the point.
