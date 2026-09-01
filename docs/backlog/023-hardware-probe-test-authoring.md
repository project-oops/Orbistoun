# Hardware-probe test authoring

The strongest oracle for an individual function is a probe run on real hardware:
write a small program that exercises edge cases, run it, record the behaviour, encode
it as a test. Other projects have done this for years.

**This is now `obSCEne` (D043, D045)** - a separate repo, so the authoring half is
committed rather than backlogged. What stays here is the *running* half: pointing it
at real hardware requires hardware nobody has, and until then the suite grades an
implementation rather than producing ground truth.

**The stronger version is D056** - a *remote-controlled* mode, so calls can be issued
interactively and answers read back, rather than authoring a fixed probe per question.
That is the highest-value capability on any list here, because it converts the
project's central constraint from inference to measurement. Gated on hardware, but it
should shape obSCEne's structure from the start.

