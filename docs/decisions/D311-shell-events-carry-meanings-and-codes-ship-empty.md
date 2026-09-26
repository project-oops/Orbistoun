# D311 - Shell events carry meanings, and codes ship empty

**Status:** decided
**Date:** 2026-08-27

`ShellEvent` is this project's own vocabulary with no codes. `Delivery`, mapping a meaning to a
code, and `Parameters` ship empty; `Settings` holds what a person chose. An event with no
measured code is withheld at `post` and counted in the run report.

**Why:** there is no lawful source for the vendor's event numbering, and an invented code is a
number the guest acts on wrongly with the failure surfacing elsewhere. Deciding at `post` keeps
the queue to deliverable events, so an unmapped event cannot block the ones behind it. A
counted withholding says the shell did not deliver, rather than appearing to work.

**Rejected:**
- Plausible or zero codes: the guest reads a meaning it was not sent.
- Queuing undeliverable events: an unmapped head blocks the queue.
