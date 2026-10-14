# D842 - A program that reads its own address is translated for it

**Status:** decided
**Date:** 2026-10-14

`s_getpc_b64` writes the address of the instruction after it into a scalar register pair. A module
built from a program containing it is translated for the guest address the program was loaded at.
The address is a translation input, `UserData::program_address`. With it, the instruction writes
that address plus the next instruction's offset as constants. Without it, the instruction is refused
by name.

The pipeline sets the address only for a program that contains `s_getpc_b64`, and salts the
module's key with it. Such a program is a different module at each place it is loaded, and every
other program stays one module wherever it is. A computed buffer access (D753) carries the address
too, so a prefix that forms a base from the program counter runs the same per draw.

**Why:** ACO places a shader's constant data after its code and reaches it through `s_getpc_b64`
and a carry-chained add. PPSA21564's vertex shaders do this. The address is the guest's own fact
about the program, as the address a draw's registers name it at, so it is read, not assumed.

**Rejected:**
- Folding the address into every module's key: identical programs at two places would be two
  modules for no reason, and the kept translations would stop matching across runs that load a
  program somewhere else.
- Answering the counter with zero, or with a fixed address: the constant data would be read from
  somewhere the guest never wrote.
