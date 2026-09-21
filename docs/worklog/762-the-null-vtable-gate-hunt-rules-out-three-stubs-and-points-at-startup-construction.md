# 762. The null-vtable gate hunt rules out three stubs and points at startup construction

**2026-09-21** — continuing the hunt for what stops PPSA04263 (Grand Theft Auto V) from constructing
the object at `image+0x5b37e98` (worklogs 759-761). This tick tested the stub suspects the honest way -
implement or disassemble, then measure - and ruled three out, which narrows the gate to startup code a
different tool has to reach.

## Ruled out, with evidence

- **`sceImeUpdate`** - the temporally closest stub (called from `image+0x1966570`, in the fault's own
  code region). Disassembling that caller shows its return is **ignored** (the branch after it tests an
  unrelated global, not `eax`), and `sceImeUpdate(handler)` only invokes its callback when there are
  pending IME events - with no on-screen keyboard there are none, so even a correct implementation
  invokes nothing here. Neither its return nor its callback gates the construction.
- **`sceKernelFstat`** - wired in worklog 761. It moved the run `FURTHER` (+6 calls) - a real gap - but
  the fault stayed at `image+0x19676d7`. Not the null-vtable's gate.
- **`sceCoredumpRegisterCoredumpHandler`** - answered `OK` as a quick measure (a title registering a
  crash handler is accepted, the `sceErrorDialogInitialize` shape). Verdict `same, nothing moved`.
  Reverted, because it is not the gate and a standalone honest fill can be added on its own later.

## What is confirmed about the object

`array[0]` (the `0x4d0`-byte subsystem record at `image+0x5521c20`) is **entirely bss** - a `PEEK` of
it at the fault window reads all zeros, including `field_0x2d0`, which the faulting thread had already
loaded as the object pointer before another thread raced it back to zero. So the subsystem record was
never initialised, and its object's vtable is never written (worklog 760's watchpoint). The object is
**registered then never constructed**.

Crucially, the construction is **not on the first-use path**. The code at `image+0x196xxxx` - where the
fault and `sceImeUpdate` live - is the lazy *use* of the object (check the flag, call the virtual
method). The code that *constructs* it (writes the vtable) runs at **startup**, earlier and elsewhere,
and never executed. So the gate is upstream in startup, not in the region disassembled so far.

## What is left, and the certain path

Two stub suspects remain, both deferred by prior decisions: `sceUserServiceGetGamePresets` (an
out-parameter getter, D346) and `scePthreadGetaffinity` (D523). Either would need an honest value, not
a guess, to test. But the gate may equally be a **non-stub upstream value** - something orbistoun feeds
the startup construction wrong that is not a missing import at all.

The way to *know* rather than keep guessing is the **ctor hunt**: find the instruction that writes the
vtable into `image+0x5b37e98`, which lives in the executable's startup code, by disassembling it and
locating the reference to that address. That is a heavier, dedicated pass than reading a single caller,
and it is the honest next step - it tells us the gate instead of implementing stubs to see which moves
the wall. Every wall here is orbistoun's (D709); the title constructs this object on hardware.

## Gate state

No code changed - the `fstat` fix landed in worklog 761, and this tick's Coredump measure was reverted.
A characterisation consolidating three ruled-out suspects and the narrowed target. `./bin/orbistoun
check` green, worklog index regenerated, identity scan clean. No commit.
