# D225 - Asking the probe is a live oracle, and the answer travels with its caveat


**Status:** decided (2026-08-24)

When this emulator cannot say what a function does, a probe can be asked. That is the
capability the probe work exists for, and everything built before this - the reader, the
grading, the corpus - was bookkeeping about answers nobody could yet obtain.

**Not "the console".** This entry said that throughout when it was written, and it was
wrong rather than merely informal: obSCEne runs wherever somebody put it - target
hardware, a stand-in, another emulator - and a probe cannot certify its own machine, which
is why [`Origin`] is operator-asserted in the first place. Naming the far end by a device
asserts the one thing the wire does not establish. It is *the probe* everywhere below,
and the window says the same (2026-08-25).

### Ask, record, and return unless it is a handle

The rule, in a sentence: **ask; record the answer with its caveat; return it unless it is a
handle.**

**Ask live.** The alternative is a stub returning `Unimplemented`, which is *certainly* not
what the real implementation does. A real measured value under slightly different state is
very likely closer, and the grade says where it came from. The comparison is not "correct versus wrong"
but "probably right, honestly labelled" versus "definitely wrong".

**Record with the divergence stated.** `known_by: measured` reads as fully trustworthy to
whoever finds it later, and *measured through a probe, in the probe's state rather than
the guest's* is meaningfully weaker. That caveat goes in `assumptions`, travelling with the
fact, exactly as the stand-in demotion already does. No new grade - a grade that needed a
footnote would be a grade nobody could compare.

**Return it, except for handles.** A function returning a handle or a pointer hands back a
value from the *probe's* address space, meaningless in this one. The guest dereferences
it and dies somewhere unrelated hours later - certainly wrong, and it looks right, which is
the one failure this project has no cheap detector for. `Returns` already exists to make
exactly this distinction: it is why an error code is correct for a status function and a
wild pointer for a handle one (D125). So: pass through for status-like, record but do not
return for handle-like.

### Why not "does this function look pure"

That was the first shape and it was worse. Purity is a per-function judgement nobody can
make reliably in advance, and getting it wrong is silent. Keying on the return *kind* is a
property already recorded, already load-bearing, and checkable.

### And the guest tells us if we are wrong

Not universally - a wrong value absorbed by a branch that never checks it is exactly the
failure principle 3 is about. But this project has a progress oracle: if answers from the
probe move a title `FURTHER`, that is evidence; `BACK` points at the change that caused it.
Combined with the handle carve-out, the residual risk is a status code that is wrong in a
way the guest ignores, which is the same risk every assumed stub already carries and is
recorded the same way.

### What was built for it

`hello` now carries the session secret as an appended fourth field, and `unauthorised` is a
named refusal - a wrong or stale key is a clean refusal rather than a broken wire.
`orbistoun ask` puts one question to a probe and prints the answer without interpreting it.
And the GUI has a probe console: address, key, a command line, and a log that renders a
death as a death.

The automatic path - orbistoun asking on its own behalf when it meets an unimplemented
function - is not built, and is blocked on `resolve` rather than on this design. Without
name-to-address there is nothing to ask about by name, and without `write` only
integer-argument functions can be asked at all.
