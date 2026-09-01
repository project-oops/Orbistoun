# D331 - The diagnostic that said something was the one the summary dropped


**decided** · 2026-08-27 · found by reading the loop's own output on a real title

A turn against PPSA02664 reported:

```
asked 6 other diagnostics; 5 changed nothing
```

Six asked, five silent, and **the sixth never named**. The whole reason to run a diagnostic is
the answer that is not `Nothing`; the summary counted the negative space and discarded the
signal.

`Taken::Probed` now names every axis whose answer was not `Nothing`, and says what kind of
answer it was - which matters more here than it looks, because the kinds mean opposite things.
The one this was hiding:

```
Fill { region: Bss, byte: 165 }: broke it earlier: 0xffffffffffffffff,
  reaching 8 against 25 - says nothing about the original wall
```

**A fault at a new address reads as a lead.** That one is not: poisoning the BSS broke the
guest before it reached what was being asked about, so the new address says nothing about the
old one. `Change` already held the distinction - `MovedTo` against `BrokeEarlier`, split for
exactly this reason (D129) - and the summary threw both away together.

So the fix is not only "report more". Reporting the address without the kind would have been
worse than silence: it would have handed somebody a wrong lead with a real-looking number on
it, which is the failure this log names most often.

`NotApplied` is spelled out too - *"applied zero times - this measured nothing it was asked to
measure"* - because a diagnostic that never reached the thing under test and one that reached
it and found nothing produce identical output, and two recorded eliminations turned out to be
the first kind (D229, D230).

**Found by running the tool and reading what it said**, on the first turn taken after the
loop was wired end to end. The machinery was correct; its account of itself was not.


