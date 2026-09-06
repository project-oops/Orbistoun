# 2026-09-02 - (/loop) The reporter could not survive an execute fault; fixed, and the ctype work turns out to be a 7x advance

Third tick on one thread, and it ends with the measurement the first two could not get.

## The crash, diagnosed (D471)

D470 established that PPSA02664's worker died before writing a trace, but not why. A probe either side
of `emit` in the vectored handler answered it in one run:

```
fault-probe: code=0xc0000005 at=0x7fff0001 rip=0x7fff0001 - before emit   <- guest jumped to a placeholder
fault-probe: code=0xc0000005 at=0x7fff0000 rip=0x7ffa932bcc54             <- the reporter, faulting
```

The reporter copies the bytes around the faulting instruction into the report (raw bytes, not a
disassembly - principle 1). Both byte windows assumed "the page holding `rip` is mapped, because the
guest was executing in it". That is true of a **data** fault and false of an **execute** one: when the
guest jumps somewhere that is not code, `rip` *is* the unmapped address. `bytes_before(0x7fff0001)`
reads backwards from `0x7fff0000` - exactly the second fault's address - and a fault inside a fault
handler ends the process instead of being reported.

**Fix:** a `readable(address, len)` using `VirtualQuery`, which describes a mapping without
dereferencing it and allocates nothing. Requires `MEM_COMMIT`, rejects `PAGE_NOACCESS`/`PAGE_GUARD`,
and requires the window to sit inside one region. Both windows ask before reading. Verified: one fault
instead of two, `emit` returns, and a trace is written.

Why it had never fired: every fault before this was a data fault. The execute fault only exists
*because* `_Getpctype` now works and the guest runs far enough to fail differently. A new kind of
fault found a hole a hundred runs of the old kind never touched - and the invariant was written down
honestly in a `SAFETY` note and was still wrong, because it was stated about "the faulting
instruction" while only ever being true of one class of fault.

## What it made visible

First honest measurement of D468, against the last recorded pre-ctype state:

| | imports | calls | fault |
|---|---|---|---|
| before ctype (01:50) | 39 | 1,544 | `image+0xb14be3`, dereferencing `_Getpctype`'s placeholder |
| after ctype | **68** | **10,905** | `0x7fff0001`, an **instruction fetch** |

**Seven times the calls, twenty-nine more distinct imports.** The ctype table was a large advance and
I spent a morning reporting it as `same - nothing moved`.

One honesty note on the verdict line: it still prints `same`, because the tool compares against the
*immediately previous* run and the first fixed run had already overwritten the baseline. The delta
above is against the recorded 01:50 state, stated explicitly rather than read off a verdict that is
comparing two post-fix runs.

## The new wall, and it is legible

`Il2CppUserAssemblies::0x6f8b9da539afc9af`, called **222 times**, `arg0` pointing at `"il2cpp_init"`
(with `"il2cpp_monitor_pulse"` following it in the same string table). It is a name resolver: it
answers our placeholder, and the guest **calls** what it was handed - hence an instruction fetch from
`0x7fff0001` rather than a read through it. Naming and implementing it is the next step; the hardware
capture's 5,400 kernel exports are the obvious naming oracle to try first.

## State

clippy `--tests` **fully clean**, including the debt flagged for two ticks: `enter` in
`orbistoun-worker/src/lib.rs` was 113/100 lines after an earlier session's spawned-thread TLS work.
Rather than flag it a third time it is now split, and the split is not lint-appeasement: the extracted
`arm_diagnostics` is the stretch whose *order* is load-bearing and whose mistakes are invisible - a
stack fill after a poke silently erases it (D229, D185) - so naming it says that it is a sequence
rather than a stretch of setup. The refusal reason is returned rather than written, keeping the halt
in `enter` beside every other way entry can be declined. Verified by running both PPSA02664 and
PPSA04263 through the guest-entry path afterwards, not by the tests alone.

fmt clean, worker/report/libc tests pass, identity scan clean, all probes removed (`report.rs` carries
only the `readable` fix), nothing committed.

One number not to over-read: the run after the refactor reported `FURTHER` on 69 distinct imports
against the previous run's 68. That is one import of run-to-run variation between two consecutive
post-fix runs, not an effect of the refactor - the advance worth reporting is the 39 -> 68 in the table
above.
