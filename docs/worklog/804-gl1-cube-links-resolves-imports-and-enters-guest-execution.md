# 804. The D247 fix lands: gl1-cube now links, resolves its imports and enters guest execution — the first fully-owned guest to run, and it faults in startup at an address nothing mapped

**2026-09-22** — the fix worklog 803 scoped. gl1-cube (`GLCB00001`), orbistoun's own source end to
end, went from dying at the linker with *"dynamic table lacks a string table"* to running guest code for
the first time. The run now reaches `ContainerParsed → ImportsResolved → Mapped → Linked → Entered`.

## What was wrong, in two places, both the same shape

The container this SDK builds is a **wrapper**: the inner ELF's program headers carry *logical*
file offsets, and the real bytes live in descriptor blocks the wrapper assigns to each header. Two
resolvers read those logical offsets as if they addressed the file directly, and a module built the
platform's way — which the retail corpus never is — exposed both.

1. **`Elf::dynamic_bytes` located the `DYNAMIC` segment by virtual address.** gl1-cube's `DYNAMIC`
   segment has a virtual address of **zero**, which collides with the first `PT_LOAD`, so the address
   route landed on code and the table read as absent though every tag was present. Fix: try the address
   route, and when the addressed bytes are not a usable table, locate the segment by **file offset**
   within the enclosing segment that actually carries its bytes (a new `dynamic_bytes_by_offset`, an
   offset-keyed mirror of `vaddr_to_offset`). Retail titles keep a real address here and never reach the
   fallback.

2. **`Elf::vendor_data_offset` returned the header's own file offset as the base.** The vendor tags are
   offsets *relative to `PT_SCE_DYNLIBDATA`*, and that segment's logical file offset (`0x118e50`) lands
   **past the end of the 1,086,016-byte file**, so the base bounds-checked to `None` and every table read
   "address is unmapped". Fix: for a wrapper container, return the descriptor block's real file start as
   the base the tags are measured from.

With both, the linker resolves strtab / symtab / hash from the SCE tags, reads the symbol table, and
binds the imports — the whole D247 case, closed. No standard `DT_` tags are involved; this is a module
that carries *only* the vendor tags, exactly the case `dynamic.rs:88-100` was written about.

## The error walked forward as each layer opened

- was: `no usable dynamic table: dynamic table lacks a string table, symbol table, or hash table`
- after fix 1: `no usable dynamic table: string table address is unmapped` — the table was found, its
  strtab pointer was not yet resolvable
- after fix 2: **no link error at all** — `reached Linked`, `reached Entered`

## Where it stops now: guest startup, not the linker

The guest runs its startup trampoline and faults on an instruction fetch from `0x8000004ea`, an address
in no region this run mapped:

```
called from image+0xfd34 <- image+0xff82 <- image+0x1a2c <- image+0x2b (entry)
rax=0x259  rdi=0x7  rsi -> stack "[GLCB00001:GL-CU..."  r14=0x8000004ea
```

Two clues line up: `rax = 0x259 = 601` and `rdi = 0x7` match the run census's *"1 system call asked for
directly that nothing here implements: CALL 601, FIRST ARGUMENT 0x7"*, and `rsi` points at the guest's
own title banner. So startup issues syscall 601, which orbistoun does not implement, and then jumps
through `r14` to an address that is almost certainly what that syscall was meant to return. That is the
next tick's target — and, unlike a retail wall, it is walkable against our own source (the crt0 and the
syscall it makes are in this collection).

This is the baseline doing exactly what backlog 037 argued it would: the first fully-owned guest to run
exposes a real, buildable orbistoun gap the tool-bound retail frontier had masked — here, two container
resolvers that only a properly-built module could reach.

## Gate state

`crates/orbistoun-elf/src/lib.rs` changed: `dynamic_bytes` gains an offset-keyed fallback and a new
`dynamic_bytes_by_offset`; `vendor_data_offset` becomes wrapper-aware. `parse` still contains no
`unsafe` and the bytes are hostile-input-safe (principle 4). `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
