# D323 - Data imports get storage, not a stub


**decided** · 2026-08-27 · finishes what [D307](#d307---an-import-that-names-data-was-getting-a-function) deferred

D307 taught the loader to *notice* that an import names data. It still handed one a function
stub, which is the wrong answer in the way that does not announce itself: the guest loads the
slot, dereferences what it found, reads x86 instruction bytes as a pointer and carries on.

`DataBlocks` reserves **one zeroed page per data import**, in an address range clear of both
the images and the thunk table, and `ImportResolver` asks it before the thunk table.

### Why one page each, and why zero

**One each** because the guest writes to some of them - `_Stdout` is an object, not a
constant - and two imports sharing storage would alias where nothing downstream could see
it.

**Zero** for the reason `process_argument_block` is zero: the real contents are not known
from any lawful source, so every field reads as zero rather than as something invented. A
guest reading a pointer gets null and can check it. A virtual call through a null vtable
faults immediately and says where - which is worth far more than executing whatever a stub
happens to begin with.

### The composition is the safety

An import that names code **misses** in the data blocks and falls through to its own stub,
byte for byte as before. That is the property that matters, because it is what stops this
change touching every guest at once, and it is asserted directly rather than assumed.

### What is proven, and what is not

Proven, by test: each import gets distinct storage; it reads as zero; it accepts a write; the
resolver prefers data; a function is unaffected.

**Not proven: that this moves any guest.** `PPSA02664` did go further - `image+0xafc959` to
`image+0xafcc08`, two more distinct imports - but a learned policy entry changed in the same
interval, and the run report said so itself:

```
! explicit stub answers went from 0 to 1, so this verdict measures a settings change
```

So the `FURTHER` is unattributable and is not claimed. **This is a correctness fix justified
by what it stops being wrong, not by a wall it moved** - which is the distinction D226 draws,
and the honest position when the only two candidate causes cannot be separated.

Isolating it needs a build with the storage off, which would mean making a correctness fix
into a setting. That is worth doing only if something later depends on the answer.

### A flake, introduced and removed

The first four tests all reserved at the shipped base. They run in parallel in one process,
so whichever arrived second got `Conflict` - a failure about the tests rather than the code,
and one that would otherwise have appeared intermittently in CI rather than immediately here.
Each test now has a base of its own.

