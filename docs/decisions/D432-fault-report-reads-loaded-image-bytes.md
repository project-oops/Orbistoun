# D432 - A fault report reads the faulting instruction's bytes from the loaded image

**Status:** decided
**Date:** 2026-09-01

A guest fault report includes the bytes at the faulting instruction pointer and the window
before it, read directly from the loaded image inside the fault handler.

**Why:** An offset a guest module reports cannot generally be located back in its file (a wrapped
module's on-disk layout does not match its loaded layout), so the loaded image is the only place
the faulting instruction can be read from. The read is safe because an executable page is always
readable here, and the read never crosses past the single page holding the instruction pointer.

**Rejected:** a separate disassemble-on-demand tool reading the guest's file - cannot locate the
bytes for a wrapped module at all.
