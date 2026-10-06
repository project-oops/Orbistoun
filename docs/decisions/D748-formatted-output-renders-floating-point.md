# D748 - Formatted output renders floating-point and starred conversions

**Status:** decided
**Date:** 2026-10-06

The formatted-output family renders `%f %F %e %E %g %G` and takes a `*` width or precision from
the arguments, as ISO C 7.21.6.1 says. A floating-point argument is read where the System V psABI
puts it: in the next of the eight vector registers the trampoline already spills (D268), which the
thunk now publishes for the length of a call beside the stack area, or in a `va_list`'s
floating-point save area by `fp_offset`; past either, from the same stack cursor the integers use,
since overflow arguments of both classes share one area in order. The `+`, space and `#` flags are
honoured rather than read and dropped. `%a` is still refused, as its own conversion.

This narrows D183: a format is still complete or empty, and floating point is no longer a cause.

**Why:** D183 refused floating point because the value never reached the renderer. Since D268 it
does, for every call, and a `va_list` always carried it. STKT00001 refused 73 writes a run on `%f`
alone, and CRFT00001 45 on `%*`, each an empty string where the game printed a number.

**Rejected:**
- Rendering through the host's `printf`: the host library's rounding and spelling of infinities
  are not the guest's to inherit unexamined, and it would put a C call in the renderer.
- Leaving the flags ignored: a dropped `+` is a partial rendering, which D183 forbids.
