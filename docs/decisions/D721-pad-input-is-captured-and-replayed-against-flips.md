# D721 - Pad input is captured and replayed against flips

**Status:** decided
**Date:** 2026-09-26

A pad script step can be timed in the guest's own flips (`at_flip = N`, counted from guest
entry), and a script uses one clock or the other, never both. Input is captured only on request,
from the GUI toolbar or `Request::Run::capture_input`, as a script in the same format that
records each state the pad-read call hands the guest and appends each step as it happens. A run
can name its own script (`--input <file>`, `Request::Run::input_script`), which takes precedence
over a script source in `config.toml`.

**Why:** a step timed by the host clock presses at a different point in the title whenever host
speed changes, and the guest's frame count is its progress. What the guest read, not what the
window sent, replays exactly through the path live input takes. Appending as it happens leaves a
crashed run with a capture of what led there, and a script belongs to one run, so it leaves the
shared controller configuration alone.

**Rejected:**
- Capturing every live run: a person's input is theirs to record, visibly.
- Capturing what the window sent: includes presses the guest never polled.
- Per-title script overrides: one title is played, replayed and tested with different scripts.
