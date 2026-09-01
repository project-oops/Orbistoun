# 2026-09-01 (/loop) - obSCEne hardware memory data mined; map now starts at 0x10000 (D437)


The memory data D436 said was needed was already in obSCEne's hardware reports. Applied the system-wide
parts: direct map default is now ReservedLow with the measured 0x10000 floor (was Whole/512MiB arbitrary)
- retires the D083/D218 never-swept assumption; PPSA02664 FURTHER (+1). Reverted a flexible-budget change
(0x1b400000) after it took PPSA02664 backwards: that is the *probe's* per-process budget, not a game's, so
it does not transfer (the "measurement stays with what measured it" rule). Recorded as a reference const.
Still to apply (system-wide, pending test): the direct map virtual base (hardware 0x2_0000_0000 vs
orbistoun 0x7200). kernel tests green (74+33+33); build clean. SURPRISE: not all obSCEne measurements
transfer to orbistoun - system-wide yes, per-process no.

