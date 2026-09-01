# D094 - Scalar destinations use the shared operand numbering

**decided** · 2026-08-19

Recorded as its own entry because it was got wrong, the mistake was invisible, and the
reasoning that produced it was superficially sound.

A scalar destination field looks like it should be a plain register index, and it was
first declared that way. It is not: scalar registers stop at 101, and the codes above
name the special registers. `s_andn2_b64 vcc, exec, s[2:3]` writes to the condition
mask *through that field*, and reading it as an index reports scalar register 106 - a
register that exists, in an instruction that decodes cleanly.

Only **vector** destinations are a direct index.

The differential test caught it on its first run, which is the entire argument for
having built it (D089) before building anything on top.

