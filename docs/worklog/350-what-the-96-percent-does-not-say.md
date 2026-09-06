# 2026-09-03 - (/loop) What the 96% does not say, a stale red phase, and three more libc functions

```
differential      194  ->  211 cases
roadmap red items   1  ->    0
```

Asked where the gaps are and whether the basics are covered. Measured rather than
characterised, and two of the three answers were not what the headline numbers say.

## The coverage figure is not capability

**674 declared / 647 implemented** is 96%. Per subsystem, against the 65-run corpus:

| | declared | bound | unbound |
|---|---|---|---|
| `libScePad` | 13 | 9 | the four `Read`/`ReadState` functions |
| `libSceAudioOut` | 5 | 2 | `Open`, `Output`, `SetVolume` |
| `libSceGnmDriver` | 6 | 1 | five of six submit/dispatch |

Same shape three times: **the lifecycle is implemented and the data path is not.** A guest can
open a pad, set vibration and the light bar, and cannot read a button - and `scePadReadState` is
519 of the 547 pad calls ever recorded. D500.

Nothing here is *behind*: the roadmap puts the first frame at Phase 6, unstarted, and has an
item literally named "Phase 6's contents, built ahead of it". The decision exists so nobody
reads 96% as "the subsystems work".

**One thing that is a surprise rather than a phase.** orbistoun declares **Gnm**; the corpus
shows guests calling **Agc** - `sceAgcCreateShader`, `sceAgcDriverGetDefaultOwner` and four more,
**none declared anywhere**. Not a stub gap, the wrong library modelled.

## The one red roadmap phase was done

Phase 0d, test corpus tooling, has been 🔴 since it was written. Checked against the four things
it asked for: `orbistoun-cli corpus` has `list`/`sync`/`run`, `corpus/sources.toml` pins **26
assets by hash** across two sources, both carry a licence field, `/titles/*` is gitignored. The
second half - a minimal app built with an open toolchain - is obSCEne, the second source.

**Nothing was built to close it.** Fourth stale work item this week, after `/dev/random`, the
differential's "missing" cases and D488's note about the verdict. Marker moved, index
regenerated; orbistoun has no red roadmap items.

## And the differential gap that was actually left

**194 -> 211 cases**, three functions that had none:

- **`strpbrk`**, beside the `strcspn` cases that ask the same question as a length. They
  disagree in shape exactly at not-found - a length of `strlen` against a null.
- **`strnlen`**, where the unterminated case is the point: an implementation that delegates to
  `strlen` answers correctly and reads out of bounds, which no return comparison sees.
- **`strlcpy`**, whose **return is the contract** - the source length, not the copied length, so
  `returned >= size` is the caller's truncation test. Confirmed: a 4-byte buffer given
  `"abcdefgh"` answers **8**. Same bug class as `snprintf`'s return.

All 211 agree. `strnstr` stays out - glibc has no such function, so there is nothing to diff
against, which is a better reason than "not yet".

**Watched failing after the refactor, not before it.** Splitting the ctype arms into
`run_ctype` changed the dispatch, so the guard was re-broken afterwards: corrupting
`isupper/high-half` gives `returned 0x7fffffe, glibc 2.39 returned 0x1`. Restored in binary
mode this time - the previous restore went through Python's text mode and rewrote the whole file
as CRLF.

## State

`cargo test --workspace` green - 117 suites, **1992 tests**, 0 failures. clippy `--tests` clean
(the additions pushed `run` past the length lint; split out the way `run_search` and
`run_into_buffer` already were). fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-350 and D466-D500.

**Next**: the 95 outstanding hardware measurements. `scePadReadState` and `sceAudioOutOutput`
are blocked on layouts and belong in the obSCEne sweep, not here.
