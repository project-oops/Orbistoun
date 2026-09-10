# 480. FURTHER

**2026-09-09** - directed, continuing 479

obSCEne answered `REQ-20260909T1720Z-4e17` with the exception context: `rsi` and `rdx` identical,
0x180 bytes on the interrupted thread's own 2 MiB stack, signal number at `+0x48`, and `+0xf8`
holding a pointer to another mapped address on that same stack.

```text
before:  141 imports   310,987 calls   ran to the time limit, silent for 19.7s of 20
after:   151 imports   321,973 calls   image+0x17554a3
verdict  FURTHER  executed code it could not reach before
```

The fault has left the title's own modules entirely. Nothing else in the corpus moved.

## Three attempts

**A reserved page of orbistoun's own**, at `0x5E2E` per the address map's rule for invented
regions. The handler ran past `mov rax, [rsi+0xf8]`, made ten more calls, reached two more
imports - and faulted `+0x13c8`, past the end of the page.

**The same region at 2 MiB**, the measured extent of the console's own mapping. Faulted `0x368`
past the end of *that*.

**That was the finding.** The guest is *scanning* - eight bytes at a time, until it leaves the
allocation. A collector walking a suspended thread's stack for roots. A scan is bounded by the
allocation it is in, so the size of an invented region never mattered; being the right allocation
did. The context is now built into the low end of the stack `call_guest` already reserves for the
handler, via a new `call_guest_placing` that hands the caller that span and takes the arguments
back - so only the function owning the reservation ever knows the address (D656).

## Surprises

- **Two wrong sizes were more informative than either would have been alone.** One overrun could
  have been a too-small buffer. Two, at 4 KiB and 2 MiB, said the number was never the point.
- **An `Option` fallback hid a whole attempt.** Using the *target* thread's recorded stack looked
  right and returned `None` - `main` is adopted, not spawned, and has none - so the handler got a
  null and the run went straight back to where it started, with nothing saying why.
- **The call count is no longer deterministic for this title.** A collector is genuinely running
  across threads now, so calls-per-second varies; three runs gave 321,973 / 321,970 / 321,961. The
  fault site, which is what progress is read from, is identical. The frontier records one count
  and may need this title on a call budget if the jitter grows.
- **The frontier gate did exactly its job**: refused the run, printed both rows, and asked to be
  regenerated and read. The diff is the entry above.

## Next

- `image+0x17554a3` - a new wall, in the executable rather than a shipped module, and nothing is
  known about it yet.
- The inbox request about `NidHasher::hash`'s byte-order wording.
