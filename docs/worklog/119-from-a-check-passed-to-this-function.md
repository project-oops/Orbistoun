# From "a check passed" to "this function returns this, and here is how well we know it"


`res` records name their check and never the function. The `try` emitted before each one
does. Pairing them by check identifier is the step from a report to something the emulator
can act on, and `orbistoun-probe` now does it: `Finding` carries library, symbol, value,
status and a grade already adjusted for what produced it, plus a citation naming the part
and firmware.

`orbistoun probe` prints the facts by name and, separately, the checks that announced a
call and never concluded.

### Surprises

**Pointed at a real report, it immediately found the call that killed the run.**
`040-file/open-rejects-null` announces `sceKernelOpen` and is the **last line in the file**.
The check before it, `open-rejects-missing`, passed. So that emulator opened a missing path
successfully and then died on a null one - which is exactly the failure obSCEne's handover
notes describe, found from the record rather than from a log.

**A test I wrote was too coarse and the fixture said so.** It asserted that a symbol which
did not conclude produces no finding. Wrong: several checks exercise one function, so
`sceKernelOpen` legitimately has a concluded result *and* an unconcluded check. The unit is
the check, never the symbol - and reporting "this symbol did not conclude" would have
contradicted a finding sitting two lines above it. `attempted_without_result` is keyed by
check now, which is also what a reader needs to repeat it.

**The constructed test data says it is constructed.** Two tests build a transcript inline
rather than adding a fixture, because everything in `protocol/` is a real capture and
filing a plausible fake beside them would make a fabrication indistinguishable from
evidence. That is the failure this crate is shaped around; it would be a poor place to
commit it.

