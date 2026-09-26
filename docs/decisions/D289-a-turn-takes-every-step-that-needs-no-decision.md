# D289 - A turn takes every step that needs no decision

**Status:** decided
**Date:** 2026-08-26

`turn::turn` runs a plan and returns what each step produced; an out-parameter finding is
followed through by reserving a region, planting its base and checking whether the guest goes
further. A step it cannot take says why: a person's step is a refusal with a reason, and a
naming step is automatic but not runnable here.

**Why:** a follow-up fully determined by the previous answer has no decision in it, so the
loop takes it instead of printing it. `Trial` carries both `run` and `spawn`, so the
dispatcher is tested against an in-memory guest. Collapsing the two kinds of "cannot" would
report the naming loop as a policy refusal.

**Rejected:**
- A planner that only prints steps: nothing takes them.
- A runner bound to `GuestTrial`: testable only by booting a commercial title.
