# 442. A different program

**2026-09-08** - directed, continuing 441

## What was done

Set out to force `0x8a6c003d` into the guest, as worklog 441 planned. Checked the baseline first,
and the plan dissolved.

**The bytes that value came from were not the baseline's.** They were read out of a `turn`
transcript, and a turn runs the guest under sweeps - those runs faulted at `image+0xf56e09`, not
at the wall. D598 is corrected in place: the mistake is left visible with the reason, because
writing *an intervention that moves a wall is not a diagnosis* into a decision entry is the
version of that error that does the most damage.

**Then the reason the transcript disagreed turned out to be worse than a sweep.**

```text
./bin/orbistoun run PPSA02664       fault image+0x39f7c
orbistoun-cli run <same file>       fault image+0xf56e09
```

The driver passes `--symbols-db`; the bare command does not, and **`turn` never did** (D599). The
database decides which imports are named, a named import is one an implementation can be found
for, and an unnamed one gets a stub - so the dispatcher has been sweeping a program in which a
different set of functions is unimplemented, and comparing the results against the loop.

Fixed: `GuestTrial` carries the database, `cmd_turn` passes it, and the turn now reports
`two runs agree: 198 imports, fault 0xffffffffffffffff` - the loop's wall.

## Surprises

- **The guard that hid it was a real guard.** `spawn` documents that it runs *the same binary* so
  the runner cannot be stale, and clears every diagnostic variable so a baseline is clean. Both
  true; neither asked whether the *configuration* matched. A symbol database is configuration
  that changes which code runs.
- **It cost a decision entry.** D598's central claim came from a run that was under a sweep *and*
  under the wrong symbol set.
- **PPSA02664 calls `libSceAmpr` directly** - the real API, which orbistoun stubs. The clean title
  needs the platform implemented; the modified one needs somebody else's shim served.

## Next

- Re-run the turn conclusions from this session that mattered. Each needs re-deriving rather than
  re-reading; a conclusion about a call named in both configurations is unaffected, and which
  those are has not been worked out.
- Whether the driver sets anything else the bare command does not. `--symbols-db` is the one whose
  absence was measured.
- The clean title's wall, now that it can be measured through either path.
