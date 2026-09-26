# orbistoun-thunk

Per-import thunks: the machine code a guest lands on when it calls out, and the dispatch
behind it.

It holds one fixed-size stub (`THUNK_SIZE`) per import, the shared trampoline the stubs jump
to, the handler table, argument recording, stack-alignment checking, and the fixed-size ring
the run report is built from. A call either reaches a real implementation or is answered by
the stub policy, and the report says which. It builds on [orbistoun-abi](../orbistoun-abi/)
for the calling convention; [orbistoun-loader](../orbistoun-loader/) writes the stub
addresses into the guest's slots.

## Interception is linking

Relocation writes an address into a procedure-linkage slot; this crate is what lives at that
address. There is no hooking pass: the guest calls what the linker put there, and the linker
is orbistoun.

## Rules

- **One stub per import, not one shared stub.** A shared target answers only "did the guest
  call something unwritten?". One per import answers which, in order, with counts - the input
  to the whole loop - for a fixed cost per import, paid once.
- **`r10` and `r11` carry the index and the trampoline address**, because they are the two
  registers System V lets a function destroy that are not argument registers. Any other choice
  corrupts an argument before the trampoline can save it.
- **Absolute jump through a register.** The table and the trampoline are separate allocations
  with no guarantee of landing within two gigabytes of each other.
- **Write, then execute.** The table is populated writable and flipped to read-execute before
  any guest can reach it.
- **The hot path stays lean.** Titles make millions of calls through here per second of run.
  The handler is looked up once per call and every decision is made from that result; an
  extra predicate costs nothing on the calls being investigated and a great deal on the rest.
