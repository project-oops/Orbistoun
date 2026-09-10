# D622 - The probe runs here too

**Status:** measured
**Date:** 2026-09-08

## The oracle that was sitting there

obSCEne is the only guest whose source this project holds. It passes on a console. And its payload
build **runs under orbistoun**, emitting the same `OBS|` record stream the hardware reports carry.

So the same binary can be run in both places and its own verdicts compared. A check that passes
on the console and fails here is not a wall to be reverse-engineered - it is a **named, sourced
defect with the probe's own sentence attached**.

That is a different kind of evidence from everything else available. A title's wall says *where*
it stopped. This says *what is wrong*, in the words of the thing that tried it.

## One command

```bash
orbistoun-cli probe <local transcript> --against <hardware transcript>
```

```text
against …-pkg.obs.log: 256 of 635 check(s) both ran concluded differently
  115 passed there and failed here - each is a defect with its own words
  pass -> fail  020-memory/allocate            allocation was refused
  pass -> fail  020-memory/virtual-query-text  virtual query on code address refused
  pass -> fail  050-time/usleep                a short sleep was refused
  pass -> fail  120-measure/sleep-fidelity     a sleep returned far sooner than asked
  pass -> fail  102-net/sockaddr-bind          bind refused both sockaddr lengths
  pass -> fail  900-surface/control            a symbol that does not exist reported present;
                                               every count in this section is meaningless
```

**Only checks both sides ran are compared.** A check one side skipped is a difference in what was
reachable, not in behaviour, and listing it as a defect buries the ones that are (principle 3).
Ordered so that *passed there, failed here* comes first, because that is the one class that is
unambiguously ours.

## What it says immediately

`900-surface/control` is the probe telling us `sceKernelDlsym` reports a symbol present that does
not exist - the divergence `tests/dlsym_divergence.rs` already records - and that it invalidates
**every count in that section**. That is a defect reporting its own blast radius, which no wall
has ever done.

The memory and sleep entries are core and in scope: one refused `sceKernelAllocateDirectMemory`
cascades into six skipped checks, and `a sleep returned far sooner than asked` sits next to
D614's early-timeout fix in a way that wants looking at.

## The payload is in the corpus now

`titles/obscene-payload` reaches **223 distinct imports at 100% standing and exits cleanly** -
more of the interface than any title, and the only guest that terminates rather than crashing or
timing out.

## And the eboot no longer loads

Today's `eboot.bin` is refused: `no usable dynamic table: lacks a string table, symbol table, or
hash table`. The corpus copy is a week old and loads. The builds differ:

| | 1 September | today |
|---|---|---|
| program headers | 6 | 5 |
| vendor segments | 1 | 2 |
| mapped segments | none | 0, 1, 2 |
| SDK version | `0x00000000` | `0x08050001` |
| proc-param pointers | set | all zero |

So the corpus has been measuring a week-old probe without anything saying so. Recorded here rather
than fixed, because what changed in the container is a reading job of its own.

## A local transcript carries every record twice

obSCEne writes each line to descriptor 1 **and** descriptor 2, deliberately: *"one emulator's
stdout is not the handle a parent process redirects - the write succeeds and the bytes are not
reachable - so the same line goes to stderr as well."* Orbistoun sends both to the host's error
stream, equally deliberately, because its standard output carries the worker protocol.

Both are right, and the result is that a locally captured transcript has every record twice.

`diverges_from` is unaffected - `verdicts` keys by check, so a repeat is an overwrite. **Raw
counts are not**: the payload run's real figures are 162 pass and 378 fail, and the 324 and 756
first read off it were the doubled ones. Anything counting records out of a local transcript has
to know this, which is why it is written down here rather than quietly deduplicated - a hardware
transcript may legitimately run a check twice, and silently collapsing that would lose a real
observation to tidy up an artefact of two descriptors.
