# D878 - A flat-segment access reaches global memory

**Status:** assumed
**Date:** 2026-10-14

A flat-encoded memory access whose segment (bits 15:14 of its first word) is flat, 0, is translated
as the global access of the same opcode - GFX10 numbers them alike (`aco_opcodes.py`:
`flat_load_ubyte` and `global_load_ubyte` are both 0x08) - its address taken whole as a guest
address. One whose segment is scratch, 1, is refused by name: it reaches a lane's private memory.

**Known by:** assumed. A flat address inside the local-data-share or private apertures that
`SH_MEM_BASES` places reaches those instead of guest memory; neither aperture is modelled, and a
flat access through one would read the wrong memory. A title's compute program reading a pointer
it was handed reads guest memory.

**Why:** Grand Theft Auto V's compute program reads a byte through `flat_load_ubyte` once its AMPR
command buffers rewind (D877). The names the decoder gives the flat-encoded family come from the
opcode alone, so every flat-segment access was already translated as its global twin, and a scratch
one was too; this says so, and stops the scratch one.

The sub-word loads - `global_load_ubyte`, `_sbyte`, `_ushort`, `_sshort`, recorded by LLVM 18.1.8
at opcodes 8 to 11 - read the word holding their address, and the next for a half at its last byte,
and widen their byte or half, zero- or sign-extended (AMD's published RDNA instruction set).
