# 794. PPSA04263's null-vtable object is static bss that is never touched, not a failed heap construction; its constructor never runs, and orbistoun runs init arrays only under `START_MODULES` (too early) — the crt-init timing question, potentially buildable

**2026-09-22** — worklog 793 read PPSA04263's null vtable as a heap sub-object "allocated but
unconstructed". A write-watchpoint on the object's own address corrects that and sharpens the wall: the
object is **static**, it is **never written at all**, and orbistoun does not run the eboot's init array
except under a diagnostic that fires it at the wrong time.

## The object is static and never touched

A write-watchpoint on `0x400005b37e98` - the pointer `array[0]+0x2d0` holds at the fault - reports
**`never touched`**. And `0x400005b37e98` is `image+0x5b37e98`: inside the eboot's own image (bss), not a
heap allocation. So the "sub-object" is a **static object** whose bytes are pristine zero because nothing
ever wrote them - including offset 0, its vtable, which is why `mov rax,[rdi]; call [rax+0x58]` calls
through null. Worklog 793's "allocated" was wrong: nothing allocates it (it is static storage) and
nothing constructs it.

## orbistoun runs no init array by default; the diagnostic that does fires too early

`start_placed_modules_if_asked` runs the parsed init arrays (`initialisers_of` →
`start_every_placed_module`) **only when `ORBISTOUN_START_MODULES` is set**, and that runs them all
*before entry* - which sends PPSA04263 BACK to 4 calls, faulting at a module stub (worklog 792), because
the constructors call into a runtime that is not up yet. So a static object's global constructor has two
ways to not run here: by default nothing runs the eboot's init array, and the one switch that does runs
it at the wrong moment. On hardware the init array runs during crt startup, after the runtime is
established, which is neither of orbistoun's options.

## Why this is a better lead than "computed invocation"

The native un-run-construction walls have looked uniformly computed-invocation-bound (ASTRO BOT's
arena-init, 784). This one is different in a way that matters: a **static** object with a null vtable is
the exact signature of a **global constructor that did not run**, and global constructors live in a list
orbistoun already parses (`DT_INIT_ARRAY`) - a fixed, enumerable set of function pointers, not a computed
address. If `image+0x5b37e98`'s constructor is one of them, running the eboot's init array *at the right
point* is a buildable fix that could clear a whole class of static-object walls, not a tracer-only
problem. That is the open question the next step settles: dump the eboot's `DT_INIT_ARRAY` and check
whether any entry constructs `image+0x5b37e98` (init-array timing, buildable) or whether it is reached by
runtime computed dispatch (tracer-bound, like ASTRO BOT).

The caution from 784 stands - running module inits eagerly did not help ASTRO BOT - but ASTRO BOT's
arena-init was established as computed-address-invoked, whereas this is static storage with an un-run
constructor, which is a different mechanism and worth the one check before it is filed with the rest.

## Gate state

No code changed - a decode that corrects PPSA04263's object from heap to static bss, shows it is never
written (global constructor un-run), and finds orbistoun runs init arrays only under `START_MODULES` and
only before entry. The `[experiment]` scratch the watchpoint runs wrote was restored. `./bin/orbistoun
check` green, worklog index regenerated, identity scan clean. No commit.
