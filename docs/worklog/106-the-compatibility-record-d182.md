# 2026-08-21 - The compatibility record (D182)


`Layer::Repo` existed in `orbistoun-overrides` as "shipped compatibility knowledge" and
nothing had ever written it. A title file said what orbistoun *sets*; nothing said what it
*got*. `compat/` now holds one tracked TOML per title with both halves in one file, because
two files keyed by the same title would disagree within a week.

The status half is derived from the trace, never typed - a hand-written grade drifts the
moment somebody is optimistic and nothing can check it afterwards. `compat record`
transcribes; `compat list` ranks; a run that beats the record says so and prints the command.

**Two things were found by running it rather than by reasoning.**

The first entry recorded was contaminated - the last trace on disk came from the run where
I had loosened the stub policy to demonstrate the reward hack. With no previous entry there
was nothing to compare against, so it went straight in as the baseline that no honest run
could then beat. `compat record` now refuses a blind-answering run outright, first entry
included.

The second: `Reach::Sustained` sorted GTA V's ninety-one-million-call spin over four
imports *above* the title that reached forty-seven and faulted. Surviving the time limit is
an outcome, not a distance, and it was already recorded as one. The rung is gone - which is
the same argument I had already accepted against a `Stopped` rung and then failed to apply.
Reading the populated table caught what the reasoning missed.

Also fixed the `cargo fmt` string-continuation trap for the third time, properly: `\` at
end of line inside a literal gets collapsed and the source indentation baked into the
rendered text. Replaced with `concat!` of one-line literals, which formatting cannot reach
inside. Note that `concat!` defeats implicit `{name}` capture, so arguments go positional.

