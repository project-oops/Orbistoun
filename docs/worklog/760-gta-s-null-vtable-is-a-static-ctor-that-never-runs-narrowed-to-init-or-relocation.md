# 760. GTA's null-vtable is a static constructor that never runs - narrowed to init or relocation

**2026-09-21** — continuing worklog 759's dig into PPSA04263 (Grand Theft Auto V), tracing the null
vtable at `image+0x5b37e98` to its source with watchpoints. The mechanism is now proven, the wrong
leads are ruled out, and the root is narrowed to two loader-level candidates - recorded so the next
tick starts at the inspection rather than re-deriving all of this.

## What the watchpoints proved

Two watchpoints on the object, one run (`ORBISTOUN_WATCHPOINT`):

- **`0x400005b37e98:w` (the vtable slot) - never touched.** Nothing writes it across the entire run.
  So the object's constructor never executes; this is not a ctor that ran and wrote zero, it is a ctor
  that never ran at all.
- **`0x400005521ef0:w` (`array[0].field_0x2d0`, the object pointer) - one runtime write, of `0x0`, at
  `image+0x1927344`.** The value it holds at the fault (`0x400005b37e98`) was therefore set
  **statically at load** - a relocation, not runtime code - and the lone runtime write is a racing
  zero from another thread (the title is heavily threaded). So the pointer is correct; the object it
  points at is simply never constructed.

## What is ruled out

- **The mutex** (worklog 759): the report's guess, disproved by the disassembly.
- **The runtime stubs**, by ordering. `sceUserServiceGetGamePresets` (writes nothing to its out-param)
  and `sceKernelFstat` were tempting suspects, but a **static** constructor runs at program init,
  *before* any of those runtime calls - so they cannot gate it. The suspects in 759 are set aside.
- **Eager init.** `ORBISTOUN_START_MODULES=all` runs every placed module's initialisers before entry
  and made the run **worse** - `BACK` to 4 calls. Eager, out-of-order init breaks the guest; it is not
  the fix, and it does not construct this executable-image object anyway (that object is the eboot's,
  not a placed module's).

## The narrowed root

Orbistoun does not run the executable's `DT_INIT_ARRAY` itself - by design it reaches the executable's
globals **through the guest's own crt** (D376), and that crt runs (30,454 calls, and the vast majority
of globals are constructed, or the title would die far earlier). So the failure is precise: **one
global object's static constructor is missing from what runs at init**, while its neighbours run.

Two loader-level mechanisms remain, both orbistoun's and both checkable:

1. **A skipped relocation of the vtable slot.** `RelocationTally` counts entries left unapplied
   (`unsupported` type, `unresolved` symbol, `weak_zero`). If the pointer that initialises
   `image+0x5b37e98` is one orbistoun skipped, the slot stays `0` with no ctor involved - the same
   class as the PPSA02664 shader relocation (worklog 748), one layer up.
2. **A truncated `DT_INIT_ARRAY`.** If a function pointer in the executable's init array is null
   (an unrelocated entry), the crt's ctor loop stops there, and every ctor after it - including this
   one - never runs. That it is *one* subsystem and the program otherwise works fits a truncation late
   in the array.

## Next step

Inspect the eboot's `DT_INIT_ARRAY` (does it carry a null mid-array?) and the eboot's `RelocationTally`
(`complete()`, or entries `unsupported`/`unresolved`?). One of those two names the fix. Both are
orbistoun loader work - the title is not at fault (D709); it runs on hardware, so the constructor runs
on hardware.

## Gate state

No code changed - a characterisation extending worklog 759 with the watchpoint proof and the narrowed
root. Diagnostics: `ORBISTOUN_WATCHPOINT`, `ORBISTOUN_START_MODULES`, `ORBISTOUN_PEEK` + capstone.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
