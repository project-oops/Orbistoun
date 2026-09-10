# D628 - The guest names its own functions

**Status:** measured
**Date:** 2026-09-08

## An answer that was in the file all along

Three passes in a row ended at the same sentence: the conformance payload, entered with the
handoff argument, calls a null pointer at `image+0x28a163` and emits nothing. Two hypotheses were
tested and killed. The third could not be tested at all, because `image+0x28a163` names a byte and
nothing here could turn it into a place.

The answer was sitting in the guest. D390 said so in passing - *"the payloads happen to be open and
to carry symbol tables"* - and nothing read one, because loading a module needs program headers and
the section table is a link-time artefact every commercial title is stripped of.

## A section-header symbol reader, and what it must not do

`Container::function_symbols` walks the section headers for `SHT_SYMTAB`, follows its `sh_link` to
the string table, and returns every `STT_FUNC` with a non-zero address.

Three things it refuses:

- **The linked string table, not `.strtab` by name.** A section's name is itself a string-table
  lookup, so trusting the name would mean trusting the table being located.
- **An empty list is not an error.** Every commercial title has no section table; making that a
  failure would push a `unwrap_or_default` into the caller, where a real parse failure would then
  read as "stripped".
- **No `unsafe`, and every index checked.** This parser takes hostile bytes like every other one
  here (principle 4), so a table past the end of the file yields nothing rather than a panic - and
  there is a test that makes it do exactly that.

## Named where the address is rendered, not where the fault is handled

The hook is in `Line::address`, so **every** address the fault path prints gains the name, not just
the faulting one - the call chain above it is where the answer actually was. The table is sorted
once before entry and looked up by binary search returning a borrowed `&str`, so the handler still
allocates nothing (principle 9).

**Only where the symbol says it covers the offset.** Where the producer recorded an extent, an
offset past the end of the nearest preceding function is in a gap or in something unnamed, and
putting a confident wrong name on a fault is worse than putting none. Where the extent is zero the
nearest preceding name is offered with the distance beside it - the same hint-not-fact bargain
`own_code_site` makes one region over (D380).

## What it said on the first run

```text
orbistoun: the module names 1352 of its own functions, so a fault in it can say which
orbistoun: guest fault: instruction fetch from 0x0 while executing at 0x0
  from 0x40000028aaf3 (image+0x28aaf3 obs_sink_open+0x73)
  from 0x4000002878a5 (image+0x2878a5 obs_run_all+0x85)
  from 0x4000002b2d3e (image+0x2b2d3e obscene_start+0x12e)
```

Three passes of narrowing, answered by one run. `obs_sink_open` does four `sceKernelMkdir` calls -
which the trace shows completing - and then asks for the time, and that call goes to zero.

## And what it stopped at, deliberately

`sceKernelGettimeofday` **has** a stub slot here: `ORBISTOUN_DUMP` arms it, and captures nothing,
so the guest is not reaching it. The guest is calling `0`, which means the relocation binding that
import was never applied - a stub exists and nothing points at it.

That is loader work, and this project's brief confines it to user-space library and ABI-stub
implementations. Recorded exactly, not fixed: the next person to look has a function name, an
offset, a symbol, and the knowledge that the stub is already there.
