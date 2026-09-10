# D619 - The other work list

**Status:** measured
**Date:** 2026-09-08

## The same disappearance, one file along

D618 fixed the measurement table losing its evidence when the report directory is overwritten.
The export table has the same problem and was not covered by that fix, because D609 deliberately
kept it out of the measurement table: its subject is a hash and its value is where the kernel
happened to place it, so folding 2,443 of those in would make a table of checkable claims four
fifths symbol table.

Which left the 215 hashes a console exports and this project cannot name existing **only while a
batch of captures is on disk** - about an hour, as it turns out.

They are worth keeping for the reason D245 gave: a hash from an import table is one a title asked
for, and a hash from a console's own export table is one the platform *offers* whether or not
anything has ever imported it. A collision search over a corpus of guests cannot reach those by
any route. Losing the list every hour is losing the one input no amount of local work reproduces.

## A second file, not more of the first

`symbols/exported-unnamed.txt`, beside `symbols/wanted.txt` and never inside it.

`wanted.txt` means *imports orbistoun cannot name*, and every report that quotes its number means
that. A kernel export nothing has ever imported is not one, which is why D605 kept them apart in
the search and why they stay apart on disk. Two lists that count different things, with the file
name saying which.

Both go through `wanted_now`, so "still unnamed" cannot come to mean two things:

- accumulate what the file already holds,
- drop anything the emulator can now name,
- drop anything this run just named.

A run that sees no reports writes nothing and destroys nothing, which is the case that matters
most - it is what every ordinary `./bin/orbistoun names` does.

## What it is for

215 entries today. The affix rules already reached the ones they can (D606), and the rest are the
standing input for whatever comes next - a wider vocabulary, another rule, a name read out of a
module. Before this they were a number in a terminal that disappeared when the probe ran again.
