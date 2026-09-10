# D585 - A fresh thread attribute set is not entirely zero, and the console said so

**Status:** measured
**Date:** 2026-09-08

## The first answer that came back from the hardware probe

`031-stackattr` was written into obSCEne on 2026-09-07 to settle three questions orbistoun had
been answering from assumption. It ran on the console, and the report is in
`obscene/reports/hardware/fullguard-klog.txt`:

```text
OBS|measure|031-stackattr/self-describes|scePthreadAttrGetstackaddr|stack-address|0x7eedfc000
OBS|measure|031-stackattr/self-describes|scePthreadAttrGetstacksize|stack-size|0x200000
OBS|measure|031-stackattr/address-is-the-base|scePthreadAttrGetstackaddr|placement|0x1
OBS|measure|031-stackattr/address-is-the-base|sceKernelIsStack|low|0x7eedfc000
OBS|measure|031-stackattr/address-is-the-base|sceKernelIsStack|high|0x7eeffc000
OBS|measure|031-stackattr/fresh-attr-names-no-stack|scePthreadAttrGetstackaddr|stack-address|0x0
OBS|measure|031-stackattr/fresh-attr-names-no-stack|scePthreadAttrGetstacksize|stack-size|0x10000
```

**Two of orbistoun's assumptions are now measurements.** The address a thread attribute set
reports is the **lowest** byte of the stack, not its top - `0x7eedfc000 + 0x200000 = 0x7eeffc000`,
which is exactly what `sceKernelIsStack` reports as the span. And a fresh set answers a stack
address of zero and success, as `pthread_attr_getstackaddr(3)` does. Both were already what this
project did, on FreeBSD's authority; they are no longer assumed.

## The third answer was one orbistoun had wrong

**A fresh set reports a stack size of `0x10000`.** `pthread_attr_init` zeroed the whole block, so
orbistoun answered zero - and the two fields have *different* defaults, which is precisely the
kind of thing that cannot be reasoned out. A guest sizing an allocation from the default got
nothing; a guest checking the size before creating a thread took a failure path the console never
gives it.

Sixty-four kibibytes is also **not FreeBSD's default**, which is far larger. Nothing but the
target could have supplied this number, which is what the question loop exists for
(`docs/THE_LOOP.md`, the questions sub-loop): an assumption written down can be counted, ranked,
probed and retired, and this one has been.

## It costs an import, and that was noise

The first run after the change reported 192 distinct imports against 193. Four runs give
192, 192, 193, 192 - **the drift `Step::CheckRepeats` measures** (D583), not a cost of the
default. Checked because a hardware-measured value making a guest do less is exactly the result
worth being suspicious of, and the check is one command now.

## What this does not establish

**What the size means.** It is what a set reports before anything sets one. Whether the platform
*uses* it as the stack size for a thread created from an untouched set is a different question,
and one this measurement does not reach - a run would have to create such a thread and ask.

**Nor anything about the other fields.** The probe read two. Detach state, scheduling policy,
priority and guard size are all still zero here because nothing has measured them, and each is a
separate question with its own answer.
