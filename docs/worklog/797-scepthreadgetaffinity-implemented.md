# 797. `scePthreadGetaffinity` is implemented — the read D523 anticipated, now that PPSA04263 asks for it; standing rises, the wall does not move, and that is the honest shape of the hardening pivot

**2026-09-22** — worklog 796 shifted the loop from wall-moving (exhausted, tool-bound) to honest
hardening: real exports the titles call that orbistoun answers with a placeholder. This is the first,
and it is exactly the read `scePthreadSetaffinity`'s own note flagged as due (D523).

## The implementation

`scePthreadGetaffinity(thread, mask)` answers from the per-thread record orbistoun already keeps. It
writes the thread's `requested_affinity` - the mask captured when the thread was created - to the
out-parameter and returns `0`. It returns **what was asked**, not the effective mask the policy produced,
because the effective mask under the default `Observe` policy is always zero (D150) and handing that back
would tell a guest its own request was discarded. A zero mask is the guest's convention for "anywhere"
(`Affinity::is_unset`), so a thread that pinned nothing, or a handle orbistoun holds no record for (the
main thread, or one made before this ran), reads back zero rather than an error code - a wrong error here
would read as a scheduling failure to a guest testing the return, the D523 hazard one call over. A null
out-parameter is refused. A unit test pins all three cases, the middle one watched failing.

`known_by = assumed`: the shape is the `pthread_getaffinity_np` analogue, but the specific answer -
return the recorded request, zero-means-anywhere - is orbistoun's model, not a hardware measurement. The
running-thread `scePthreadSetaffinity` still drops (D523), so a set-then-get on a live thread reads the
creation-time value; that is the documented limit, not a new one.

## Measured: standing up, wall unchanged - as expected

PPSA04263 now answers `scePthreadGetaffinity` from an implementation rather than a placeholder: **5
functions on stubs, down from 6**, and the run reaches its usual `image+0x19676d7` at the same place
(verdict `same`). That is the honest shape of hardening a tool-bound title: the placeholder-lie is gone
and the guest gets a real answer, but the wall is the un-run static constructor (worklog 794-796,
computed-dispatched), which no affinity answer reaches. The value is fidelity, not a moved wall, and the
run says exactly that rather than dressing the standing bump as progress on the wall.

## The pivot, working

This is what the loop does now while the tool-bound walls wait on an enabling build or an obSCEne
measurement: turn placeholder-lies into honest answers, one real export at a time, picking each from what
a title actually calls and what published semantics or orbistoun's own model can answer without a guess
dressed as a fact. `scePthreadGetaffinity` was the cleanest (the record already existed); the next come
from the same per-title stub lists.

## Gate state

Code changed: `orbistoun-kernel` implements `scePthreadGetaffinity` (arity 2, answered from the thread
record), wired into the arity table and `implementations()`, with the `libkernel` knowledge entry
upgraded from bare to behavioural (`known_by = assumed`) and a unit test. PPSA04263 standing rises 6→5
stubs, wall unchanged. `./bin/orbistoun check` green, generated docs regenerated, worklog index
regenerated, identity scan clean. No commit.
