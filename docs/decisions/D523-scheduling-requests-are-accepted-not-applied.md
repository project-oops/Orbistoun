# D523 - Scheduling requests are accepted, not applied

**Status:** decided
**Date:** 2026-09-26

Thread affinity and scheduling setters answer success and do not pin or reprioritise host
threads; which host core runs a guest thread is the host scheduler's decision. A getter answers
only from a record orbistoun keeps, such as the affinity captured at thread creation, and never
invents a value.

**Why:** a setter's contract is that the request was taken, and a caller testing against zero
reads the placeholder as a refusal that stops it. Applying guest affinity to host threads would
tie guest behaviour to the host's core layout. A getter with no record behind it would be
fabricating the platform's answer.

**Rejected:**
- The stub placeholder: reads as a refusal and sends the guest down its error path.
- Pinning host threads to the requested mask: host cores are not the console's cores.
- Answering getters with plausible defaults: invented values a guest will act on.
