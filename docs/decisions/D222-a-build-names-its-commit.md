# D222 - A build names its commit

**Status:** decided
**Date:** 2026-08-24

Every binary is stamped with the commit it was built from - the workflow's head SHA in CI, `git`
otherwise, shortened only when it is plain hex, with `-dirty` for a modified tree - and shows it
in the application and in every run report. With no commit it shows its own build time, in UTC.

**Why:** a result that cannot be tied to a tree cannot be reproduced, and the short SHA is the
version number. A pull request's merge commit exists only inside the CI run. A local time shifts
and makes two stamps incomparable.

**Rejected:**
- An unset field reading "unknown": defeats its only purpose.
- The pull request merge SHA: nobody can check it out.
- A compile-time constant in one crate: older than the binary it describes.
