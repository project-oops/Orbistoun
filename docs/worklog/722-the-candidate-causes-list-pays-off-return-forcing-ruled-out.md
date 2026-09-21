# 722. The candidate-causes list pays off; return-forcing the graphics stubs is ruled out

**2026-09-19** — first PPSA02664 (Alex Kidd in Miracle World) tick under the new tooling (worklog
721). The candidate-causes block did its job immediately: instead of hand-digging, the fault report
named every placeholder the run rested on - `0x7d86501b8094ef57`, `sceAgcDriverAddEqEvent`,
`sceAgcDriverSetHsOffchipParam`, `sceAgcDriverSetTFRing`, the two `libSceAmpr` constructors,
`sceAmprCommandBufferSetBuffer`, and more. This tick tested the cheapest hypothesis against them.

## The hypothesis, and why it is ruled out

*Does Unity take the faulting path because orbistoun hands it an error-looking placeholder from these
functions, and would a plausible success let it through?* Tested by forcing the four named/unnamed
GPU-driver candidates to `0x0` (a sanctioned intervention, verdict caveated, not recorded - D227):

`ORBISTOUN_RETURN=<AddEqEvent>:0,<SetHsOffchipParam>:0,<SetTFRing>:0,<0x53bbd82b51d172db>:0`

Result: **BACK.** 154 imports (−68), 410,893 calls (−7,453), and a *new, earlier* fault -
`read of 0xf7ff0039` at `image+0x3ac99`, "one of our own placeholder codes, used as an address".
So Unity **does** consume these returns (forcing them changed the path), but `0` is the wrong value:
it steered the guest into a different unimplemented function whose placeholder it then dereferenced.
The lesson is D708's, one level down - guessing a return is the same plausible-output move as
inventing a layout. These functions need their **real measured returns**, not a convenient one.

## Disposition

The wall is orbistoun's (D708, PPSA02664 renders on hardware), and the accurate levers all need
hardware data orbistoun does not yet have:

- the two `libSceAmpr` constructors - obSCEne `-af31`, filed, pending;
- the two `libSceAgcDriver` set-functions (`SetTFRing`, `SetHsOffchipParam`) - obSCEne request filed
  this tick;
- `0x7d86501b8094ef57` itself - a non-export whose write semantics no oracle can supply, the
  standing block (worklogs 716-719).

Return-forcing being ruled out, there is no honest local move left on this wall until a measurement
lands. The loop's next useful step is whichever obSCEne answer arrives first, or building GPU
execution (roadmap phase 6) far enough that the command-buffer builders can be exercised rather than
guessed.

## Gate state

No code changed - a name lookup, one intervention run, and a reading. `./bin/orbistoun check` was
green as of worklog 721. Identity scan clean.
