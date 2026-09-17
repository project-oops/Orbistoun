# 606. The null-deref finding routes to the call that answered the pointer - as a lead, not a verdict

**2026-09-15** - the last of the fault classes worklog 605 left half-taxonomised, and the one that
nearly re-made the mistake this whole thread is about

## What it does

A null dereference used to end at `>> the null base is likely rax (=0x0) - ... so find where rax was
set to zero`, which is an *instruction to go searching*, not an answer. The value the guest
dereferenced was answered by some call - its return lands in `rax`, which the guest carried to the
`[rax + offset]` that faulted. So the finding now names that call:

```
>> libc::__cxa_guard_release answered 0x0 immediately before, and the guest dereferenced that
   value here without checking it
-> the guest used libc::__cxa_guard_release's answer as a pointer without checking it. If it
   should answer a pointer here, it is the gap - implement or fix it; if its answer is correct,
   the guest reached this path from an earlier wrong value, so read the calls further back
```

That is ASTRO BOT (PPSA21564), whose null the report previously left as "read the calls just before
it". Now it names the call and states both readings.

## The mistake it nearly made, and how it was caught

The first version matched **any recent call that returned the base value**. That pointed ASTRO
BOT's null at `__cxa_guard_release` - which was *four calls back*, while the immediately preceding
calls were `strcmp`s. A call that far back returning zero is coincidence, not evidence. Matching it
would have been a confident wrong lead - the exact failure this whole line of work exists to remove.

Corrected to the only sound link: the **immediately preceding call** on the faulting thread. Its
return is what is in `rax` by the calling convention, so only it can be the value a dereference of
`rax` carried. A call before it is not evidence and is not named.

## The second mistake, subtler, and how it was handled

With the strict version, `__cxa_guard_release` *is* the immediately preceding call on the faulting
thread (the `strcmp`s were on another), and it returned zero - so the link is real. But
`__cxa_guard_release` is **implemented and returns zero correctly** (it is `void`). So the first
wording - *"implement it, or fix what it returns"* - was a confident wrong **verdict** even though
the observation was sound.

The fix is to separate the observation from the conclusion. The finding now states the sound fact -
this call answered the dereferenced value, and the guest used it without checking - and then gives
**both readings**: the call is the gap (if it should have answered a pointer), *or* its answer is
correct and the guest reached this path from an earlier wrong value, so read further back. For
`__cxa_guard_release` a reader sees at once it is `void` and follows the second reading; for a real
case - an unimplemented `sceKernel*` that answered zero where a handle belonged - the first reading
is the gap. The finding does not decide which; it hands over the sound observation and the two ways
it can go.

This is the module's own rule applied to itself: *a confidently wrong suggestion is worse than no
suggestion* (D179). A routed lead that names the wrong function as broken would be exactly that.

## What is sound about it

- **Only the immediately preceding call**, so the calling-convention link to `rax` holds.
- **Only when its return is at or just below the faulting base** - zero for a null, the address
  itself for a wild pointer - so an unrelated answer routes nothing.
- **Stated as observation plus both readings**, never a verdict, so an implemented function
  answering zero correctly is not accused.

When the last call answered something else, the null came from further back than one call - stored
earlier, or loaded from memory - and the finding falls back to the general search, blaming no one.

## Made to fail

`a_null_dereference_names_the_call_that_answered_zero`: a fault whose immediately preceding call
answered the base routes to that call by name; a fault whose preceding call answered something
unrelated falls back to the search and names no one. Disabling the routing fails it with the old
generic-search string.

## The taxonomy, now

Every fault class names itself and its next step:

- **kernel entry / guest trap** - worklog 605.
- **placeholder-as-pointer** - `Gap::ErrorUsedAsPointer`, routes to the stub that answered it.
- **emulator's own code** - `EMULATOR BUG`.
- **null-deref / bad pointer** - this unit: routes to the supplying call as a lead with both
  readings.

The request that began this - "make the fault give us the answer, not a person" - is met for the
classes that wall the corpus. What it deliberately does **not** do is manufacture certainty the
trace cannot support: a null from an implemented function's correct zero is a lead to check, and the
finding says exactly that rather than a name to blame.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,345 pass / 0 fail, worklogs unique, identity scan clean. Verified on the
live PPSA21564 run: the finding names `__cxa_guard_release` and gives both readings.
