# Every floating-point function worked, and every one was reported missing


Chasing why the probe reported `libc::sqrt` on a stub while `037-math/sqrt` passed against
spec. The first explanation was `sceKernelDlsym` - unimplemented, seven calls - on the theory
that a run-time lookup resolves somewhere different from the import table. **Wrong**, and the
probe's own transcript said so: `110-modules/symbol` is *skipped*, "no module loaded, so there
is no handle to ask". Nothing reaches maths through `dlsym`. A mechanism asserted without
being established, in the entry that names that failure.

The real cause: `orbistoun-thunk` keeps two handler tables - `rax` and `xmm0` - and
`is_implemented` read **only the first**. So every maths function dispatched correctly,
computed the right answer, passed its check, and was recorded as a call nothing implemented.

Measured before and after, same probe, same budget:

| | before | after |
|---|---|---|
| unimplemented findings | `round`, `sqrt`, `pow`, `trunc` + 6 real | **6, all real** |
| `standing` | understated | **81,524 of 81,559 calls, 0% on stubs** |

`standing` is the number `PROJECT_STATUS.md` tells a reader to use instead of the call count -
*"the share of calls answered by a real implementation rather than a placeholder"*. It has
been wrong in the pessimistic direction since floating-point dispatch was added.

Two consequences beyond the number. Argument dumps fire only for imports nothing implements,
so every maths call was dumping six integer registers that hold **leftovers** - a float
function takes nothing in them, which is why those dumps read as nonsense. And the findings
list ranks by call count, so the loop had been proposing work on functions that were finished.

The comment that got this right is one layer up, in `symbols::all()`: *"Both tables. A function
that answers in `xmm0` is as implemented as one that answers in `rax`, and counting only the
first would report the maths library as missing while it worked."* Somebody wrote that
sentence, applied it there, and the layer underneath still asked the shorter question.

