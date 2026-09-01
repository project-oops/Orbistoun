# 2026-08-24 - PROJECT_STATUS.md rewritten, because it opened with a falsehood


The first document a contributor reads began "Runs nothing. Nothing executes guest code at
all yet - the address space is unimplemented and there is no loader past the survey stage",
then contradicted itself two lines later. It also claimed the address-space mapping was not
written, that nothing recorded into the trace, and listed most of the roadmap's completed
phases as gaps.

Rewritten from measured figures rather than memory - and **I put a wrong number in it on the
first pass**, writing 246 re-derivable names where the audit says 280. Caught by checking the
claim against the tool immediately after writing it, which is the only reason it is not still
there. An accuracy document is exactly where an unchecked number does the most harm.

The audit turns out to report **three** tiers, not two: 280 names re-derivable from this
repository, 137 read out of guest modules' own bytes - ours, checkable by anyone holding
that title, not reproducible without it - and 219 unaccounted. The middle tier only exists
because `Method::Observed` carries how it was found (D193), and it is the honest number to
watch: it grows as titles are read and shrinks as the grammar learns to spell what they
contained (D195).

Also recorded `sceAgcCreateShader`'s argument convention, derived from dumps: an
out-parameter, a header beginning `31 32 33 34` with a size field, and the bytecode. Put in
the knowledge file rather than implemented, because `libSceAgc` is declared in the GPU
thread's crate and that is theirs to touch.

