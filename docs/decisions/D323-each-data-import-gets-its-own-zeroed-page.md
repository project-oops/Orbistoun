# D323 - Each data import gets its own zeroed page

**Status:** decided
**Date:** 2026-09-26

`DataBlocks` reserves one zeroed page per data import, clear of the images and the thunk
table, and `ImportResolver` consults it before the thunk table. The pages are published by
name, so an implementation that reports through storage writes the guest's own slot through
`data_symbol`, and writes nothing if the guest never imported it.

**Why:** a thunk address in a data slot is read as instruction bytes and nothing faults. The
real contents have no lawful source, so zero is what a caller can check, and a call through a
null table faults at once. Guests write some of these objects, so sharing a page would alias.
A code import misses the data blocks and resolves exactly as before. Functions such as
`getopt`, `__error` and `strerror` report through storage and need the guest's own slot.

**Rejected:**
- A function thunk: plausible bytes and no fault.
- One shared page: writes alias between imports.
- Invented contents: a guess the guest acts on.
