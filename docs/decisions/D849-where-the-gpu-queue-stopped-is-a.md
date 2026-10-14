# D849 - Where the GPU queue stopped is a position progress compares

**Status:** decided
**Date:** 2026-10-14

A run's trace records where its GPU queue stopped, when it did: the submission whose
command-processor work stopped, counted from the first carried out, and the byte of its command
buffer it stopped at. Nothing after that point retires (D710), so it is how far the title's GPU
work got. When neither run faulted, progress compares the two runs' stops as it compares fault
offsets within a region (D129). Later is FURTHER, earlier is BACK, the same is the same. The
progress block prints both stops.

**Why:** Grand Theft Auto V runs to the time limit every time, so its runs have no fault
position, and a draw it could not run before only shows in the GPU log. Its fourth and fifth
draws ran one after another, moving the stop from byte `0xbb0` to `0x1600` of its first
submission, and the verdict read `same`. The stop had been read from the log by hand each run.

**Rejected:**
- Counting draws carried out instead: a submission's draws are carried out a segment at a time,
  and the stop says where the next refusal is, which the count does not.
- Comparing stops when one run faulted: a fault position and a GPU stop do not order against
  each other.
