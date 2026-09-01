# D341 - Cross-port key conflicts were invisible, and a keyboard could not move a stick


**decided** · 2026-08-27 · found by review against a parallel implementation

Prosperous grew a controller model of its own, for sending input to real hardware, and
reading the two side by side found three things here.

**A keyboard could not move a stick at all.** `Port` mapped `Button` only, and the reader
set buttons and triggers and returned. Since the shipped configuration is one keyboard port,
the out-of-the-box setup could press seventeen buttons and drive nothing analogue - which is
most 3D titles, and nothing said so.

Sticks are not buttons: a button is a bit and an axis is a number, so they get their own
table keyed by [`Push`] - one of eight named directions, because a key is on or off and one
key per axis could only move it one way. **Opposite pushes sum and clamp**, so left and
right held together mean centre. Letting the first win would make the pair mean something no
pad can express.

**A key bound on two ports was never reported**, because `conflicts()` built its seen-map
inside the per-port loop. That left two ways to add a second keyboard player: copy the first
port's layout, where all seventeen keys silently drive both pads and one person moves two
characters, or bind every one by hand. The docstring already made the argument against the
first - *"a binding that half works with nothing saying so"* - and the cross-port case is the
same failure with a wider blast radius.

**Worth recording as a deliberate disagreement**, so neither project gets "fixed" to match
the other: this `PadState` is host-shaped floats and does not guess the guest layout, while
prosperous's is unsigned bytes centred on 128 because that layout has since been measured on
hardware. Same question, different evidence, both right for their own project.

