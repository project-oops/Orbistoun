# 748. The wall is a relocation-only bug — the group 0 register data is present, only its pointer is raw

**2026-09-21** — the full descriptor dump closes the "what". The register data the faulting copy wants
is **already in memory**, at the address group 0's pointer should hold; the pointer alone was never
relocated from its relative value to an absolute one. Nothing is missing but the fixup. And the guest
builds this header entirely on its own — no shader API is called at all — so the relocation that skipped
two fields is the guest's inlined loader, not an orbistoun handler.

## The data is there; the pointer is raw

Dumping the whole descriptor (`[0x6000007fc190]+0x120`, the header base reached through the copier's
saved-`r14` slot):

```
+0x00  "1234" 0x18                          ; magic
+0x08  base+0x110   (relocated sub-table)
+0x10  base-0x100   (bytecode-ish pointer)
+0x18  0xa8         ; group 0 data pointer -> RAW; should be base+0xa8
+0x20  base+0x70    ; group 1 -> relocated
+0x28  base+0x38    ; group 2 -> relocated
+0x30  base+0x60    ; group 3 -> relocated
+0x38  0x58         ; group 4 data pointer -> RAW; should be base+0x58
+0x4c  0x5          ; a count of five
+0x5b  0x0a  +0x5c 0x06  +0x5d..0  ; per-group register counts: groups 0 and 1 present, 2-4 empty
+0xa8  80 00 00 00 7c c0 37 d8 …   ; group 0's register data — PRESENT, ten registers' worth
```

So `base+0xa8` holds real register data. Group 0's pointer at `+0x18` should be `base+0xa8`
(`0x…6a8`), but it holds the bare `0xa8`, and the copy reads address `0xa8` and dies. The fix the guest
needs is one relocation it did not do — not a missing buffer, not a missing call. Group 0 is present
(count `0x0a`, ten registers, `n = 0x50`) and its data is laid out; only the address fixup is absent.

## The shape of the miss

The relocated fields are `+0x20`, `+0x28`, `+0x30` — the **middle three** of the five group pointers —
and the two skipped are the **endpoints**, `+0x18` and `+0x38`. That is exactly `create_shader`'s
hardcoded relocation set `&[0x20, 0x28, 0x30]` (`agc.rs:253`), and it reads like a loop that ran
`0x20..=0x30` where it should have run `0x18..=0x38`. But this is not orbistoun's `create_shader`: the
whole run contains **zero** shader calls — no `sceAgcCreateShader` by name, and the only two unresolved
`libSceAgc` NIDs are `sceAgcInit`'s alias and the phantom, neither of them a shader create. The guest's
own inlined loader built the header and did the three-of-five relocation.

## What this means, and the honest gap

On hardware the same guest code produces valid pointers at all five, so the divergence is an **input**
the inlined loader reads while relocating — a bound, a count, or a step that orbistoun answers such that
two fields are left raw. The descriptor even carries a `5` at `+0x4c`, the count that a correct loop
over five fields would use; whatever the loader actually counted, on orbistoun it came out three. Which
value that is, and where orbistoun sets it, needs the loader's code — and that is the part still out of
reach: the header is built early, into an ASLR'd object, so a watchpoint on the relocation cannot be
armed before the write, and the loader's address is not yet known.

## Next step

The construction is the last unknown, and it is now a bounded search rather than a fault to stare at.
The header is a shader blob the guest parses (the `"1234"` magic), so the loader is the code that tests
a dword against `0x31323334` and walks the group table; finding that test in the guest image names the
loader, and disassembling its relocation loop shows the bound it reads and where orbistoun feeds it.
Failing a cheap way to locate it, the parallel move is to confirm orbistoun serves the shader blob
whole — a truncated or mis-sized asset read would give the loader a short table and the exact
three-of-five it produced. Either way the target is finally singular: one loop bound, one input.

## Gate state

No code changed — an indirect peek of the descriptor's full extent, and reading `agc.rs`.
`./bin/orbistoun check` is unchanged from 747 (green but for the same three generated-doc drifts from a
prior session's uncommitted `compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No
commit.
