# D241 - One lock per module for one process-wide table, and the run that measured nothing


**decided** · 2026-08-25 · a flaky test, and the general form of four failures

### The flake

`orbistoun-fs` had **two** `exclusively()` helpers - one in `descriptor`, one in `open` -
each over its own private `Mutex`. The state they guard is neither module's: the mount table
and the descriptor table are process-wide, and both modules' tests call `mount::clear()`
before installing their own title.

So a descriptor test holding its lock and an open test holding *its* lock ran at the same
time and unmounted each other, and `open("/app0/game.bin")` returned `None` in a test that
had just written the file. Two locks, one piece of shared state - the same shape as the two
counters in D239.

It failed about **twice in five runs**, which is the worst frequency a test can have: too
rare to be believed and too common to ignore, so the reflex is to re-run rather than to
look, and an intermittent red becomes a thing people scroll past. That is where a real
failure goes to hide.

One lock now, in the crate root, named for what it protects rather than for whichever module
needed it first. Twelve consecutive runs, no failures.

### The general form

Four failures in one day shared a structure, and it is worth stating as a rule rather than
four entries:

> **A mechanism whose "did nothing" output is identical to its "changed nothing" output will
> eventually be read as the second when it was the first.**

- A plant that reached no writable target reported an unchanged run (D229).
- A forced return that could not key an unnamed function reported an unchanged run (D230).
- Two counters computing one quantity printed different numbers, neither saying which
  definition it used (D239).
- Documentation asserting its numbers were tool-generated while they drifted (D240).

None was a bug in the emulator. All four were the tooling reporting success it had not
established, which is principle 3 pointed at the instruments.

### What that changes

Counting was not enough. `(0 planted, 1 refused)` was already printed, in the conditions
line, and was read straight past - because it sits above the verdict and looks like
bookkeeping.

`Conditions::did_nothing` records any diagnostic that was asked for and applied **zero**
times, and the report prints it **at the verdict**, where the conclusion is drawn:

```
verdict  same     nothing moved
         !! ORBISTOUN_WRITE planted nothing - this run measured nothing it was asked to measure
```

"Nothing moved" and "measured nothing" together cannot be misread as an elimination. Proven
both ways before being trusted: a write aimed at a non-pointer argument raises it, and a
write that lands does not.

The rule to apply to the next diagnostic: **it is not finished when it works. It is finished
when a run in which it did nothing says so where the answer is read.**

