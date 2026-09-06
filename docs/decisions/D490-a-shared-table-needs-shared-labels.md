# D490 - A shared stub table needs shared labels, or the trace names the wrong function

**measured** - 2026-09-03 (four labels, three of them wrong, all corrected)

D484 put every module's imports in one stub table. The trace labels were still built from the
executable alone, and the two disagree in the worst possible way: **the label vector ends
exactly where the second module's slots begin.**

## What it looked like

The calls before the fault, as reported:

```text
just before: libc::usleep(0x30)                     -> 0x176fa836720
just before: libc::_ZdlPvSt11align_val_t(0x6000…b88) -> 0xb
just before: libc::_ZSt15get_new_handlerv(0x4800…b0) -> 0x4800…b0
just before: libc::memcpy(0x600000800b88)            -> 0x600000800b88
```

`usleep` answering a heap pointer for an argument of 48 is not a sleep. orbistoun's `usleep`
sleeps and returns zero; it cannot produce that value. **The label was wrong**, and it was
wrong in a way that reads as plausible - three of those four names are real functions the guest
could have called.

Corrected:

```text
just before: libc::_Znwm(0x30)                       -> 0x246edb418e0
just before: libc::strlen(0x600000800b88)            -> 0xb
just before: libc::strncpy(0x480001f0d2b0)           -> 0x480001f0d2b0
just before: libc::memcpy(0x600000800b88)            -> 0x600000800b88
```

`_Znwm` is `operator new(unsigned long)`: forty-eight bytes in, a heap pointer out. `strlen`
answering 11, `strncpy` answering its destination. **Every label now agrees with its own
return**, where three of four did not.

## The mechanism

`Service::labels` sizes the vector to the executable's dynamic symbol count and then *appends*
the by-name stubs. With one module that is exactly right - the by-name block sits past every
import, where it belongs.

With four modules sharing one table (D484), the executable's 585 imports are followed by
`Il2CppUserAssemblies` at 585, `PS5Util` at 1102, `libc` at 1116. The label vector's by-name
block sits at 585 too. So **every call from a title module was named after whichever by-name
stub happened to share its index** - and since the guest now spends nearly all its time inside
those modules (D489), that is most of the trace.

`import_labels_for` builds the vector over every module at its own offset, with the by-name
block after all of them.

## Why this is worse than an unlabelled trace

A missing name reads as missing. A **wrong** name reads as evidence, and it was about to be
acted on: the plan for this tick was to work out why a call returning a heap pointer was
attributed to `usleep`, and the first two hypotheses were about the *symbol database* having a
bad name - which would have been a provenance defect recorded against a name that was never
involved.

The contradiction is what saved it. A label whose return cannot match its contract is a
falsifiable claim, and this one was false. Same shape as principle 3 one level up: the report
said more than its data supported, and the check is to ask whether a value could have come from
the function named.

## What it does not explain

The wall is unchanged: `read of 0x8` at `the title's own modules+0x13dca44`, a null field read
inside the title's own il2cpp init, at a stable address. `operator new` succeeded - it answered
a real heap pointer - so the null came from somewhere else, and with the labels correct that is
now a question the trace can actually be asked.
