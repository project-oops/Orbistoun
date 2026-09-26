# D621 - Per-thread call attribution

**Status:** decided
**Date:** 2026-09-26

Every recorded call carries the host thread that made it, and "what am I inside" is answered by a
per-thread current-call slot rather than by the newest call in the shared ring. A fault lists its
own thread's recent calls first and then a few from other threads, each labelled as another
thread's.

**Why:** the shared ring answers "what was this process doing", which is the wrong question for a
fault or a mapping record once a guest runs more than one thread. A process-wide tail pairs a
fault with a wait on a different thread and reads as a cause. Other threads stay visible because a
fault while another thread holds something is a real situation.

**Rejected:**
- The newest call in the shared ring for every question: attributes one thread's work to another.
- Showing only the faulting thread's calls: hides cross-thread interactions a reader needs.
