# 453. The guest was carrying the answer

**2026-09-08** - directed, continuing 452

Nothing new on the bus this pass: six requests open, two of them mine, none claimed, and no sweep
since 17:04. So all local.

## Three passes of narrowing, answered by one run

Worklogs 451 and 452 both ended at the same sentence - the payload entered with the handoff
argument calls a null pointer at `image+0x28a163` and emits nothing - and 452 gave up on it
explicitly: *"going further needs that ELF's symbol table, and nothing on this machine reads one."*

That was true and it was the wrong thing to conclude. D390 had already said the payloads *carry*
symbol tables; what nothing here did was read one, because loading needs program headers and the
section table is a link-time artefact every commercial title is stripped of.

`Container::function_symbols` now walks the section headers for `SHT_SYMTAB`, and every address
the fault path prints gains the guest's own name for it:

```text
orbistoun: the module names 1352 of its own functions, so a fault in it can say which
orbistoun: guest fault: instruction fetch from 0x0 while executing at 0x0
  from 0x40000028aaf3 (image+0x28aaf3 obs_sink_open+0x73)
  from 0x4000002878a5 (image+0x2878a5 obs_run_all+0x85)
  from 0x4000002b2d3e (image+0x2b2d3e obscene_start+0x12e)
```

`obs_sink_open` makes four `sceKernelMkdir` calls - which the trace shows completing - and then
asks for the time. That call goes to zero (D628).

## And it stopped there, deliberately

`sceKernelGettimeofday` **has a stub slot**: `ORBISTOUN_DUMP` arms it, and captures nothing. So the
guest is not reaching the stub, it is calling `0` - the relocation binding that import was never
applied, with a stub sitting there unpointed-at.

That is loader work and this project's brief confines it to user-space library and ABI-stub
implementations. Recorded exactly rather than fixed: whoever picks it up has a function, an offset,
a symbol name, and the knowledge that the stub already exists.

## What the reader refuses to do

- **An empty list is not an error.** Every commercial title has no section table; making that fail
  would push an `unwrap_or_default` into the caller, where a real parse failure would then read as
  "stripped". Verified on PPSA03416: no symbol line, and the run is otherwise identical.
- **The linked string table, not `.strtab` by name** - a section's name is itself a string-table
  lookup, so going by name means trusting the table being located.
- **A name only where the symbol says it covers the offset.** Past a recorded extent it says
  nothing rather than putting a confident wrong name on a fault.
- **No `unsafe`, every index checked**, and a test that feeds it a section table pointing past the
  end of the file and requires an empty answer rather than a panic.

## Surprises

- **The frontier is a high-water mark, not a latest reading.** Three consecutive runs of
  PPSA03416 give a stable 196 imports and 469,747 calls; the frontier records 197 and 511,383 from
  a run under a longer time limit. Not a regression and not drift - but worth knowing before
  reading a `-1` in a live run as a loss.
- **The call counts wobble by ±3 across identical runs** and the import count does not. So the
  distinct-import figure is the one a verdict can rest on, which is what it already rests on.
- **1352 functions**, in a guest this project has been running for two days by byte offset.

## Next

- The unapplied relocation behind `obs_sink_open`, when loader work is in scope.
- The seven-plus casualties of the declined syscall, waiting on `REQ-20260908T1620Z-4c1e`.
- The input group, blocked on two requests that are not mine.
- `sceAgcCreateShader`, still the wall, waiting on `REQ-20260908T1621Z-7a5d`.
