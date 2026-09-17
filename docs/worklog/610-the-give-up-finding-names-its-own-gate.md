# 610. The give-up finding names the call the guest gated on, so the reader stops blaming the far one

**2026-09-15** - the abort taxonomy now derives, mechanically, the same gate D677 read by hand -
and the same one this project misread on its way there

## What I set out to look at

"Earthion's texture null" - PPSA28061's `image+0x43c4`, a read of `0x0` after ten textures load
(backlog 019). Looking at it turned up two things: the texture null is not the live wall, and the
finding that describes the live wall was quietly inviting the wrong conclusion.

## The texture null is downstream of a value nothing measured

The record for PPSA28061 is `47 imports / image+0x43c4`, measured **2026-08-23** with
`default_return = unimplemented` and zero overrides - three weeks before `sceKernelMapperGetParam`
was implemented at all. The current, more faithful build returns the measured `0x8002_0006` from
that call (commit 647952e), and the guest's own code - disassembled in worklog 516 -
`test eax,eax; jne abort` turns any non-zero into an abort. So the honest floor is **22 imports /
391 calls**, and `image+0x43c4` is only reachable past `ORBISTOUN_RETURN=sceKernelMapperGetParam:0x0`,
a success over a 56-byte structure nothing has measured. D677 already decided this: keep the measured
return, treat the abort as the thing to understand. The remaining unknown is a retail datum obSCEne
owns (a2f9). **Nothing to implement on the wall itself**, and re-running confirmed 22/391 live - the
run report says so plainly ("below the best ever recorded ... time is not the explanation").

So the "texture null" is a wall the record remembers from a less faithful build, not one this build
reaches. That is honest failure working as designed: being more correct reached less far.

## The finding was inviting the exact misread I made

The live abort finding listed the four calls before the stop, flat, with the advice "read the calls
immediately before it ... that call is the gap." Its evidence, in order:

```
just before: libc::abort(0xbe9c) from 0x480000a1c7ad
just before: libkernel::sceKernelMapperGetParam(...) -> 0x80020006 from 0x480000a1c760
just before: libSceAgcDriver::sceAgcDriverRegisterOwner(...) -> 0x8a6c9018 from 0x40000012747c
just before: libSceAgc::sceAgcCreateShader(...) -> 0x0 from 0x40000012741e
```

Reading that, this project (me, last session) blamed `sceAgcCreateShader -> 0x0` - a `-> 0x0` two
frames back reads like a null a caller choked on. It is the wrong call: it is in `eboot.bin`
(`0x4000...`), a gigabyte from the abort in `game.bin` (`0x4800...`), and the guest returned from it
fine. The gate is the mapper, `0x4d` bytes below the abort in the same function - which D677 had to
establish by hand disassembly precisely because the finding did not name it.

## What changed: the gate is derived, not left to the reader

`gave_up_gate` in `diagnose.rs` picks the call the giving-up code made **itself**: the one on the
aborting thread whose call site is closest *below* where the stop was decided. The distance is the
discriminator and it is stark - the gate sits `0x4d` back, and the calls before it sit gigabytes
away in another module. The finding now leads with it and carries the delta:

```
its own last call before stopping: libkernel::sceKernelMapperGetParam(...) -> 0x80020006
  from 0x480000a1c760 (0x4d before it stopped)
```

That is D677's finding, produced mechanically from the trace orbistoun already had. The gate is also
the finding's `subject`, so a consumer routes to `sceKernelMapperGetParam` without parsing prose.

It reports the delta rather than asserting a cause: `0x4d` reads as the same function, a gigabyte
reads as "not this decision". The number is the evidence and the reader judges - which is what keeps
this from being a new confident-wrong lead of its own.

## Why closest-below and not most-recent

Because the most recent call before an abort is often part of the **giving-up ritual**, not its
cause. Earthion's own abort path calls `sceErrorDialogInitialize` - a call into another module - after
the gate and before `abort`. "The last thing it called" names the dialog; only "closest below the
stop" names the mapper. The made-to-fail test encodes exactly this shape (gate, then a far dialog,
then abort) and the mutation to most-recent fails it, naming the dialog at delta `0x800008f538f`
against the mapper's `0x4d`. The negative case is pinned too: a stop whose only earlier call is on
another thread names no gate rather than reaching for an unrelated one - the discipline the null-deref
router learned when it blamed a call four frames back (worklog 606).

## Made to fail

- `the_gate_named_is_the_giving_up_codes_own_last_call_not_a_far_one` - the mapper is named, not the
  more-recent cross-module dialog; verified by mutating the picker to most-recent and watching it
  fail with the dialog and its gigabyte delta.
- `a_give_up_with_nothing_called_below_it_names_no_gate` - a cross-thread-only history invents no
  gate and falls back to the original advice.
- `giving_up_outranks_everything_else_and_carries_its_last_calls` - unchanged and still green: a
  single-call tail has nothing below the stop, so the gate line is absent and the old shape holds.

## What I did not do

Rewrite the record. The `47` was honestly measured on an older build; the frontier is best-ever by
design and the run report already flags that the current build reaches less. Overwriting it downward
would be a change to what a record *means*, which is a decision to raise, not to make in passing -
noted here so it is not lost. No obSCEne request: a2f9 already carries the one open datum (whether the
mapper can return `0`, and the four qwords it fills).

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` green, worklogs unique, identity scan clean. Verified live: PPSA28061 now
prints the mapper gate at `0x4d` as its own last call, and the `-> 0x0` shader call sits below it as
the ordinary history it is.
