# orbistoun-input

Controller input: the guest's pad and mouse libraries, and the host-side model of a
controller.

On the guest side it opens and closes pads, validates handles, answers pad reads, and accepts
and discards vibration and light-bar calls. On the host side it models what a controller is:
buttons by position, sticks, triggers, ports, the key-to-button mapping (`mapping`), and the
difference between a tap and a hold on the shell button. [orbistoun-gui](../orbistoun-gui/)
feeds it host input; [orbistoun-proto](../orbistoun-proto/) carries its types to the worker.

## Pad reads

`scePadReadState` and its batched twins write the full extent a title reads, every call. The
bytes are transcribed from a hardware measurement (`pad::AT_REST`) rather than built from a
guessed structure, and a shim writing only the fields it thought it understood would leave
the rest of the guest's buffer holding stale bytes.

## Names and arities

Every declared name is one a module in the title library imports, read from that module's
own import table: not a name this project derived and hoped matched, but one a real module
asks for. The library exports both an older and a newer spelling of several calls
(`scePadOpen` beside `scePadOpenExt`, `scePadReadState` beside `scePadReadStateExt`), so both
are declared and one implementation serves each pair.

The names are confirmed; the arities are provisional. A wrong arity degrades a call trace and
does not break the call, while a wrong name is a NID that matches no import.

## Buttons by position

Buttons are named by where they sit - `South`, `East`, `L1`, `R2` - never by glyph. Vendor
marks stay out (CLAUDE.md, *Naming*), and a keyboard has no glyphs, so a key-to-button mapping
targets a position anyway.

`Button::Shell` is the exception to title ownership. Every other button belongs to the title
when it has focus; the shell button always belongs to the shell, because it is how a person
reaches the shell from a title. `as_title_sees_it` removes it and nothing else.

Cross-port key collisions are reported by `conflicts()`, never silently resolved.

Vendor-specific haptics and adaptive triggers have no general host analogue and are out of
scope ([docs/SCOPE.md](../../docs/SCOPE.md)).
