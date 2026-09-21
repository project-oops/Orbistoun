# 759. GTA's next wall is a null-vtable virtual call, not the mutex the report guessed

**2026-09-21** — with `rpf.cache` served (worklog 758) PPSA04263 (Grand Theft Auto V) runs 192 calls
further and stops at a new wall, `image+0x19676d7`. The run report named the calls just before it and
guessed `scePthreadMutexUnlock`'s answer was dereferenced as a pointer. Disassembling the fault shows
that guess is wrong, and the real cause is one level deeper - which is worth recording because the
wrong lead is the kind that becomes another weeks-long detour if believed (D709).

## What the report guessed, and why it is a red herring

The fault is `read of 0x58` with `rax = 0`, and the report offered `scePthreadMutexUnlock answered 0x0
immediately before, and the guest dereferenced that value here`. But `scePthreadMutexUnlock` returning
`0x0` is *correct* - that is success. Capstone on the fault window (`ORBISTOUN_PEEK`) shows the
instructions the register heuristic could not:

```asm
mov  rdi, [rax+r14+0x2d0]   ; rdi = a stored object pointer (array[0].field_0x2d0)
test rdi, rdi ; je ...       ; the object is non-null, so proceed
lea  rbx, [rax+r14+0x3a1]    ; &an "already initialised" flag
mov  rax, [rdi]              ; rax = the object's VTABLE pointer
call [rax+0x58]              ; <-- FAULT: a virtual call, but the vtable (rax) is 0
mov  byte [rbx], 1           ; would mark the flag set
```

`rax` is `0` because `[rdi]` is `0`, not because the mutex answered `0`. The mutex calls are simply the
nearest traced imports; the fault is a **virtual call through an object whose vtable pointer is null**.

## The object exists but was never constructed

`rdi = image+0x5b37e98`, and its first sixteen bytes are all zero - a `.bss` object whose vtable slot
was never written. This is **not** a missed relocation: an un-applied relocation leaves a stale
*relative offset* (the shape of the PPSA02664 shader bug, worklog 748), not zeros. Zeros mean the
constructor that would set the vtable never ran.

The access shape is a lazy-init: a global array of `0x4d0`-byte records (`image+0x5521c20`, indexed by
a counter at `image+0x53a1ea0`), and for `array[0]` the code checks an "already done" flag at `+0x3a1`,
and if not done, calls a virtual method on the object at `+0x2d0` and sets the flag. So the object is a
subsystem that was *registered* (`+0x2d0` points at it) but not *constructed* (null vtable) - a
deferred construction that was short-circuited.

## Where construction runs, and where it did not

Orbistoun runs a placed **module's** `DT_INIT`/`DT_INIT_ARRAY` through `run_initialisers` - on a guest
`sceKernelLoadStartModule`, or eagerly under `ORBISTOUN_START_MODULES`. The **executable's** own init
array runs from the guest's crt at entry. PPSA04263 executed 30,454 calls, so the crt ran and the
ordinary static ctors ran with it - this object is not one the whole-program init missed, or the title
would have died far earlier. So the construction that did not happen is a **later, conditional** one:
something the guest does on the path to this subsystem was short-circuited.

The prime suspects are two out-parameter stubs this run hit, both the exact failure `param_get_int`
was written to prevent (D171 - an out-pointer never written is worse than a wrong return):

- `sceUserServiceGetGamePresets` - called, landed on a stub, **wrote nothing** to its out-buffer
  (`image+0x56d2f60`). Declared and deliberately unimplemented (D346), because the *structure's meaning*
  is unmeasured - but leaving the buffer unwritten hands the guest whatever was there.
- `sceKernelFstat` - called, stubbed, out-param unwritten.

Either could leave the guest with a zero where it expected a constructed object. Not yet proven the
cause - the next step is to trace what writes `array[0].field_0x2d0` and what should construct
`image+0x5b37e98`, and whether one of these stubs sits upstream of it.

## The tooling lesson

The report's fault heuristic guessed a cause from register values (`rax`/`rcx`/`r8` are zero, the access
is `+0x58`, so "find where one was set to zero") and named `scePthreadMutexUnlock`. That is a message
naming a cause the measurement did not establish - the CLAUDE.md §3 failure of reporting more than was
measured. Disassembling the faulting instruction names the *actual* base register (`rax` from `[rdi]`),
which the heuristic cannot. A fault report that disassembles its own fault site would not have pointed
at the mutex. Filed as a tooling note, not fixed here.

## Gate state

No code changed - a characterisation, correcting the run report's guessed cause with the disassembled
truth (the mutex is a red herring; the wall is a null-vtable virtual call on an unconstructed static
object). Diagnostics: `ORBISTOUN_PEEK` + capstone, `ORBISTOUN_DUMP`. `./bin/orbistoun check` green,
worklog index regenerated, identity scan clean. No commit.
