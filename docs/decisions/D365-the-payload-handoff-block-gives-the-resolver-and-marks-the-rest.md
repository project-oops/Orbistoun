# D365 - The payload handoff block gives the resolver and marks the rest

**Status:** decided
**Date:** 2026-09-26

A payload entry receives a handoff block whose field zero is the name resolver. The fields
nothing has established are filled per `ORBISTOUN_HANDOFF_FIELDS`: `strict` (unmapped markers
naming the field), `deep` (markers naming field and member offset), `members` (stubs that
report a call through a member) or `zero`. Unset, they hold mapped markers, or the measured
layout when a firmware image is present. `[entry] handoff-fields` places a literal in a named
field. Markers are decoded by the crate that makes them, with separate strides per depth.

**Why:** a marker makes the guest name the field it used in one boot, and a reporting stub
shows what it was called with. Which fill gets further depends on what an unknown field is
for, so it is a setting recorded with the run, and a sweep needs no rebuild. A guest often
truncates a marker to 32 bits, and distinct strides keep the depth recoverable from the low
half.

**Rejected:**
- Guessing field offsets: one boot per candidate and an invented layout.
- Zero everywhere by default without firmware: a pointer field faults on null and names nothing.
- A copied decoder: disagrees silently with the encoder.
