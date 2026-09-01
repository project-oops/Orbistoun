# D022 - The trace sink is wired at phase 4, not earlier

**decided** · 2026-08-19

It records guest calls, and no guest call exists before phase 4. Wiring it sooner
would be exactly the unexercisable code D004 exists to prevent. (An earlier note
called the unwired sink "a real gap" - that was wrong about sequencing.)

