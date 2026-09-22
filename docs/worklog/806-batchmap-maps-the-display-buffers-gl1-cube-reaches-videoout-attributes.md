# 806. `sceKernelBatchMap` maps the display's scanout buffers, and gl1-cube's display bringup walks one wall further — to `sceVideoOutSetBufferAttribute2`, which is an open rendering-cluster request the cube has now reached

**2026-09-22** — the wall worklog 805 left. With its syscalls dispatching, gl1-cube (`GLCB00001`)
runs its startup and its display bringup, and stops with *display not ready* because
**`sceKernelBatchMap` was unimplemented** — the one call in the run (of 20) that landed on a stub.
Implementing it lets the display's scanout buffers map, and the guest advances to the next call in
the same bringup: `sceVideoOutSetBufferAttribute2`. Verdict **FURTHER** (20 → 28 import calls).

## What the display needed, and why the retail titles never showed it

The AGC display init (`agc_display.c`) brings up a scanout surface in a fixed sequence: probe
`/dev/dce`, `sceVideoOutOpen`, `sceKernelAllocateMainDirectMemory` for one 32 MiB physical block,
then **`sceKernelBatchMap` to map sixteen 2 MiB pages of it** into the GPU virtual range at
`0x40_0000_0000`, at the addresses it will render to. orbistoun implements the open, the allocate,
and the AGC queue create (`sceAgcDriverCreateQueue rc: 0x0`), but `sceKernelBatchMap` fell on a
generic stub — so the map failed, `agc_display_init` returned its error, and
`oops_display_is_ready` was false one step before any surface existed.

`sceKernelBatchMap` was not even declared in a `guest_module!` (the run named it
`unknown::sceKernelBatchMap`): the name is in the symbol database, found statically in a title's
`fakelib`, but nothing bound it. No retail title reached it, because a retail title uses the named
VideoOut interface and never batch-maps its own scanout memory the way a freestanding SDK does.

## The implementation: the placement half of the named map, per entry, at the address it names

`sceKernelBatchMap(entries, count, completed)` takes `count` 32-byte descriptors - `vaddr` (8),
`paddr` (8), `len` (8), `prot` (1) + three pad, `flags` (4) - and maps each. It shares its core
with `sceKernelMapNamedDirectMemory`, factored into `map_direct_at`: reserve-or-reprotect `len`
bytes at the exact guest vaddr the entry names, record the mapping and its physical alias. The one
difference is the whole reason it is a separate function: **a batch entry is never relocated.** The
named map searches for a free address when the guest expresses no preference and writes the answer
back through a `void**`; a batch entry says where it wants each page, so it is placed there or the
batch stops. `completed` is written whatever the outcome, so a caller tells a partial map from a
total failure - our own SDK reads it back and bails if it is not the full count.

Declared in `libkernel`'s `guest_module!` at arity three (the SDK's `(entries, count, completed)`),
registered in the implementations table, and a knowledge entry records it: `found_by = "static"`
(matching `symbols/generated.json`), `known_by = "guest-observed"`, with the return convention
marked inferred rather than hardware-measured.

## Where it stops now: an open rendering-cluster request the cube has reached

The guest now advances through the batch map to **`sceVideoOutSetBufferAttribute2`** - the next call
in `agc_display_init`, which sets a buffer's pixel format, tiling mode and dimensions before the
buffers are registered with VideoOut. It is declared (arity six) but deliberately unimplemented
(D500), and it is **an open request in orbistoun's inbox** - `REQ-20260920T0125Z-a6b3`,
*"sceVideoOutSetBufferAttribute2 is unimplemented for a reason the measurement discharged"*. So the
fully-owned baseline has now walked the display bringup right up to the first rendering-cluster
request it can close, which is the next tick's target. This is backlog 037 working exactly as
argued: the cube reaches, in its own source, the real requests the retail frontier could only
describe.

The call arrives as `(ptr, 0x8000000000000000, 0, 0x780, 0x438, 0)` - `arg3 = 0x780 = 1920` and
`arg4 = 0x438 = 1080`, the width and height, confirming it is the 1080p buffer attribute the cube
set up. `arg1` is a tiling/format word, not a pointer, which is the measurement `a6b3` turns on.

## Gate state

`crates/orbistoun-kernel/src/lib.rs` gains `batch_map` and its shared `map_direct_at`, the
`guest_module!` declaration, and the implementations-table entry;
`crates/orbistoun-hle/data/knowledge/libkernel.toml` gains the `sceKernelBatchMap` entry. The
mapping runs on a `sysv64` frame and every rounding is checked, never panicking (D156). The compat
record advanced itself (`recorded [experiment] entered - 11 imports, 28 calls, 97% standing`), so
the generated compat and status docs are regenerated with it. `orbistoun-kernel` tests 116/… pass,
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
