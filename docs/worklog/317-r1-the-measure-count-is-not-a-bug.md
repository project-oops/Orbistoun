# 2026-09-02 - (/loop) R1: the measure count is not a bug, and the name oracle already answered

```
130 obs_report_measure sites in the probe   ->   34 measure records in the hardware run
```

That ratio was read as under-firing and set as the tick's first task: work out which sites did
not fire and why. **They all have a reason, none of them is a fault, and one of the reasons is
worth more than the question was.**

Analysis only - no console, no probe change, nothing written to either repository's code.

## Where the 130 sites went

Cross-referenced each site's check id against the check ids the 2026-08-30 capture actually
ran, which the `try`/`res` records name:

| | sites |
|---|---|
| in checks the capture never ran - written since | **102** |
| the oracle answered no, so the census had nothing to walk | 2 |
| the platform accepted every size, so the rejection branch never ran | 2 |
| built without `OBS_BULK` | 1 |
| a conditional branch not taken on a check that passed | 1 |
| in checks that produced the 34 records | 21 |

**Seventy-eight per cent of the measurement surface postdates the capture.** `035-libc` is the
clearest case: 33 sites, all in `getpctype`, `fpu-environment` and `wctype`, none of which
existed on 2026-08-30 - the section ran 30 checks that day and passed 29 of them.

So the honest reading is the opposite of the premise. The probe has roughly quintupled what it
would measure, and **the next hardware run is worth about five times the last one** rather than
the last one having gone wrong.

It also fixes the ceiling on R5: **34 measure records is all there is**, and no amount of
reading makes it more. What is ingestible now is the 294 valued `res` records, at whatever tier
their check-name conditions honestly support.

## The find: the name oracle was answered two days ago

`140-oracle/resolve-by-name` is the check behind the highest-leverage idea either plan carries -
if the platform resolves symbols by name, naming stops being a dictionary attack over ~249k
candidates and becomes one yes/no per candidate. Both plans list it as "needs console time".

**It ran on 2026-08-30 and the record has been sitting on disk since:**

```text
OBS|res|140-oracle/resolve-by-name|skip||nothing resolves by name, including a symbol known to be present|assumed
OBS|res|140-oracle/resolve-census|skip||the oracle does not answer, so there is nothing to walk|assumed
```

Six `OBS|resolve` records show every candidate absent at `0x0`, including `sceKernelWrite`,
which the probe imports and the loader had already resolved. The check carries two controls -
one symbol that must resolve and one name that cannot exist - and the negative control also
came back absent, so this is a real no rather than a function that says no to everything
including questions it should answer.

### But the question is narrower than "no", and the probe is already ready for it

The call used `OBS_HANDLE_SELF`, which is zero, and the source says in as many words that a null
handle "may well be refused, which is a reportable answer rather than a failure of the check".
So what was measured is: **`sceKernelDlsym` with a null handle resolves nothing.** Whether a
*real* module handle behaves differently was not asked that day.

It is asked now. obSCEne added `060-module/dlsym-resolves-known-symbol` on 2026-09-01 - opens
`libScePad`, resolves `scePadOpen` through a valid handle, and requires the address be
**callable** rather than merely non-null. Written, unrun, and one of the 102.

**Nothing needed writing here**, which is the whole point of having looked: the follow-up probe
was already built, and a tick that had started coding instead of reading would have written it
twice.

## What this changes

- **R2 is not "unknown, needs a console".** It is: the null-handle form is answered no, the
  real-handle form is written and waiting, and that is a much smaller question.
- **R1 is closed** without a console and without a fix, because there was nothing to fix.
- The one genuinely actionable item is a *run configuration* rather than code: `910-bulk/probe`
  skipped because the binary was built without `OBS_BULK`, so a full sweep needs that build.
- **R4 gains its clearest example.** `dlsym-resolves-known-symbol` answers pass or fail, not a
  measurement, so even when it runs, orbistoun cannot ingest what it learned - only a person
  reading the report can. That is the record-shape problem exactly.

## State

No code changed in either repository. `cargo test --workspace` green as left by worklog 316 -
115 suites, 1921 tests. Nothing committed; it is past 17:00 UK and the day still holds worklogs
292-317 and D466-D477.

**Next**: R5 - join `try`+`res` on their shared check id and ingest the 294 valued records at
the tier their conditions support. No probe change, no console.
