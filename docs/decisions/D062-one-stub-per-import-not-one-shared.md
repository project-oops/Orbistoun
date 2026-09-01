# D062 - One stub per import, not one shared target

**decided** · 2026-08-19

This is the other half of D005. Relocation writes an address into a linkage table slot;
a thunk is what lives at that address.

A single shared target answers "did the guest call something unimplemented?" and nothing
else, which is worth very little. One stub per import answers **which**, in order, with
counts - the entire input to the loop this project exists to run: execute it, read what
it wanted, implement the frequent ones, execute it again. The cost is 32 bytes per
import; the 96 MB executable spends 45 KiB across 1,411 stubs, once.

**The carrier registers are forced.** A stub puts its own index in `r10` and the
trampoline address in `r11` - the only two registers System V lets a callee destroy that
do **not** carry an argument. Any other choice corrupts an argument before the
trampoline can save it, and the corruption stays invisible until some implemented
function eventually reads the wrong value.

An absolute jump through a register rather than a relative one, because the table and
the trampoline are separate allocations and nothing keeps them within two gigabytes.

**One hand-written trampoline serves the whole table**, because Rust cannot read `r10`.
It spills the six argument registers and re-presents them as an ordinary call. Its
`sub rsp, 8` is not decoration: System V wants `rsp % 16 == 0` at a call site, and
getting it wrong does nothing at all until some callee runs an aligned vector
instruction against a stack slot.

Recording obeys principle 9 - no allocation and no locks on the call path. Counters are
allocated when the table is built; a bounded ring keeps the first calls **in order**,
because a histogram loses the sequence and the sequence is what says what the guest was
trying to do.

