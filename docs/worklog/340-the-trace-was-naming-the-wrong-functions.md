# 2026-09-03 - (/loop) The trace was naming the wrong functions

```
tests   1991  ->  1991   (a sizing bug; the fix is in code the suite already covers)
```

Went looking for why the guest reads a null field inside its own il2cpp init. Found instead
that the report had been lying about which functions it called, ever since the stub table was
shared four worklogs ago.

## The contradiction that gave it away

The report's own "what to do about it" section named the calls just before the fault:

```text
just before: libc::usleep(0x30) -> 0x176fa836720
```

**`usleep` cannot return a heap pointer.** orbistoun's implementation sleeps and answers zero -
that is four lines of code with no other path. A label whose return cannot match its contract
is a falsifiable claim, and this one was false.

Corrected, it is `libc::_Znwm(0x30) -> 0x246edb418e0` - `operator new(48)`, forty-eight bytes in
and a heap pointer out. The other three in the window were wrong too, and all four now agree
with their own returns:

| as reported | actually |
|---|---|
| `usleep(0x30) -> 0x176fa836720` | `_Znwm(0x30) -> 0x246edb418e0` |
| `_ZdlPvSt11align_val_t(p) -> 0xb` | `strlen(p) -> 0xb` |
| `_ZSt15get_new_handlerv(p) -> p` | `strncpy(p) -> p` |
| `memcpy(p) -> p` | `memcpy(p) -> p` |

## The mechanism, which is mine (D490)

`Service::labels` sizes its vector to the executable's symbol count and appends the by-name
stubs after it. Correct for one module - the by-name block sits past every import.

D484 then put four modules in one table: the executable's 585 imports, then
`Il2CppUserAssemblies` at 585, `PS5Util` at 1102, `libc` at 1116. **The by-name block sits at
585 as well.** So every call from a title module was named after whichever by-name stub shared
its index, and since D489 the guest spends nearly all its time in those modules.

`import_labels_for` now builds the vector across every module at its own offset. The labels are
computed inside `link_title_modules`, because that is where the module bytes are - so the
symbol database is passed in rather than the 43 MB of module bytes being carried out.

## Nearly acted on it as a provenance defect

The plan for this tick was to find which call answered the null. The first two hypotheses were
that the *symbol database* had a bad name for a NID - which would have been recorded as a
provenance defect against a name that was never involved.

What stopped it was asking whether the value could have come from the function named. It could
not. That check is cheap and it is the same shape as principle 3 one level up: **a label is a
claim, and a claim that contradicts its own data is the one to pull.**

## What it does not explain

The wall is where it was: `read of 0x8` at `the title's own modules+0x13dca44`, stable across
runs, inside the title's own il2cpp init. `operator new` **succeeded** - it answered a real
heap pointer - so the null came from somewhere else. With the labels correct that is now a
question the trace can be asked; it could not be before.

The ±3 oscillation survives (44/46 distinct, 2077/2080 calls across three runs).

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Removing the now-unused `bytes` parameter from `prepare_diagnostics` collapsed its call site and
brought `place_and_relocate` back under the line limit, which the label change had pushed over.

Nothing committed. The day holds worklogs 292-340 and D466-D490.

**Next**: the null read, now that the trace can be read. `operator new` answered; something
between it and the field access at `+8` did not.
