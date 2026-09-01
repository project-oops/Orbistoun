# 2026-08-30 - Ask the guest, one field at a time


```text
  field  0  USED         instruction fetch from the field's own value
  field  1  USED         read of the field's own value, at image+0x4519
  field  2  USED         read of the field's own value, at image+0x44ef
  field  5  USED         write to the field's own value, at image+0x24a2
fields used: 0, 1, 2, 5
```

`orbistoun-cli handoff` poisons one handoff field per run with an address nothing maps and
reads back whether the guest faulted on it. Three payloads, built separately, agree exactly -
so that is the runtime's structure, not one binary's quirk (D390).

Field five is the new one and it is the interesting kind: the runtime **writes** through it.
The loader hands the payload somewhere to put something.

**The tool matters more than the answer.** This session leaned hard on the payloads being open
and carrying symbol tables - names in fault reports, a `main` to enter at, READMEs describing
an interface. None of that will be true of a commercial title, and a method that needs symbols
stops working the moment it is pointed at the thing this project exists for. The field walk
needs neither: a stripped binary, an undocumented structure, and one run per question.

Along the way, two corrections to yesterday's reasoning. I concluded `0x2001` was not from the
handoff because it was identical under two marker modes - which does not follow, since those
modes only differ for the first sixteen fields. Retested against a third fill and the default
mode, and against field two set explicitly: it holds, now for a reason that is actually
evidence. And I had never tested the *default* handoff mode at all, which turned out to be the
one I was proposing to build.

`/dev/klog` also exists now, fed from what orbistoun already writes about a guest - with the
caveat recorded rather than left to be found: the device is faithful, the content is this
emulator's log and not a console's (D389).

