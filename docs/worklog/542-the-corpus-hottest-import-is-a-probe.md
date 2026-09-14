# 542. The corpus's hottest import is a probe artefact, and `worklist` cannot say so

**2026-09-14** - closing the `sceKernelDlsym` question, and finding a worse one behind it

## The question

`orbistoun-cli questions` ranks `libkernel::sceKernelDlsym` first by a factor of two: **880,718 calls,
27% of every call in the corpus**, and `[shape unrecorded]`. A correct `dlsym` is called once per
symbol, so that number reads like a loop - and 27% of all guest work would be the largest
hardware-free lever available.

## The answer: it is not a defect, and it is not retail

Traces persist per guest under the platform trace directory, and `worklist` aggregates them. Split
by guest:

| guest | dlsym calls | total calls | share |
|---|---:|---:|---:|
| `PPSA99980` (obSCEne's own eboot) | 285,777 | 419,005 | **68%** |
| `obscene-payload` (build) | 279,878 | 394,637 | **70%** |
| `obscene-payload` (eboot) | 279,544 | 432,217 | **64%** |
| `obscene` (eboot) | 35,465 | 280,213 | 12% |
| `PPSA25872-app0` | **1** | 321,962 | 0% |
| `PPSA03416-app0` | **1** | 470,416 | 0% |
| `PPSA02664-app0` | **1** | 418,420 | 0% |

**Every retail title calls `sceKernelDlsym` exactly once.** The 880k is obSCEne's own guests
resolving their surface by name, which is precisely what a conformance probe does. orbistoun's
`dlsym` is not looping and there is nothing here to fix.

## The worse question behind it

`worklist` is documented as "rank what to implement next". Its top row is **an artefact of the corpus
being three obSCEne guests out of twelve**, and nothing in its output says so. A reader taking it at
face value would spend the day on a function retail titles call once.

The skew is not small. Sorted by call volume the corpus is dominated by whichever guest ran longest,
and obSCEne's probes are built to exercise everything - that is their job - so they will always
out-call a title that dies early. The more useful orbistoun's probe guests get, the more they drown
out the titles the project exists to run.

This is the same failure principle 3 names one level up: **a report that is confidently wrong about
what it measured.** It is not lying about the counts; it is letting the reader believe the ranking
answers a question it does not.

**Proposed, not built:** `worklist` should separate the project's own probe guests from retail
titles, either as a flag or by reporting both columns. The judgement it needs - *which guests are
ours* - is a real one and not purely mechanical: `obscene` and `obscene-payload` are obvious from
their names, and `PPSA99980` is obSCEne's homebrew wearing a retail-shaped title id, so a filename
rule alone gets it wrong. That is worth a decision entry rather than a quick predicate.

## Two other paths, closed for now

**Naming the unnamed imports: a clean miss.** `orbistoun-cli names titles/` tried 584,345 module
strings, 3,018 published names, **3.9 billion generated candidates** across 11 patterns, and 1.1
million affixed forms. It named **0 of 2,932**. The tool's own note - that no published standard name
matching can indicate a wrong hash suffix - does not apply here, since 30,184 names already resolve
against that suffix. The vocabulary simply does not contain these; extending it is the method, and
that is its own project.

**The two unimplemented syscalls** (number `1`, and `601` with first argument `0x7`) remain open. The
dispatch table takes its refusal value by injection (D378) and is small enough to extend; nothing was
changed here because the investigation above consumed the tick.

## Surprises

**The attribution data existed and I nearly concluded it did not.** Per-title import counts are not
in the compat records and the persisted-reports directory is empty, which read as "this is not
recoverable without instrumenting a run". It was recoverable: `worklist` aggregates JSON traces that
survive every run, one file per guest. One `grep` in the CLI source for the line it prints found the
source in a single step, after four commands looking in the wrong places. **Read what the tool reads
before deciding the data is gone.**
