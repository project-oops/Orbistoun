# 765. GTA's stale handler field is a static-init relocation the redirect-registration never overwrote; init-array is the axis and GetProcParam is the next gap

**2026-09-21** — worklog 764 left a fork: the null-vtable handler pointer at PPSA04263 (Grand Theft
Auto V)'s fault is either a relocation we apply that hardware would not, or a registrar that runs on
hardware and not here. A write-watchpoint plus a fault-time object dump settled it, and the answer is
sharper than the fork: the field is a **stale static-initialiser relocation** that a **runtime
redirect never overwrote**, and enabling init-arrays confirms init-array execution is the axis - while
surfacing the next concrete gap.

## What the fault-time memory actually holds

`ORBISTOUN_PEEK` over the three slots at the fault, not read from the file but from live guest memory:

| slot | address | value at fault | meaning |
|---|---|---|---|
| real-handler pointer | `image+0x5b37f10` | `0x0` | **null** - no real handler is registered |
| default handler | `image+0x5b37f30` | vtable `image+0x3c729d0` | **constructed** - valid vtable |
| the object the fault uses | `image+0x5b37e98` | vtable `0x0` | **not constructed** |

And the write-watchpoints: `image+0x5b37f10` is **never written by guest code** (so its `0x0` is its
load state), and `image+0x5521ef0` (`field_0x2d0`, the pointer the fault dereferences) is written only
`0x0`, late, at `image+0x1927344` - never `0x5b37e98`. So `field_0x2d0 = image+0x5b37e98` is placed by a
**load-time relocation**, i.e. a C++ static initialiser `field = &g_real_handler`.

## The mechanism, corrected

The registration at `image+0x1965fbc` computes `field_0x2d0 = real_handler[0] ? : default`. With
`real_handler[0]` null (no device registered), a correct run sets `field_0x2d0 = default`
(`image+0x5b37f30`) - **which is constructed**, so the virtual call at `image+0x19676d7` would succeed.
That registration **never runs before the input-update path** reaches the activate-handler routine, so
`field_0x2d0` keeps its static-initialiser value `image+0x5b37e98` (the never-constructed real handler),
and the guarded virtual call reads a null vtable.

This is not "an object is never constructed" (764's framing). The default the title falls back to *is*
constructed; the bug is that the **redirect from the static-init default to the runtime default is
skipped or mis-ordered**. The caller chain (walked through the faulting routine's frame: return address
`image+0x19669b2`) puts the fault inside input/controller subsystem bring-up, alongside `libSceIme`,
`libSceUserService` and an `/app0/...` config read.

## The axis, confirmed by intervention

`ORBISTOUN_START_MODULES=all` (run every placed module's `DT_INIT_ARRAY` before entry) does **not** leave
the trajectory unchanged - it moves the fault *earlier*, to `read of 0xf7ff0001` at
`the-title's-modules+0x60f4`. `0xf7ff0001` is our own placeholder-error range (D670): an init routine
**dereferenced `libkernel::sceKernelGetProcParam`'s stub return as a pointer**. Two things follow, and
the second is the honest caveat:

- Init-array execution is the relevant axis. The tree's own note says so already -
  `crates/orbistoun-kernel/tests/module_start.rs:5`: *"Nothing in the tree runs a module's
  DT_INIT_ARRAY. That is the wall PPSA02664 stands at."* GTA stands at a relative of it.
- But `START_MODULES=all` ran a **module's** init (`0x48...` module space), not the **eboot's** static
  init, and hit `sceKernelGetProcParam` before reaching the redirect-registration. So it does not yet
  *prove* the eboot registration is what a correct init order would run first - it shows the init path is
  load-bearing and names the first thing missing on it. An intervention that moves a wall is not a
  diagnosis (D226); this one corroborates the static read above and hands the next gap.

## Next

`sceKernelGetProcParam` returns a pointer to the process parameter block the loader sets up; it is a
real, needed function, stubbed today, and init code dereferences its answer. Implementing it honestly -
returning a real pointer to a minimally-populated proc-param, recorded in its knowledge file first
(a function that answers a pointer must never answer an error code) - is the next unit, and it is on the
critical path this run surfaced rather than a guess. Whether it then lets the eboot's redirect-init run
before the input poll is the observation that would close GTA's null-vtable.

## A test race, fixed in passing

The gate's first pass went red on `orbistoun-kernel`'s `a_noted_region_is_found_and_a_gap_is_not`:
`region_containing` returned `None` for a base the test had just noted. The cause was not the doc
change - it was a pre-existing parallel-test race. `note_region`/`clear_noted_regions` mutate one
process-global list, and the crate's two writer tests (`a_noted_region_is_found_and_a_gap_is_not` and
`a_noted_region_can_be_re_protected_and_a_gap_cannot`) run concurrently, so one's `clear` wiped the
other's `note` mid-assertion. Fixed the same way `said` and `script`'s shared-static races were: a
module-level `noted_regions_serial()` guard both writers take before touching the list, poison
recovered so a panicking holder does not cascade. They are the only callers that clear, so serialising
the two closes it.

## Gate state

One test-only change - the serialising guard above - no emulator behaviour touched. The GTA content is a
characterisation that spends 764's fork with a fault-time object dump and a watchpoint, corrects the
mechanism to a skipped redirect over a constructed default, and names `sceKernelGetProcParam` as the
measured next gap. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No
commit.
