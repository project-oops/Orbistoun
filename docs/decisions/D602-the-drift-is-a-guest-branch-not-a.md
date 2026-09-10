# D602 - The drift is a guest branch, not a mapping we refused - NO, it is a refusal

**Status:** measured, and its conclusion is corrected inside
**Date:** 2026-09-08

## Localised to one mapping, and then to one branch

PPSA03416 alternates between reaching 192 and 193 distinct imports, and the mapping record
(D581) puts the whole difference in one place:

```text
0  call 219   0x740000000000 +0x100000  arena      during sceKernelReserveVirtualRange
1  call 226   0x740000000000 +0x100000  asked-for  during sceKernelMapDirectMemory     <- only sometimes
```

Across four runs the correlation is exact: **the mapping at call 226 is present ⟺ 192 imports;
absent ⟺ 193.** The reservation-failure addresses differ between the two outcomes by exactly the
size of that mapping, so those are a consequence rather than a cause.

## Which needed the half the record was not keeping

A missing mapping has two readings - orbistoun refused it, or the guest never asked - and the
record kept **successes only**, so they were indistinguishable. That is the same asymmetry D578
closed for the filesystem and D581 closed for reservation failures, surviving one level down in
the record built to close it.

Refusals are recorded now, beside the placements, with the reason:

```text
… REFUSED: <why>
```

**That reading was wrong, and the measurement behind it was empty.** It rested on runs captured
with `2>&1 > file`, which sends the error stream to the terminal rather than to the file - so the
grep for refusals searched output that contained none of them. Captured correctly, every run
records two to four refusals and each answers `0x7fff0004`, which is `NoMemory`: the map **is**
attempted and orbistoun declines it, because the reservation conflicts (D603).

So the divergence is not a guest branch. It is how many of those conflicts happen, which varies
run to run - and the guest doing less when a mapping it asked for was refused is the ordinary
consequence rather than a second mystery.

## Why that matters more than the number

192 against 193 has been treated as ambient noise all session. It was used to dismiss a `FURTHER`
verdict, cited as the motivating example in D583, and blamed for a finding that turned out to be
a configuration artefact (D600). It is none of those things: it is one guest branch, taken or
not, with everything downstream following from it.

A drift with a mechanism can be chased. A drift called noise cannot.

## What this does not establish

**What the guest branches on.** Seven calls separate the reserve from the map, and the mapping
record indexes by call ordinal without saying what those calls were. Reading them needs an
ordered record of the *head* of the call sequence, which the trace does not keep - it keeps the
tail, for the fault.

**Nor that this is the only drift.** It accounts for the import count. The mapping sequence
diverges in more places than this one, and three runs still give one or two identical mappings
out of forty-seven, so something else varies as well.

**Nor that the four runs measured it.** They were unanimous both ways in different sessions of
this hour. Four runs is what localised the correlation; it is not a measurement of how often
each branch is taken.
