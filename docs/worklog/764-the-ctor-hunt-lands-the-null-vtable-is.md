# 764. The ctor hunt lands - the null-vtable is an unconstructed global handler the title itself null-guards

**2026-09-21** — worklog 763 closed the stub-suspect line and named the remaining path as *the ctor
hunt*: disassemble PPSA04263 (Grand Theft Auto V)'s startup and find what leaves `image+0x5b37e98`'s
vtable null. This did that pass. The fault is now understood end to end, and it is **orbistoun's**
(D709): the title's own code null-guards this exact pointer, so a correct emulation reaches it either
null (skipped) or pointing at a constructed object - never non-null-but-unconstructed, which is what we
hand it.

## The faulting function, disassembled

`image+0x19676d7` is a virtual call inside a small per-element "activate handler" routine at
`image+0x1967680`. Unwrapped from the current-gen container and disassembled with capstone (the inner
ELF's executable `ph[0]`, 54 MB), it reads:

```text
0x1967687  movsxd rax, [0x539eea0]        ; rax = index   (= 0 at the fault)
0x196768e  lea    r14, [0x5521c20]        ; r14 = array base
0x1967695  imul   rax, rax, 0x4d0         ; element stride 0x4d0
0x19676b4  cmp    byte [rax+r14+0x3a1], 0 ; "already activated" flag
0x19676bd  jne    skip
0x19676bf  mov    rdi, [rax+r14+0x2d0]    ; rdi = the handler-object pointer
0x19676c7  test   rdi, rdi
0x19676ca  je     skip                    ; << NULL-GUARD: null is a valid, handled value
0x19676d4  mov    rax, [rdi]              ; rax = object->vtable
0x19676d7  call   [rax+0x58]   <-- FAULT  ; vtable is 0, so this reads [0x58]
0x19676da  mov    byte [rax+r14+0x3a1], 1 ; mark activated
```

At the fault `index = 0`, so the handler pointer is `[0x5521c20 + 0x2d0] = [0x5521ef0]` (call it
`field_0x2d0`), and its value is `image+0x5b37e98`. That object's first qword - its vtable - is zero, so
`mov rax,[rdi]` yields 0 and `call [rax+0x58]` dereferences `0x58`. The registers confirm it exactly:
`rdi=image+0x5b37e98`, `rdi -> 00 00 …` (null vtable), `r14=image+0x5521c20`.

**The decisive detail is the guard at `0x19676ca`.** The title is written to accept a null handler here
and skip the activation. It faults only because we give it a pointer that is *non-null yet points at an
object whose constructor never ran*. That is precisely the shape D709 predicts for our own bug.

## Where the object comes from - it is only ever a relocated pointer

A full-text reference scan (all 54 MB, every rip-relative operand) settles the provenance:

- **`image+0x5b37e98` (the object storage): 0 references.** Nothing anywhere in the executable computes
  its address. It is never the subject of a `lea`, so no ordinary static constructor targets it.
- **`image+0x5b37f10` (the pointer slot that *holds* `&object`): 52 references.** Almost all are
  `lea rcx/rdx, [0x5b37f10]` inside a registrar cluster at `image+0x2b44740…0x2b455a8`; exactly one
  loads its *value* (`0x1965f9d mov rax, [0x5b37f10]`), and one null-checks it (`0x2b4841c cmp […],0`).

So the object is reachable **only** through the pointer, and the pointer's value (`image+0x5b37e98`) is
placed by a **load-time relocation**, not written by running code. The sibling default at
`image+0x5b37f30` *is* constructed (by `image+0x2b41b50`); the real object at `0x5b37e98` is not.

## Why the pre-entry poke did not move it

`ORBISTOUN_POKE="…5521ef0:…5b37f30"` (point `field_0x2d0` at the constructed default before entry) left
the fault unchanged: `rdi` was still `image+0x5b37e98`. That is consistent with the value being a
**relocation** applied at load - the poke writes the slot, then relocation resolution overwrites it back
to `0x5b37e98` before entry - or with a registrar write on the construction path. Either way it is not a
value a startup import return controls (763), and it is not a value a pre-entry poke of the *slot* can
hold. A poke is a diagnostic, not a fix (D227); this one only narrowed the mechanism.

## What this means, and the next concrete move

The title constructs this handler on hardware; we leave it a zero-vtable object behind a live pointer.
Two mechanisms remain, and one experiment separates them:

1. **A relocation we apply that hardware would not** (leaving the slot pointing at un-constructed bss
   when it should stay null → the guard would skip). 
2. **A registrar/constructor that runs on hardware and not here** - the `image+0x2b44740` cluster, or
   `image+0x2b41b50`'s sibling for the real object - so the pointer is right but the pointee is never
   built.

The separating observation is a **write-watchpoint on both `0x5521ef0` and `0x5b37f10`**: if neither is
written by guest code, the value is purely our relocation (mechanism 1) and the fix is in relocation
resolution; if the registrar writes it, that function is the gate (mechanism 2) and the question becomes
why it does not run. That watchpoint pass is the next tick - cheaper than this disassembly was, and it
turns "one of two" into "this one".

## Gate state

No code changed - a characterisation that spends worklog 763's named "ctor hunt" and records the exact
faulting routine, the null-guard, the object's zero-reference provenance, and the relocation-vs-registrar
fork for the next tick. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean.
No commit.
