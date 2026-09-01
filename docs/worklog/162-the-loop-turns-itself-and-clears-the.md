# The loop turns itself, and clears the wall doing it


`turn::plan` had produced steps since it was written and **nothing had ever taken one**. The
sentence "step 17 is now partly mechanical" described code somebody ran by hand. It has a
runner now, and against `PPSA02664` one turn does this unattended:

```
TURN  8 findings, fault=0xfffe0
PLAN  4 steps, 3 of them this can take on its own
  swept every argument: arg0 is an out-parameter, faulting at arg0+0xfffe0
  *** gave it a region at 0x50000000: reached 25 against 23, faulting at 0x0
  stopped: implementing a function is a person writing code
TURN RESULT  took 4 of 4 steps in 7.8s
```

**Seven point eight seconds**, and the second line is the loop satisfying a contract it
measured minutes earlier by hand. The wall that stood through twenty-three eliminations is
behind it - reached 25 imports against 23, and the fault is a different one at a different
address.

### What was built

- `Axis::Watch`, so the watchpoint step is runnable rather than only printable. The trial
  spawns a process and sets environment variables; the only thing missing was a variant.
- `Trial::spawn_axes`, so anything driving an axis can be tested against a mock. A dispatcher
  exercisable only by booting a commercial title has no unit tests.
- `turn::take` and `turn::turn`, and `Taken`, which carries **what** each step produced -
  a runner reporting "done" makes a step that found the answer indistinguishable from one
  that found nothing.
- `Taken::Elsewhere` beside `Taken::Declined`, because "nobody can run this here" and "nobody
  should" are different facts and `NameAHash` is the first.
- An `OutParameter` finding **follows itself through**: reserve, plant, ask. No decision in
  it - the region, the slot and the forced answer all come from the sweep.

### Three bugs found by writing it, and one by running it

- `satisfy` first used `Poke { address: 0 }`. The sweep identifies a **slot**, and where that
  slot points is the guest's business - it had to plant through `Write`.
- It also built `Return { target: "" }`. `Target::matches` is a substring test and every label
  contains the empty string, so that reads as "force every import". It would not have done
  that in practice - both worker parsers reject an empty target outright - so the real failure
  would have been a diagnostic that silently did nothing, which is worse.
- The integration test was a **second copy of the dispatcher**: a match over every step kind,
  in a test, drifting from the one that ships. It calls `turn` now.
- And the first live run reserved `0x1fffc0` - `0xfffe0` doubled, half a page short of
  covering its own last byte. The guest faulted **inside the region it had just been given**,
  at `base + 0xfffe0`, which reads as "the base was not the problem" and is really "the
  reservation stopped forty bytes early". Rounded up to a page; the wall then cleared.

### Duplication removed while looking for it

`Axis::every_variable` was seven hand-written strings and `ORBISTOUN_WATCHPOINT` had never
reached them - so a sweep launched from a shell with a watchpoint set would have carried it
into every run and called the result controlled (D288). Four more copies in `Axis::env`, one
in `orbistoun-llm`, five in tests. All read `orbistoun_env::<VAR>.name` now, which is what
`orbistoun-paths` already did.

Third instance of that shape in one day, after D123's registration lists and D281's dead
`register` functions. The pattern is always the same: a list that looks authoritative, is a
copy, and is wrong only where nobody re-reads.

