# D050 - Revised starting order after real material arrived

**decided** · 2026-08-19

**Phase 0 shrinks and stops gating phase 1.** Its rationale was that no real container
could ever live in this repo, so one had to be synthesised. Real containers now exist
*on disk outside the repo*, exactly as intended - so synthetic fixtures are still
needed for malformed cases a compiler will never emit (truncation, offsets past EOF,
absurd counts), but the parser can be developed against genuine input.

**obSCEne is not needed before phase 4**, and two findings strengthen that:

- Its main early justification - being our first real container - has evaporated.
- The open toolchain is previous-generation, so it would emit `4f153d1d`-wrapped
  containers: the format we are *not* parsing first.

Homebrew (third-party corpus, phase 0d) and obSCEne (our own test app) are separate
things and it is easy to conflate them. Starting with homebrew does not require
obSCEne to exist.

**Recommended order**, all of it unblocked:

1. **0c - structural seams.** Before more code exists; cheap now, expensive later.
2. **0e - observability substrate.** So later phases feed the run report rather than
   retrofitting it.
3. **1 - the container parser**, against real material, starting with the **76 KB
   module** rather than a 68 MB executable. Same format, same wrapper, trivially
   inspectable.
4. **0 alongside**, reduced to malformed-case fixtures.

0b any time; 0d whenever homebrew is wanted in the mix.

