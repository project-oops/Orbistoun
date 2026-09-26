# D162 - No control without something behind it

**Status:** decided
**Date:** 2026-09-26

A settings control exists only when something honours it. A pane for a subsystem with nothing
behind it says what is missing; a control that cannot work yet is disabled with its reason on
hover, never hidden and never inert.

**Why:** a dropdown over an unimplemented subsystem changes nothing, and nobody can tell whether
the setting, the emulator or the title is at fault. A control that vanishes reads as a bug; a
greyed one with a reason reads as a state.

**Rejected:**
- Controls built ahead of their subsystems: a lie with a widget.
- Hiding unavailable controls: users search for them and assume a defect.
