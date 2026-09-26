# orbistoun-report

Run reports: the machine-readable contract.

| Type | Holds |
|---|---|
| `RunReport` | a versioned, bounded document describing one run |
| `RunDiff` | the delta against the previous run |
| `CallTrace` | what a run recorded, persisted per module |
| `Conditions` | the settings a run was made under, so a verdict cannot silently measure a settings change |
| `diagnose` | the ranked findings a run prints |

It also holds the report store and its retention. [orbistoun-worker](../orbistoun-worker/)
assembles reports; the CLI, the GUI and `orbistoun-turn` read them.

## Rules

- **Logs are for people; this is what an agent reads.** A consumer that greps log prose breaks
  when a message is reworded, and the log becomes an unversioned API.
- **The diff is the primary output.** One report says what happened; the delta says whether
  the last change helped.
- **First touch as well as frequency.** The first unmet need is usually the cause, and
  everything after it is cascade.
- **Inputs are embedded**, so a difference is not misattributed to the change when it was
  configuration drift.
- **Bounded to kilobytes.** A report is an index with `TOP_N` and `TAIL_N`; the trace is queried
  on demand.
- **Retention has two guards:** 72 hours, and a byte budget. The byte budget is what fires when
  many runs happen inside the age window.
