# D380 - A fault in this project's own code names the nearest implementation

**Status:** decided
**Date:** 2026-08-29

A fault address inside this project's own binary is reported against the
nearest preceding entry in a sorted table of implementation start addresses,
printed as a name plus offset; a formatted string argument is followed only
when it falls inside a range the run has published, and prints as unmapped
otherwise.

**Why:** The binary carries no symbols and is relocated per run, so a raw
fault address is meaningless on its own; the nearest implementation and its
distance says whether the fault is inside a routine or in support code called
from one. Following every pointer a guest passes is right for guest-computed
addresses but wrong for uninitialized stack contents an overflowing format
string exposes; there the honest answer is that the address is not known to be
valid, not an invented placeholder string.

**Rejected:**
- Reporting only the last import called: names which function the guest
  wanted, not where the fault actually is.
- Dereferencing every string argument unconditionally: an address outside
  every published range is not safe to read and not worth guessing at.
