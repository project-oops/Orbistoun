# D341 - Keyboard sticks are eight named pushes that sum

**Status:** decided
**Date:** 2026-08-27

A port maps keys to buttons and, in a separate table, to one of eight named stick pushes;
opposite pushes held together sum and clamp to centre. Conflict checking spans all ports.
`PadState` is host-shaped floats and makes no claim about the guest's layout.

**Why:** a button is a bit and an axis is a number, and a key is on or off, so one key per axis
could move a stick only one way. Letting the first of two opposite keys win means something no
pad can express. A key bound on two ports drives two pads for one person. Orbistoun has not
measured the guest's pad layout, so its host state stays floats; Prosperous uses the measured
byte layout, and neither is changed to match the other.

**Rejected:**
- Buttons only for the keyboard: nothing analogue can be driven.
- First key wins: an impossible pad state.
- Per-port conflict checks: cross-port duplicates pass silently.
