# D085 - The instruction encoding table is data, and says it is unverified

**assumed** · 2026-08-19

`crates/orbistoun-shader/data/encodings.toml` rather than a `match` in Rust.

Principle 5's test applies unusually sharply here. Every row is a claim about
hardware, transcribed from a published specification, and at this volume
transcription errors are likely rather than possible. A wrong row does not fail to
compile - it silently mis-decodes, and the result reads as a shader using
instructions nobody understands rather than as a typo. As data, a correction is an
edit rather than a release, and can be checked against a real shader in seconds.

It also keeps the provenance answerable, because the two things are stored
separately: the table is a transcription of AMD's published instruction set
documentation, and the code around it is ours.

**The file states that it has not been verified line by line.** That is deliberate.
An unverified table presented as authoritative would have every downstream number
quietly wrong; an unverified table that says so has numbers that are honest lower
bounds. Verification is in the backlog.

Two structural guards make a mistake announce itself rather than hide: the table is
sorted by mask specificity at load rather than trusting file order, so a table
correct as a *set* is correct as a *sequence*; and a decode that runs off the end of
a buffer is reported, which is what a wrong instruction length looks like from the
outside.

