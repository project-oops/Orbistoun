# D061 - Thread-local storage is variant II, and the block sits *below* the pointer

**decided** · 2026-08-19

The single most consequential fact: a variable at offset `x` in a module thread-local
image is read at a **negative** offset from the thread pointer. Laying the block out
above the pointer makes a guest read whatever precedes its control block - plausible
values, wrong ones, and no fault to point at the cause.

Only the module own block is answered. `DTPMOD64` gives the main module id, `DTPOFF64`
the addend unchanged, and `TPOFF64` the addend minus the block size. A relocation naming
another module needs a descriptor table and a second loaded image, so it is **deferred
and counted** rather than answered with a plausible number.

Installing the thread pointer - writing an `fs` base - is not done yet and is a separate
platform problem. It has not blocked anything: **all four** commercial executables
examined declare no thread-local relocations at all.

