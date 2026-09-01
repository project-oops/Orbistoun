# D290 - Every floating-point function works and every one is reported as missing


**decided** · 2026-08-26 · a first explanation that was wrong, and the real one underneath

One run of the conformance probe says both of these:

- `037-math/sqrt` **passes**, against `spec`, along with twelve more maths checks.
- The run report says `libc::sqrt`, `round`, `pow` and `trunc` were called and **nothing
  implements them**.

The first explanation was `sceKernelDlsym` - unimplemented, called seven times - on the
reasoning that a symbol resolved at run time by name arrives somewhere different from the same
symbol in the import table. **That was wrong, and the probe's own transcript says so**:
`110-modules/symbol` is *skipped*, with the reason *"no module loaded, so there is no handle to
ask"*. The probe never resolved anything through `dlsym`, so nothing reaches maths that way. A
mechanism was asserted that had not been established, which is the failure principle 3 names,
committed while writing the entry that names it.

The real cause is accounting. `orbistoun-thunk` keeps two handler tables - one for functions
answering in `rax`, one for functions answering in `xmm0` (D268) - and `is_implemented` reads
**only the first**. So every floating-point function dispatches correctly, computes the right
answer, passes its conformance check, and is recorded as a call nothing implemented.

Three things follow from it, and the third is why this is a decision rather than a bug fix:

- **Argument dumps are taken for it.** Dumps fire only for imports nothing implements, so
  every maths call has been dumping six integer registers that hold leftovers - a float
  function takes nothing in them.
- **The findings list is wrong**, and it ranks by call count, so the loop has been proposing
  work on functions that were finished.
- **`standing` is understated.** That is the number `PROJECT_STATUS.md` says to read rather
  than the call count - *"the share of calls answered by a real implementation rather than a
  placeholder"* - and it is the project's headline measure of its own progress.

The comment that got this right is one layer up, in `symbols::all()`: *"Both tables. A function
that answers in `xmm0` is as implemented as one that answers in `rax`, and counting only the
first would report the maths library as missing while it worked."* Somebody wrote that
sentence, applied it there, and the layer underneath still asks the shorter question.

