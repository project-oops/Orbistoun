# orbistoun-input

Controller input - the guest's pad library, and the host-side model of a controller.

**Models:** opening and closing a pad, handle validity, vibration and the light bar. Plus,
for the host side, what a controller *is*: buttons by position, sticks, triggers, how many
ports there are, which key maps to which button, and the difference between a tap and a hold
on the shell button.

**Deliberately fakes:** vibration and the light bar, which are accepted and discarded.

**Deliberately refuses:** everything that reads pad state into guest memory -
`scePadReadState`, `scePadReadStateExt`, `scePadRead`, `scePadReadExt`. They are declared so
a trace can name them and left unimplemented, because the structure they write has a size and
a layout nobody here has measured, and inventing bytes a title will read is principle 3's
forbidden case with a consumer attached.

## Where the names come from

A binary in the library imports **ninety-seven** functions from this library, and every name
declared here is one of them - read from that module's own import table. That is stronger
than a derived name: not something this project generated and hoped matched, but something a
real module demonstrably asks for.

The library exports **both** an older and a newer spelling of several calls - `scePadOpen`
beside `scePadOpenExt`, `scePadReadState` beside `scePadReadStateExt` - and that module
imports both, so both are declared and one implementation serves each pair (D340).

The names are confirmed; **the arities are not**. That asymmetry is deliberate: a wrong arity
degrades a call trace and does not break the call, while a wrong name is a NID that matches
no import and a shim that can never be reached.

## Buttons are named by where they sit

`South`, `East`, `L1`, `R2` - never by glyph. Principle 2 keeps vendor marks out, and there
is a practical reason too: a keyboard has no glyphs on it, so a key-to-button mapping has to
target a position anyway. Naming by symbol would mean translating twice.

`Button::Shell` is the one with different rules. Every other button belongs to the title when
the title has focus; that one is always the shell's, because it is how somebody reaches the
shell *from* a title - so `as_title_sees_it` removes it and nothing else.

## What is missing, plainly

**Real gamepads.** `Source::Gamepad` is in the configuration and nothing reads one, so a port
set to it reports a pad nobody is holding. The settings pane says so beside the control
rather than leaving it to be discovered.

**A second keyboard player** has no shipped layout. Cross-port key collisions are now
reported rather than silent (D341), so binding one by hand is safe - but it is still by
hand, and one keyboard is a cramped place for two people.

Vendor-specific haptics and adaptive triggers have no general PC analogue and are out of
scope until something asks for them (`docs/SCOPE.md`).
