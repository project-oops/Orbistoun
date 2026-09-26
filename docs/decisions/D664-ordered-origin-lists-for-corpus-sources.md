# D664 - Ordered origin lists for corpus sources

**Status:** decided
**Date:** 2026-09-10

A corpus source lists its origins as plain strings tried in order, first answer wins, each
classified as a URL (it contains `://`) or a path. The attempt records every origin that failed
with its reason, and all failing is an error naming each attempt.

**Why:** a sibling checkout is the fast path for whoever has one and absent for everyone else, and
a release is always there but slower; one origin per source is wrong for one of them. Keeping the
failures shows when the fast path has quietly broken behind a working slow one. The ordering and
reporting live in a pure function taking the fetch as a closure, so they are tested without a
network.

**Rejected:**
- One origin per source: breaks for people without the sibling, or ignores it for people with it.
- Reporting only the origin that answered: hides a broken fast path indefinitely.
- Keyword-tagged origins: every manifest author learns a keyword per kind.
