# 450. The probe was not calling

**2026-09-08** - directed, continuing 449

## Three new sweeps, and 235 new measurements

The `20260908-160006` batch ingested clean: **462 distinct measurements, 324 constant**, up from
227 at the last commit. Two new sections answer questions this project had been guessing at.

`031-stackattr` settles `sceKernelIsStack`, which worklog 448 changed from a one-argument predicate
to a three-argument status-and-bounds call **on reasoning alone**. The console:

```text
sceKernelIsStack       low  0x7eedfc000   high 0x7eeffc000   is-stack 0x0
scePthreadAttrGet      stack-address 0x7eedfc000   stack-size 0x200000
```

`high - low` is `0x200000`, which is the size the attribute reports, and the address the attribute
gives is the **low** one. So the change was right and is now measured rather than assumed. A fresh
attribute answers stack-address `0x0` and stack-size `0x10000`, which is the constant this crate
already had.

`032-syncaddr` exercises the platform futex for the first time - and **fails on the console**:
`wake-releases-a-waiter` is a fail there, `wait-returns-on-mismatch` a SIGSEGV. Worth knowing
before treating either as a target.

## The differential was counting items and calling them findings

`probe --against` reported *115 passed there and failed here - each is a defect with its own
words*. Ninety-nine of them were one sentence: **none of this library is present**, once per
absent library. That is one fact about this build printed ninety-nine times, on top of the sixteen
that each said something different.

Grouped by what was said, smallest group first, nothing hidden (D624):

```text
115 passed there and failed here, saying 17 distinct thing(s)
  …sixteen one-line findings…
  pass -> fail  x99  none of this library is present
                     900-surface/audioout, 900-surface/corpus_0010_libSceAmpr,
                     900-surface/corpus_0019_libSceAt9Enc, and 96 more
```

## And seven of the sixteen were not ours

Chasing `020-memory/allocate` - "allocation was refused" - through obSCEne's own source found this
in `obs_invoke_syscall`:

```c
if (obs_get_payload_args() == NULL && s_libkernel_syscall_gadget == 0) {
    return -1;
}
```

The payload build does not call the libkernel import. It carries weak definitions that go straight
to a raw syscall - 572 allocate, 603 virtual query, 240 the usleep fallback - and under orbistoun
there is no payload-args struct and no gadget, so **the call is never issued and `-1` comes back**.
`0xffffffff` is `(uint32_t)(-1)`, and five checks record exactly that. Two more follow from the
same declined sleep, and the whole `pass -> skip` cascade beneath them - map, release, unmap, four
relational checks, two layout checks - is one allocation that never happened (D626).

So the memory group was never a memory bug. Filed on the shared bus as `REQ-20260908T1620Z-4c1e`,
asking for "not issued" to be distinguishable from "refused".

## The dump had a third silence in it

D623 fixed two reasons `ORBISTOUN_DUMP` produced nothing. It still produced nothing, and the third
reason was the oldest: **a dump reaches a reader only through a finding about the same import, and
every finding that carries one is about an import nothing implements.** So a dump forced on an
implemented import was taken and then dropped at the last step - which is exactly the case D198
added forcing for.

Now a `Gap::Captured` finding, printed ahead of the ranked six rather than competing with them,
and the arming reported in both directions (D625):

```text
orbistoun: ORBISTOUN_DUMP matched no import called "sceKernelNoSuchThingAtAll"
orbistoun: ORBISTOUN_DUMP armed 2 of 1040 stub slot(s)
! libkernel_fs::sceKernelWrite was asked about, and here is what it was passed
    arg1 = 0x40000022fb9c -> image+0x22fb9c = "obscene: eboot entry reached"
```

Both cases were run before the record was written. The first is an implemented import called four
hundred thousand times, whose arguments this tool could not show until today.

## Surprises

- **My reading in worklog 449 was wrong and is corrected there.** "Every diagnostic keyed on an
  import label is inert for a bare-ELF guest" was not true; the labels resolve fine. A silent drop
  and a guest that never calls the thing look identical from outside, and I took the stronger
  claim.
- **`ORBISTOUN_ENTRY_ARGUMENT=handoff` is much worse for this payload**, not better: 223 imports
  and a clean exit becomes 5 imports and an instruction fetch from `0x0` after 1,731 calls.
  Handing a guest a resolver whose `+0xa` is the middle of a thunk rather than a syscall
  instruction is handing it a wild call. Orbistoun already has the right shape for this in
  `getpid_export_slot` (D400, D407) and it only stands up under a firmware skeleton.
- **libSceAgc is absent on the console obSCEne runs on.** `title/ps4-bc; libSceGnm mapped,
  libSceAgc absent`, and every `166-agc/*` check skips. So the wall's return contract cannot come
  from the current sweep at all. Filed as `REQ-20260908T1621Z-7a5d`, with `not-possible` named as
  an acceptable answer.
- **Six measured facts stopped being constant** with the new sweeps - three mouse vaddrs and three
  input reachability flags - and had to leave the hardware gate's lists. An address that varies
  between runs is not a property of the platform, which is what that half of the gate is for.
- **The frontier moved and had never been written down**: PPSA03416 is 197 imports and faults at
  `image+0x39f7c` now, not 193 and `image+0x1389269`. That is worklog 448's `sceKernelIsStack`
  change, recorded for the first time.

## Next

- Whether the by-name stubs should carry the vendor libkernel stub shape - a syscall gadget at
  byte ten - so a payload's gadget hunt finds a working one. It would reach the seven checks above
  properly rather than by the handoff route that made things worse.
- `102-net/sockaddr-bind`: the console binds with `sin_len` 16 **or** 0 and answers `0x0`; this
  refuses both. A small, measured, in-scope fix.
- Library text is execute-only on the console (`xotext` is zero for all ten input symbols) and
  readable here, which matters to any guest reading a prologue - obSCEne itself does.
- `sceAgcCreateShader`, still the actual wall, now waiting on a question that may have no answer.
