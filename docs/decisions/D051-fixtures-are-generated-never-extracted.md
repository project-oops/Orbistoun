# D051 - Fixtures are generated, never extracted

**Status:** decided
**Date:** 2026-09-26

Nothing derived from a title is committed - no bytes, header, trimmed copy or hex dump. A
fixture that resembles a real container is built by a generator from observed structure.
Generated shader fixtures use `.gcn`; a shader dumped from a title keeps `.bin`, which the
provenance gate bans.

**Why:** committed fixtures are exempt from the provenance gate's extension rules, so the gate
cannot see a carved fixture; the rule is held by the generator being the only route. The two
shader extensions carry opposite obligations - one must be tracked, the other never - and
splitting them keeps the gate exactly as strict.

**Rejected:**
- Saving the first bytes of a real module as a fixture: console-derived material in the tree.
- A path exception in the provenance gate: weakens the gate to fix a naming collision.
