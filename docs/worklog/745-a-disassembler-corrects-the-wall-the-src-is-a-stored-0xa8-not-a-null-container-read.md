# 745. A disassembler corrects the wall: the src is a stored `0xa8`, not a null-container read

**2026-09-21** — capstone is already present in the portable `Python314`, so the code bytes dumped in
744 could finally be disassembled instead of hand-decoded. It took one pass to overturn a reading four
worklogs deep. The faulting `memcpy`'s source is **not** a null container dereferenced at `+0xa8`; it is
a descriptor field that literally **holds the value `0xa8`**, read straight through as the source
pointer. Hand-decode had invented the `+0xa8` dereference and a `0xf8`-byte container; the disassembler
shows neither exists.

## What the copier actually does

`0x42d90`, disassembled (not guessed):

```
0x42e9f  shl  r12, 3          ; r12 = arg2 << 3   (arg2 = the per-group count byte)
0x42ea7  mov  rsi, r13        ; src = r13 = arg1, restored from [rbp-0x30] on both paths
0x42eaa  mov  rdx, r12        ; n   = count << 3
0x42eb8  call memcpy          ; memcpy(dst, src = arg1, n = count << 3)
```

`r13` is reassigned to a vector cursor mid-body (`mov r13,[rbx+8]`) and restored to the original `arg1`
before the copy (`mov r13,[rbp-0x30]`), so the source is exactly the pointer the **caller** passed —
with no offset added. And the count is `arg2 << 3`. This is a `std::vector` element append: the memcpy
copies `count` eight-byte words from `arg1` into the vector at `[rbx+0x3c0]`.

## What the caller passes

`0x37ea0`, disassembled, confirms 744's structure and names the operands:

```
mov  r14, rsi                  ; r14 = descriptor
...
movzx edx, byte [r14+0x5b+k]   ; the per-group value passed as arg2  -> count
mov   rsi, [r14+0x18+8k]       ; the per-group value passed as arg1  -> src
call  0x42d90
```

So the byte at `[r14+0x5b+k]` is not a boolean present-flag (744's reading); it is the group's
**register count**, and `n = count << 3` is that many eight-byte registers. The faulting copy has
`n = 0x50`, so the count is `0xa` (ten registers), and its `arg1` — the descriptor's data pointer for
that group — is the value that faults.

## The correction

`src = 0xa8` is the **stored data pointer**, read directly. The descriptor field for the faulting group
contains the small integer `0xa8` where a valid pointer to the register data belongs; `memcpy` reads
from address `0xa8` and dies. There is no null container and no read at `+0xa8` — that was hand-decode
filling a gap the disassembler closes. Two earlier claims fall with it:

- **738 / 742 / 744's "null container, copied from `+0xa8`"** — wrong. The `+0xa8` was never a
  dereference; `0xa8` *is* the field's value.
- **743's "the container is `0xf8` bytes (`r9=0xf8`)"** — wrong, and wrong the same way 719-739 were:
  `r9` at the fault is `memcpy`'s mid-execution state, not a call argument
  ([[fault-registers-are-mid-execution-not-call-args]]). `0xa8 + 0x50 = 0xf8` was a coincidence read
  off the register dump, exactly the trap the copy-operands tooling was built to stop — and it caught
  everything except this one number, which never went through the tooling.

The wall, stated cleanly: a register-group descriptor holds `0xa8` in a data-pointer slot whose count
says ten registers. The copier faithfully copies from `0xa8`.

## Where `0xa8` comes from — the decisive next test

`0xa8` is almost certainly `0 + 0xa8`: the producer computed `data_ptr = base + 0xa8` with `base` an
object orbistoun answered null for, and stored the result. 740 already ruled out the phantom GetSize's
*written size* (changing it to `0xb8` left `src` at `0xa8`), but not the phantom's **return**, which is
`0x0` and untested as the base. The one-bit oracle (principle 5) is clean: make the phantom
`0x7d86501b8094ef57` return a non-zero sentinel instead of `0x0` and re-run — if the stored pointer
becomes `sentinel + 0xa8`, the phantom's return is the base orbistoun is getting wrong, and returning a
real allocation there is the fix. If `src` stays `0xa8`, the base comes from elsewhere and the producer
disassembly (now cheap, with the disassembler proven) is the next read.

## Tooling note

The disassembler that settled this is a scratch capstone script, not orbistoun's. The recurring need to
read guest code at a fault — this is the fourth worklog to hand-decode bytes — is the case for teaching
the fault report to disassemble the window it already dumps (a permissive-licensed x86 decoder passes
the `deny.toml` allow-list). That is a deliberate follow-up, noted here rather than rushed in.

## Gate state

No code changed — disassembly and analysis only. `./bin/orbistoun check` is unchanged from 744 (green
but for the same three generated-doc drifts from a prior session's uncommitted `compat/PPSA02664-app0.toml`
edit, not this tick). Identity scan clean. No commit.
