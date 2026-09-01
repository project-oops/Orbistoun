# D326 - A controller subsystem that stops at the guest, and says where


**decided** · 2026-08-27 · built after checking what could honestly be built

Asked for a controller system - configurable count, real pads, keyboard mapping - and for
those inputs to reach the emulator so the shell could be tested by pressing its own button.
Checking first split the work in two, and the split is the decision.

**The host side is entirely buildable and is built.** Buttons named by position rather than
by glyph, because a keyboard has no glyphs and a host pad reports positions anyway - naming
by symbol would mean translating twice, and it keeps vendor marks out of the tree
(principle 2). Ports are configurable to four, each driven by nothing, the keyboard, or a
gamepad; key names are text so the mapping does not carry a window toolkit into the input
contract (principle 12).

**The guest side stops at a structure nobody has measured.** `scePadReadState` writes pad
state into guest memory and *nothing in this repository knows that layout* - the same wall as
`sceSystemServiceReceiveEvent` (D311). So host input is read, the shell takes its button, and
what is left is **not carried across the process boundary at all**: there is nothing on the
far side that could consume it, and building the transport first is speculation by principle
12's own test. `orbistoun-input` remains declarations only, as its own header already said.

**The shell button is the reason the window owns input.** The system's own button has to be
seen by something that is not the title, and a worker reading a pad directly could never hold
one button back. That is not a workaround for the process split - it is the argument for
reading input on this side of it.

Tap and hold are separated in a pure type taking elapsed milliseconds, so every edge is an
assertion: a hold fires *while still held* because that is the only feedback before letting
go, it fires once rather than every frame, and a release after one is not also a tap. Each
of those is a menu bug that would otherwise be found by holding a button until something
looked wrong.

**Verified by pressing it.** Holding for 1.1s opened the power menu with "quit the title"
correctly absent, because nothing was running.

