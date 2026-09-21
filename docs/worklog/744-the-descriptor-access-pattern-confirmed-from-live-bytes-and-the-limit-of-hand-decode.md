# 744. The descriptor access pattern, confirmed from live bytes — and the limit of hand-decode

**2026-09-21** — the indirect peek from 743 gave the fault's heap objects; this tick reads the *code*
that walks them. Both the caller `0x37ea0` and the copier `0x42d90` sit at the fixed guest image base
(`0x400000…`), so `ORBISTOUN_PEEK=0x400000037ea0+0xc0,0x400000042d90+0x140` is a stable, ASLR-free
read. Hand-decoding the bytes confirms worklog 738's descriptor structure — which 724 had flagged as
possibly-wrong static disassembly — from the **live** bytes this time, and pins the offsets. It also
finds the honest edge of what hand-decode can settle here.

## The descriptor access, confirmed

The caller `0x37ea0` prologue and loop, decoded from the dump:

```
mov  r14, rsi            ; r14 = arg1 = the register-group descriptor
lea  r15, [rdi+0x38]     ; r15 = arg0 + 0x38  (a container inside the shared object)
mov  esi, 2
call 0x3fdd0             ; accessor(container = arg0+0x38, 2)  -> rax
...
lea  r12, [rbx+0x3c0]    ; r12 = arg0 + 0x3c0  (the destination std::vector)
; then, unrolled per group k:
movzx edx, byte [r14+0x5b + k]   ; group k present flag
test  edx, edx
jz    <skip group k>             ; flag zero -> group absent, no copy
mov   rsi, [r14+0x18 + 8*k]      ; group k data pointer  -> the copier's arg1
mov   rdi, r12                   ; dest = the vector at arg0+0x3c0
call  0x42d90                    ; append group k's data
```

So, measured rather than inferred: the descriptor `r14` carries **present flags at `[r14+0x5b+k]`
(one byte each)** and **data pointers at `[r14+0x18+8k]`**, walked group by group; each present group's
data pointer is handed to the copier `0x42d90` with the destination fixed at `arg0+0x3c0`. By the
confirmed stride, the tenth group (k=9, the faulting one) reads its data pointer from **`[r14+0x60]`**
and its flag from **`[r14+0x64]`**. This is 738's structure, now from live bytes, with the offsets
nailed down.

## The copier is a std::vector append, not a memcpy

`0x42d90` is not the thin copy 738 read it as. Its body reallocates a vector — `movsxd rax,[rdi+0x44]`
(size), a growth branch that writes `[rbx+0x20]`, `[rbx+0x44]`, `[rbx+0x54]`, and an indirect
`call [r13+0x20]` (a vtable slot) — and the byte copy that faults is one memcpy **inside** it, at
`0x42eb8` (returning to `0x42ebd`, the site every one of the ten copies is charged to). So the wall is
`std::vector<...>::append(group_data)` reallocating and copying a group whose source pointer is bad —
the copier is faithful, the bad pointer came from the descriptor, exactly as 743's finding routed it.

## Where hand-decode stops

Two readings of the fault survive the code dump, and hand-decode cannot choose between them because the
copier reassigns `r13`/`r14` across branches (`mov r14d,edx`; `mov r13,[rbx+0x8]`; `mov r13,[rbp-0x30]`)
and the register values at the memcpy depend on which growth branch ran:

- **(A) `[r14+0x60]` is null**, and the copier reads a field at `+0xa8` of it, so `src = 0 + 0xa8`.
- **(B) `[r14+0x60]` holds the value `0xa8`** (a size/offset where a pointer belongs), read directly as
  `src`.

Both fit `src=0xa8`, `n=0x50`, and `r9=0xf8` (743). 740's phantom experiment (writing `0xb8` left
`src` at `0xa8`) rules out the phantom's size *being* that value, but not which reading holds. Settling
it needs the copier's `src` computation traced precisely — a real disassembler, not hand-decode of
branch-dependent register flow — or the descriptor's `[r14+0x60]` read directly.

## Next step, chosen honestly

Hand-decode has reached diminishing returns; the next tick should bring a proper tool rather than more
manual byte-reading. Two options, both reusable beyond this title: stand up a disassembler (capstone in
the portable `Python314`) and decode `0x42d90` fully to see how `src` is formed; or extend
`ORBISTOUN_WATCH`/`WATCHPOINT` with the same indirect `[<slot>]` form the peek gained in 743, so the
descriptor's `[r14+0x60]` slot can be watched across ASLR to catch whether the inlined producer ever
writes it. The watch is the more decisive of the two — it answers *why* the pointer is wrong, not just
*what* it is.

## Gate state

No code changed — fixed-base code dumps and analysis only. `./bin/orbistoun check` is unchanged from 743
(green but for the same three generated-doc drifts from a prior session's uncommitted
`compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No commit.
