# 703. The compatibility scale now sees a missing picture

**2026-09-19** — inbox `-10ba`: the scale topped out at `flipped`, so six guests that reach the output
layer and show no pixel ranked at the top with 100% standing, and the table read as done. It asked for
a rung above `flipped` that cannot be reached without a framebuffer holding content, a test that a flip
over an empty framebuffer does not reach it, and regenerated tables. All three are now in the tree -
built by `-9b1f` (worklog 696), and this confirms 10ba's acceptance against them and closes it.

## What satisfies it

- **The rung.** `Reach::Presented` sits above `Flipped`, awarded by `status_of` when a flip's buffer
  was read back and found holding bytes the guest wrote - "a buffer whose pixels differ from what it
  held before the guest ran". That is exactly 10ba's "a rung that cannot be reached without a
  framebuffer containing content." Nothing weaker reaches it: the readback is a positive measurement
  against a known prior, which reaching the interface cannot fake (9b1f).
- **The negative test.** `a_written_frame_reaches_presented_and_an_unwritten_flip_stops_at_flipped`
  asserts a flip with nothing written stays at `Flipped`, not `Presented` - 10ba's "a flip over an
  empty framebuffer does not reach it", watched failing before it was trusted.
- **The tables.** `COMPATIBILITY.md` now names the full ladder through `presented` ("the top rung …
  nothing in the corpus has reached it"; `flipped` is "a place reached and not a picture shown"), and
  all six guests - PPSA99980, PPSA03416-app0, PPSA02664-app0, obscene, PPSA25872-app0, obscene-payload
  - read `flipped`, one rung below the top. `PROJECT_STATUS.md` agrees, and `status --check` is exit 0,
  so both are current with the rung.

So every guest now ranks one below the top, which is the accurate reading, and the next stretch - a
guest that actually presents - is measurable in a way `flipped` could not express. That was 10ba's
whole point.

## The one word of the acceptance the loop does not do

10ba asks for "a commit adding the rung". The rung, its arm, its test and the regenerated tables are
all in the working tree; the commit is the operator's, as it is for every unit this run resolves. The
substance is delivered and verified.

## Gate state

No code changed here - this is a verification and a close. `status --check` exit 0 (the generated
compat tables carry the rung and the six flipped guests); `./bin/orbistoun check` was green as of
worklog 702 this session and nothing was touched since. Corpus unchanged. No commit.
