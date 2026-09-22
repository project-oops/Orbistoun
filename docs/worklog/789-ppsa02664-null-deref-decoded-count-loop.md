# 789. PPSA02664's `image+0x3f8f0` null-deref is a count-gated copy loop storing through `[r15+0x18]`; zero-filling the descriptor `0x71040c4df8235e1d` does not move it, so the empty-workload escape that cleared PPSA28061 does not apply here

**2026-09-22** — the loop pivoted to the sibling AGC titles PPSA02664/PPSA03416 (worklog 788's plan),
which share the `image+0x3f8f0` wall worklog 770 rooted in the unnamed descriptor-fill NID
`0x71040c4df8235e1d`. This tick disassembled the fault, tested the empty-descriptor escape that worked
for PPSA28061, and records honestly that it does **not** transfer - a correction to worklog 770's hope
that filling the descriptor is the lever.

## The fault, decoded

`image+0x3f8f0` is a `vmovups [rax], xmm0` inside a copy loop (function entry `0x3f8b0`):

```text
0x3f8be  mov  r15, rdi          ; r15 = arg0, a struct
0x3f8c1  test edx, edx          ; edx = arg2 = entry count
0x3f8c3  je   0x3f94c           ; count == 0 -> skip the whole loop
...
0x3f8e0  mov  rdx, [r15 + 0x18] ; entry-array base pointer  <-- NULL
0x3f8e7  lea  rax, [rdx + rax*4 - 0x80]
0x3f8f0  vmovups [rax], xmm0    ; store through null + offset -> fault (write to 0x94)
```

So the null the `+0x94` write goes through is **`[r15+0x18]`** - an entry-array pointer in the struct
`r15`, left zero - and the loop that dereferences it is gated on a **count in `edx`** (`test edx,edx;
je`). A count of zero skips it entirely, exactly the shape of the empty-descriptor escape that cleared
PPSA28061's register walls (worklog 787).

## The empty-workload escape does not transfer

`0x71040c4df8235e1d` is the fill this path's descriptor comes from (the guest `memcpy`s `0x100` bytes
from its `arg0` right after the call, worklog 770). Implemented as a **zero-fill** - write `0x100` zero
bytes to `arg0`, return `0` - on the theory that a zeroed descriptor reads a zero count and takes the
`je` skip.

Measured: the wall is **byte-for-byte identical** - `image+0x3f8f0`, `write to 0x94`, same `[r15+0x18]`
null (call count shifted by 80 only because the return went `0xf7ff0001 -> 0x0`). So:

- The **count `edx` is not in the zeroed descriptor** - it stays non-zero and the loop still runs. It
  is computed elsewhere on the `r15` path (through `0x385e0`, multi-level), not read from
  `0x71040c4df8235e1d`'s buffer.
- **`[r15+0x18]` stays null** whether the descriptor is garbage or zeros, so `r15` is not this buffer
  either - `r15` is a downstream struct whose entry-array pointer a *correct, non-zero* fill would have
  to establish, which zeros cannot.

Zero-fill is therefore an unconfirmed guess that does not move the wall, so it was reverted rather than
shipped - a zeroed descriptor here is plausible output that buys nothing, which is the failure principle
3 forbids. `0x71040c4df8235e1d` stays honestly unimplemented.

## What this narrows

Worklog 770 offered two candidates for the null: "a pointer field left zero in the unfilled descriptor,
or `0x7d86501b8094ef57`'s `0x0` return used as a pointer." This tick shows the null is specifically
`[r15+0x18]` in a count-gated loop, and that **zeroing the descriptor changes neither the count nor the
null** - so the fix is not an empty fill. It is one of: the *real* descriptor contents (a hardware fact,
the obSCEne trace REQ-...7a5d worklog 770 filed), or the origin of `r15` and its count, which is a
multi-level disassembly through `0x385e0` that fixed static addresses may not reach (the same
computed-invocation wall the native titles hit). The count-gated-loop structure is the new detail; the
descriptor's bytes remain the blocker.

## Gate state

No net code change - the zero-fill experiment was measured (wall identical) and reverted, and the
`[experiment]` scratch it wrote to `compat/PPSA02664-app0.toml` was restored. A decode that pins the
null to `[r15+0x18]` in a count-gated loop and rules out the empty-descriptor escape for this title.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
