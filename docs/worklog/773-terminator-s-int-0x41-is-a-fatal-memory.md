# 773. Terminator's int 0x41 is a fatal memory-resource assertion from a pre-set-fatal template, raised through a deep chain with an /app0 file path on the stack

**2026-09-21** — worklog 772 explained PPSA25872's regression as a real path change into an error branch
and named the fix target as the error source. This disassembles that source. The `int 0x41` is a fatal
assertion in a memory/resource subsystem, and the stack ties it to worklog 772's FS-change hypothesis;
the exact failing operation is a few frames further up, but the shape is now clear.

## The abort is a flag-gated fatal assertion

Disassembling `image+0x17554a3` (the fault) in PPSA25872's unwrapped eboot:

```text
0x1755487  lea rdi, [rip+...]          ; rdi = "%s"  (image+0x1e5dccc)
0x1755490  call [rbp-0x90]             ; the formatter (vsnprintf-like)
0x175549d  test byte [rax+0x30], 0x10  ; rax = the error-context; test the "fatal" bit
0x17554a1  je 0x17554a5               ; not fatal -> skip
0x17554a3  int 0x41                    ; fatal -> abort
```

So it is not a bare trap: it formats an error and aborts **only when a fatal bit is set** on the
error-context. At the fault that byte is `0x11` (bit `0x10` set). The context was copied from a template
at `image+0x1dd0840` whose flag byte is pre-set to `0x11` - i.e. a **specific error type that is always
fatal**, not a runtime-decided severity.

## The chain, walked

Walking the `rbp` frames from a stack `PEEK`:

- `image+0x17554a3` (reporter, `fn 0x1754ed0`) ← `image+0x1755b55` (a formatter) ← `image+0x7b6081`
  (the **raiser** - it `vmovups` the fatal template `0x1dd0840` and emits it through the logger
  `0x1755880`) ← `image+0x7b62dd` ← further frames still in the `0x7b6xxx` memory/error subsystem.

So several layers of logging and error-emission sit between the trap and the code that detected the
failure. The raiser and its callers are all in one `0x7b6xxx` cluster - a memory/resource manager.

## What the stack says the failure is

The `PEEK` window around the frames carries, in order: memory-allocation log fragments (`"…ALLOC"`,
`"Alloc 0B | pe…"`, `"…resource"`, `"Memory …"`), the pool name **"Hostname Lookup Memory Pool"**, the
error string **"Invalid info address."** (which lives in a middleware string table, 0 direct code refs,
so it is indexed not `lea`-d), and a **`/app0/Me…` path fragment**. Together these read as a
**resource/memory-pool setup that failed** - a pool getting a zero size or an invalid base address - and
the `/app0` path says the resource is **file-backed**.

That is exactly the surface worklog 772 predicted: a8b2d74's FS rewrite altering a file/descriptor result
Terminator reads for a resource, so the pool it builds from that file comes out empty/invalid and the
title fatally asserts. The chain is consistent with it; it does not yet prove it.

## Next

One more climb reaches the frame that initiated the failing pool/resource build (Frame 4+ at
`rbp 0x6000007fb8b0`, return `image+0x7b62dd`'s caller), which names the file read or size query behind
the `/app0/Me…` path. That query, answered wrong since a8b2d74, is the thing to fix - not a revert
(worklog 772). This is a genuine multi-frame grind; it is being taken to the root rather than rotated
away from, because a fixable file/size answer here moves PPSA25872 back the 17,566 calls it lost and does
it honestly.

## Gate state

No code changed - a disassembly that names the abort as a flag-gated fatal assertion from a pre-set-fatal
template, walks the emission chain three frames into the `0x7b6xxx` memory subsystem, and reads the stack
as a file-backed resource/pool failure consistent with worklog 772's FS hypothesis. `./bin/orbistoun
check` green, worklog index regenerated, identity scan clean. No commit.
