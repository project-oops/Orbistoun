# D311 - The shell holds meanings and refuses to hold the vendor's numbers


**decided** · 2026-08-27 · a provenance constraint that turned out to be the design

A title learns it was interrupted by draining an event queue, and this repository has no
lawful source for what those events are *numbered*. Inventing a plausible code is principle
3's forbidden case exactly: the guest reads a number that means something specific to it,
acts on it, and the failure surfaces somewhere else. Choosing zero is the same act in humbler
clothing.

So meaning and number are separated. `SystemEvent` is our own vocabulary and carries no
codes; `Delivery` maps a meaning onto a code and **ships empty**. `Settings` holds what a
person chose; `Parameters` holds numbers and ships empty too. An event with no measured code
is not delivered - and is *counted*, so a run says *"4 withheld for want of a measured code
(backgrounded x2, focus-lost x2)"* rather than the shell quietly appearing to work.

The undeliverable case is decided at `post` rather than at the far end, so the queue only
ever holds things the guest can actually be given. An unmapped event parked at the head would
otherwise deny the guest every deliverable event behind it.

`sceSystemServiceReceiveEvent` is therefore **still not declared as an import**, though its
name is hash-confirmed in this repository's own database. Implementing it needs two things
nobody has measured: the value meaning *no event is pending*, and the layout of the structure
an event is written into. Both are jobs for a probe on real hardware, not for reasoning about
what the numbers probably are.

**The reframe worth keeping.** This looked like a limitation and is closer to the opposite:
`Settings` is the first thing in the tree with standing to answer the questions
`orbistoun-systemservice` has always answered with a placeholder, because a console setting
is a fact about what the owner wants and the owner is right there. What was missing was never
knowledge - it was somebody entitled to decide.

