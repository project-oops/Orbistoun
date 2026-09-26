# D721 - Pad input captured and replayed against the guest's flips

**Status:** decided
**Date:** 2026-09-26

A pad script step can be timed in the guest's own flips (`at_flip = N`, counted from guest entry),
and a script uses one clock or the other, never both. Input is captured only when somebody asks -
the GUI toolbar's "capture input" or `Request::Run::capture_input` - as a script in that same
format, recording each state `scePadReadState` hands the guest and appending each step as it
happens. A run can name its own script (`orbistoun-cli run <module> --input <file>`,
`Request::Run::input_script`), which takes precedence over a script source in `config.toml`.

**Why:** a step timed by the host clock presses at a different point in the title whenever the
host's speed changes; the guest's frame count is its progress. What the guest read, not what the
window sent, is what replays exactly, through the same `scePadReadState` path live input takes.
Appending as it happens leaves a crashed run with a capture of exactly what led there. A script is
a property of one run, so it is a flag on the run and leaves the shared configuration, which holds
a person's controller setup, alone.

**Rejected:** capturing every live run - a person's input is theirs, and capturing it is their
decision, made visibly.
**Rejected:** capturing what the window sent - it includes presses the guest never polled.
**Rejected:** per-title overrides for scripts - one title is played by hand, replayed and tested
with different scripts.
