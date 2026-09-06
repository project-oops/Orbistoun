# D527 - The wall is `sceAgcCreateShader`, and a hardcoded cap was hiding it

**guest-observed** - 2026-09-03

The fault D526 moved to is fully diagnosed. Getting there needed a tool change first, and that
change is the more useful half.

## The chain

```text
lea  rdi,[rsp+0x38]        an out-parameter on the caller's stack
mov  rsi,rbx ; mov rdx,r14
call 0x3e7a0               a validating wrapper: null-checks rdi/rsi/rdx, answers 0x8a6c000a,
                           then tail-jumps to a PLT stub -> GOT slot 0x1992c08
test eax,eax ; je ...
mov  ebx,eax               ebx = 0x7fff0001, orbistoun's placeholder
cmp  ebx,0x8a6c003d        the guest tests for ONE specific error, and this is not it
jne  ...                   so it takes the other path
mov  rsi,[rsp+0x38]        the out-parameter, still zero
mov  eax,[rsi+0x50]        FAULT
```

The import is **`sceAgcCreateShader`**, identified by its `arg0` being exactly
`0x6000007fc538` - which is `[rsp+0x38]`, `rsp` being unchanged between the call and the fault.

I read the wrapper's three null-checks as arity 3. **That was wrong, and a gate caught it.**
`declared_arity_and_recorded_arity_never_disagree` refused the tree: `sceAgcCreateShader` is
declared at **arity 4**, deferring to a knowledge entry that already existed.

Two mistakes in one:

- **A validating wrapper that tail-jumps does not bound its callee's arity.** A tail-jump
  preserves every register, so a fourth argument passes through unexamined. The wrapper's
  checks bound *the wrapper*.
- **`learn` overwrites, and the record it overwrote was better.** The existing entry had arity
  4 **derived from argument dumps across two titles** (D194), and described each argument: arg0
  an out-parameter, arg1 a header beginning `31 32 33 34` then `0x18` then a per-call length,
  arg2 the bytecode. It already knew arg0 was an out-parameter. I replaced that with one
  disassembly's inference and a vaguer purpose.

Restored, with the wrapper observation kept and rewritten to say what it actually shows. What
PPSA02664 adds is that here arg0 is on the **stack** rather than at an image address, which is
new and is what made the fault findable.

`0x8a6c` is libSceAgc's error family, established the same way - the wrapper returns
`0x8a6c000a` and `0x8a6c0002` from its own argument validation, so the family comes from the
guest's code rather than from anything invented.

## Why the placeholder is worse than an error here

The guest does not test "did this fail". It tests **`== 0x8a6c003d`**, one specific condition.
`0x7fff_0001` is not that, so the guest continues as though the call had worked - and reads an
out-parameter nothing wrote.

This is the third time an unwritten out-parameter has been the wall (D507, D509, D524), and the
first where the caller's error handling made a placeholder *more* dangerous than a plausible
vendor code would have been.

## It is not implemented, and that is the rule working

What a shader object *is* has no model here. Writing something into that out-parameter means
putting a fabricated pointer where a guest will dereference it - `[out + 0x50]` is the very next
read. Recorded in the knowledge file with the arity, the evidence and the reason.

## The cap was hiding it, and had already misled me once

`print_findings` had a hardcoded `take(6)`, with the rest summarised as `... and 23 more`. The
findings past the sixth keep their **arguments**, and the arguments are what name a call - so
matching `0x6000007fc538` against the list was impossible until the cap moved.

That same cap had already cost something: I read the six as the whole set and reported "three
stubs left" twice, when the trace held twenty-eight (D526).

So it is `ORBISTOUN_FINDINGS` now - a `Setting` that `Observes`, defaulting to six. Six is right
for reading a run; it is wrong for working one, and the difference is what a setting is for.

**A mistyped value is the default, not zero.** `take(0)` still prints the heading and then
nothing, which reads as *this run found nothing* - the opposite of what a typo should say. An
explicit zero is honoured, because suppressing the list deliberately is reasonable. Broken and
watched to fail.

## What this does not do

It does not move the wall. The guest still dies at `read of 0x50`, at 197 distinct imports. What
changed is that the next thing to build is named, with its arity and its argument shapes, instead
of being behind a truncation.
