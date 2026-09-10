# D605 - The largest record kind in every report, read by nothing

**Status:** measured
**Date:** 2026-09-08

## What the count did not say

`orbistoun-cli probe` reads an obSCEne report and says how much it read. On the three reports that
arrived this morning it said so correctly, and here is the tally it did not print:

```text
eboot    305 sym   298 measure   264 res   256 try   217 bytes   133 import
payload 2588 measure   264 res   207 try   162 sym   134 import   117 bytes
pkg      433 measure   264 res   252 try   247 sym   229 bytes   133 import
```

`measure` is the most numerous record kind in every one of them, and `orbistoun-probe` had no
`Measure` variant. All 3,319 landed in `Record::Other` - which the protocol permits, which was the
right default while nothing consumed them, and which was invisible.

Nothing here was *wrong*. The record count was right, the section coverage was right, every
symbol fact was right. The report simply had no way to say **"and there is a class of record in
this file I make nothing of"**, so it read as a full account of the file. That is the failure
principle 3 already names one level down: reporting more than the measurement supports.

## What is in them

Nineteen sections in the payload report alone, and the list is worth reading because most of it is
platform ground truth this project currently assumes:

| section | what it measures |
|---|---|
| `120-measure/cache-topology` | cache levels, line sizes |
| `120-measure/cpuid` | processor identification |
| `120-measure/clocks-advance`, `timer-ratio` | how fast each clock actually moves |
| `130-layout/direct-memory-query-flags` | the flags a direct-memory query answers with |
| `135-sysctl/names` | which sysctl names exist and how long their answers are |
| `136-kernel/handoff` | where the kernel puts what it hands a process |
| `138-layout/addresses` | named symbols against their console addresses |
| `015-sync/mutexattr-round-trip` | what a mutex attribute reads back as |
| `140-oracle/kexport-table` | **2,443 kernel exports, hash against address** |

## The fix, and the part that is not a fix

`Record::Measure` parses now, `Transcript::measurements` returns them graded like everything else,
and `Transcript::measured_sections` tallies them per section. The report prints that tally and
marks every section it does **not** interpret:

```text
measurements  2588 in 19 section(s)
     16  120-measure/cache-topology  - carried, not interpreted
     18  136-kernel/handoff  - carried, not interpreted
   2443  140-oracle/kexport-table
```

Eighteen of the nineteen still say *carried, not interpreted*, and that is the honest state. The
change is not that the reader understands them; it is that the reader can no longer imply it does.
The tally is also the work list, ranked by how much evidence each line represents.

## Grading

A measurement is graded exactly as a behaviour is: `Oracle::Measured` only when the operator
asserts the run was on the target, `Oracle::Assumed` otherwise (D246). Nothing about a number being
a number lets it escape that - a cache line size measured on a stand-in describes the stand-in.

**`--is-target` is required, and its absence is easy to misread.** Without it a genuine console run
reports `measured 0`, which looks like a run that established nothing rather than a run whose
grading was never asserted. The reader prints the origin first for this reason; it is worth knowing
before the numbers are read.

## What is not parsed

The value stays a `String`. Most are hexadecimal and some are not, and a reader that guessed would
turn a correct measurement into a wrong one without changing a character of the record.
`Measurement::number` parses on request and answers `None` when it cannot - `0x10` is sixteen, `10`
is ten, and neither is silently the other.

## And it is not the only one

`measure` was the largest unread class, not the only one. What still falls to `Record::Other`
across the three reports:

```text
import  responsive  size  module  progress  region  modtier  display
tally   resume      meta  guard   frontier  end     context
```

`tests/conformance.rs` has always pinned that an unrecognised kind is kept verbatim rather than
dropped, and it named `measure` among the kinds deliberately left alone because no real output
had ever carried one. That reasoning was correct when it was written and is now overtaken by
evidence, so `measure` comes off the list and the rest stay on it - on exactly the same terms,
and each should graduate the same way when material turns up for it.
