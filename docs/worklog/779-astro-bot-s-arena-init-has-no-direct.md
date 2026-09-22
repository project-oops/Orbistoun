# 779. ASTRO BOT's arena-init has no direct caller; it is one entry in a custom startup init-table of init-fn records that orbistoun never walks

**2026-09-21** — worklog 778 found ASTRO BOT (PPSA21564)'s null-arena manager is built by a single
function `image+0x27cc20` that never runs. This tick found the function's entry, proved it is never
*called*, and found how it is meant to be invoked: not by a direct call and not by `DT_INIT_ARRAY`, but
by a **custom init-table** the game registers itself in.

## The function is never called

The arena-init's entry is `image+0x27cc20` (`push rbp; mov rbp,rsp; push r14; push rbx`, int3-padded from
the hash function before it). A write-watchpoint on the first global it writes, `image+0xe553bb8`
(`mov [..], 0x79800000` at `0x27cc2d`, the very first instruction after the prologue), came back **`never
touched`** - so the function body never begins. And a full-text scan of the eboot for `call`/`jmp` to
`0x27cc20` found **0** call sites. It is not invoked by anyone directly.

## How it is meant to run: a custom init-table

The eboot has **no `DT_INIT_ARRAY`** (checked: `DT_INIT_ARRAY`/`PREINIT_ARRAY` all zero, as PPSA04263).
So the arena-init is not a standard static constructor. Its address `0x27cc20` appears **exactly once** in
the whole image, as data, at vaddr `image+0xee136f0` - inside an array of 24-byte records:

```text
{ init_fn : 0x27cc20, arg/name_ptr : 0x88dc160, 0x8 }
{ init_fn : 0x27cd30, arg/name_ptr : 0x88dc168, 0x8 }
{ init_fn : 0x27cd80, arg/name_ptr : 0x88dc170, 0x8 }
...
```

Each record pairs an **init function** (a cluster at `image+0x27cxxx`) with a data pointer (a name or
config in `image+0x88dcxxx`) and an `8`. This is a **game-defined startup registry** - the title's own
"run these initialisers" table - and the arena-init is one of its entries. A walker somewhere iterates
this table and calls each `init_fn`; because the walker does not run (or does not reach this entry), the
arena is never built and `image+0xe553bd0` stays null.

## Next

The walker is the last link: find the code that references the table region around `image+0xee136f0`
(a `lea` to its start/end and a `call [entry]` loop), which a full-text ref scan into that range is
already running for. Once found, disassembling it says whether the walker never runs (an orbistoun
startup gap, the same class as PPSA04263's un-run registration) or runs but stops before this entry. A
title-defined init-table that orbistoun does not drive would be a **shared root** - the sort of
mechanism several titles could depend on - which is why this is worth taking to the walker rather than
stopping at "a table exists".

## Gate state

No code changed - a characterisation that finds ASTRO BOT's arena-init has no direct caller, is not a
`DT_INIT_ARRAY` ctor, and is registered as one entry in a custom 24-byte-record startup init-table at
`image+0xee136f0`; the walker that should drive it is the next step. `./bin/orbistoun check` green,
worklog index regenerated, identity scan clean. No commit.
