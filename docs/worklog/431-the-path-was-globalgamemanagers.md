# 431. The path was globalgamemanagers, and yesterday's finding was wrong

**2026-09-08** - directed, continuing 430

## What was done

**The asynchronous file path is declared and reports itself** (D587). Three functions the wall
sits behind - resolve, submit, wait - had no declaration at all, so every call landed on the
generic stub and the run could say only that something unimplemented had been called. They report
and refuse now; declaring is not implementing.

`sceKernelAprWaitCommandBuffer` was a bare hash on `symbols/wanted.txt`. Named from the guest's
own printed wrapper name plus its sibling's naming pattern, confirmed by the hash agreeing.

**Write implies read, and the dump had run out of room** (D588). Two separate causes of one
symptom, both found by chasing why a buffer would not read back.

## The first thing the telemetry said

```text
orbistoun: the guest asked the asynchronous file path to resolve 1 path(s):
           /app0/Media/globalgamemanagers
```

The loose-layout entry point - the file D578 established this title opens and reads nothing from.
The shape is settled: the guest opens by name, hands the *path* to the asynchronous path for an
identifier, and reads through that. Five opens, one read of zero bytes, explained.

## Yesterday's finding is withdrawn

D580 recorded that the guest holds a pointer to a buffer orbistoun never mapped. **It maps it** -
`0x740009200000..0x740009300000`, one mebibyte, exactly the length the header states. Asked from
inside the call that receives the pointer, where the address and the answer cannot be a run apart.

Two mistakes, both ones this project already names. The address was typed in from a *previous*
run while the arena moves between runs. And "no published span" was read as "not mapped" - the
distinction that same entry had renamed a message to make. **Renaming a message is not the same
as believing it.**

## Why it would not read, which was worth the chase

- **`protection_from_guest` recorded a write-only mapping as unreadable.** An x86-64 page table
  entry has no read bit; a writable page is readable and there is no encoding that is not. This
  title maps its destination buffer with write alone.
- **The readable-range table held sixty-four and silently dropped the rest.** Sized when only
  thread stacks published there; D579 then had every guest mapping publish too, and this title
  reaches about a hundred and twenty. Raised to five hundred and twelve, and an overflow now says
  so - a capacity limit and a wrong pointer printed identically.

## Surprises

- **The buffer is zero at submission**, while its header reports one command of twenty bytes -
  and `sceAmprAprCommandBufferReadFile` is imported by this title and never called. Recorded as
  an open contradiction rather than resolved by picking the convenient half.
- **Resolve is not the gate.** Markers planted in all three out-parameters *and* the return
  forced to success changed nothing visible: the guest submits either way.
- **Three imports moved from stubbed to answered** - 181 to 184 - purely by being declared.
- **A knowledge gate caught the skeletons immediately**: implementing something without recording
  what is known about it fails `every_implemented_function_is_written_down`, which is principle
  1's accounting working exactly as designed.

## Next

- Where the twenty bytes of command actually are, given the buffer the header points at is empty.
- Why `sceAmprAprCommandBufferReadFile` is never called when the title imports it.
- The mapping count doubling when handles clustered, still undiagnosed from worklog 430.
