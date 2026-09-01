# orbistoun-report

Run reports: the machine-readable contract.

**Models:** `RunReport` (a versioned, bounded document), `RunDiff` (the delta against
the previous run), a report store and its retention, `CallTrace` (what a run recorded,
persisted per module), `Conditions` (the settings it was made under, so a verdict cannot
silently measure a settings change), and `diagnose` (the ranked findings a run prints).

**Deliberately fakes:** nothing.

**Design note.** Logs are for humans; **this is what an agent reads**. If a consumer
greps log prose, rewording a message silently breaks it and the log becomes an
unversioned API nobody knows they are maintaining.

Every choice follows from the reader having no memory of the session that produced the
code:

- **The diff is the most important output.** One report says what happened; the delta
  says whether the last change helped.
- **First-touch as well as frequency** - the first unmet need is usually the cause,
  and everything after it is cascade.
- **Inputs are embedded**, so a difference cannot be misattributed to the change when
  it was really config drift.
- **Bounded to kilobytes.** A finite context cannot read a multi-gigabyte trace, so
  this is an index with `TOP_N` and `TAIL_N`, and the trace is queried on demand.

Retention has two guards because either alone fails: 72 hours, and a byte budget -
the latter is the one that actually fires when an agent does hundreds of runs inside
the age window.

**Status:** complete, and the busiest crate in the loop - the trace, the progress
verdict, the conditions a run was made under, the fault detail, and the ranked findings
all live here.
