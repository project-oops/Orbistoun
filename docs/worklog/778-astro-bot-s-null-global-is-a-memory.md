# 778. ASTRO BOT's null global is a memory-arena manager whose construction block is never reached - a 2GB arena setup that orbistoun's startup does not run

**2026-09-21** — worklog 777 traced ASTRO BOT (PPSA21564)'s null to the eboot global `image+0xe553bd0`
and named a write-watchpoint as the decider. This tick ran it, found the one writer, disassembled it,
and confirmed it is never reached. The null now has a cause with a shape: a memory-manager construction
that orbistoun's startup skips.

## The global is written in exactly one place, and it never runs

A full scan of the eboot's executable segment for references to `image+0xe553bd0` found **7**: six reads
(one of them `image+0x27df99`, the load that carried the null to the fault in 777) and a single **write**
at `image+0x27ccc2`:

```text
0x27ccc2  mov qword ptr [rip+0xe2d6f07], rax   ; *(0xe553bd0) = rax
```

A write-watchpoint on `image+0xe553bd0` came back **`never touched`**, and so did one on `image+0xe553bc8`
- a global the same block writes 0x60 bytes earlier (`0x27cc91`). So the block is not diverted partway;
it is **never reached at all**.

## What the block is

Disassembled, `0x27cc50..0x27ccc2` is **memory-arena / allocator construction**:

```text
0x27cc70  movabs rax, 0x300000000            ; a 12 GB base/limit constant
0x27cc81  mov r9d, 0x200000                  ; 2 MB (page/grain)
0x27cc91  mov [0xe553bc8], rax               ; store the arena descriptor
0x27cca3  call 0x74e71f0                     ; allocator init (fn ptr 0x82d75e8 on the stack)
0x27ccb6  mov edx, 0x79800000                ; ~2 GB size
0x27ccbd  call 0x74e7200                     ; construct the manager -> rax  (name string 0x819efdf)
0x27ccc2  mov [0xe553bd0], rax               ; publish it to the global
0x27ccc9  test rax, rax; je ...              ; (the ctor's own null-check)
```

So `image+0xe553bd0` holds a **memory manager for a ~2 GB arena**, built once at startup and read
everywhere after (the fault's `[rdi+0x38]` validity check is one such reader). Because the block never
runs, the global stays bss-null, and the first reader that dereferences it without its own guard - the
module routine at `+0x7af792` - faults. This is the same never-runs-construction shape as PPSA04263, but
the object is the process's main memory arena and the slot's writer is a single, identified instruction.

## Next

The question is now narrow: **why is `0x27cc50`'s block not reached?** Find its enclosing function's
entry and caller (native, so a direct-call scan for the function reaches it), and see whether it is a
static constructor orbistoun does not run, an init gated on a flag orbistoun answers wrong, or a path
cut off by an earlier early-return. A memory-arena init that never runs is a strong candidate for an
orbistoun startup gap that would move the wall a long way if closed - ASTRO BOT already reaches 500,260
calls with this arena null, so a great deal works without it, and populating it may unblock much more
than the one deref.

## Gate state

No code changed - a characterisation that finds ASTRO BOT's null global has exactly one writer
(`image+0x27ccc2`), proves the writer's whole block is never reached (two never-touched watchpoints), and
identifies the block as ~2 GB memory-arena construction. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
