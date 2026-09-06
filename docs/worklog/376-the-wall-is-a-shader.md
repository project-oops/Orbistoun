# 2026-09-03 - (/loop) The wall is `sceAgcCreateShader`, and a cap was hiding it

```
wall unchanged: read of 0x50 at image+0xf56e09, 197 distinct
```

Thirteenth cron tick. The fault D526 moved to is now fully diagnosed, and getting there needed a
tool change first.

## The chain

```text
lea  rdi,[rsp+0x38]        an out-parameter on the caller's stack
call 0x3e7a0               a validating wrapper -> PLT -> GOT slot 0x1992c08
mov  ebx,eax               ebx = 0x7fff0001, orbistoun's placeholder
cmp  ebx,0x8a6c003d        the guest tests for ONE specific error; this is not it
mov  rsi,[rsp+0x38]        the out-parameter, still zero
mov  eax,[rsi+0x50]        FAULT
```

The import is **`sceAgcCreateShader`**, identified by `arg0 == 0x6000007fc538` - which is
`[rsp+0x38]`, `rsp` being unchanged between the call and the fault.

**The placeholder is worse than an error here.** The guest does not test "did this fail", it
tests `== 0x8a6c003d`. `0x7fff0001` is not that, so it proceeds as though the call worked. Third
time an unwritten out-parameter has been the wall (D507, D509, D524); first time a caller's error
handling made a placeholder *more* dangerous than a plausible vendor code.

## Two mistakes, both caught by a gate

I read the wrapper's three null-checks as arity 3.
`declared_arity_and_recorded_arity_never_disagree` refused the tree - it is declared at **4**.

- **A validating wrapper that tail-jumps does not bound its callee's arity.** A tail-jump
  preserves every register; a fourth argument passes through unexamined.
- **`learn` overwrites, and the record it overwrote was better.** The existing entry had arity 4
  **from argument dumps across two titles** (D194) and described every argument - it already knew
  arg0 was an out-parameter. I replaced it with one disassembly's inference and a vaguer purpose.

Restored, with the wrapper observation kept and rewritten to say what it actually shows. Both are
now standing checks in the loop file.

## The cap was hiding it, and had misled me once already

`print_findings` had a hardcoded `take(6)` and summarised the rest as `... and 23 more`. The
findings past the sixth keep their **arguments**, and the arguments are what name a call - so
matching that stack address was impossible until the cap moved. The same cap had already cost me
"three stubs left", twice (D526).

`ORBISTOUN_FINDINGS` now, a `Setting` that `Observes`, default six. Six is right for reading a
run and wrong for working one.

**A mistyped value is the default, not zero** - `take(0)` prints the heading and nothing, which
reads as *this run found nothing*. An explicit zero is honoured. Broken and watched to fail.

## Not implemented, and that is the rule working

What a shader object *is* has no model here, and `[out + 0x50]` is the very next read - a
fabricated pointer would be dereferenced immediately.

Decision: [D527](../decisions/D527-the-wall-is-a-shader-and-the-cap-was-hiding-it.md).
