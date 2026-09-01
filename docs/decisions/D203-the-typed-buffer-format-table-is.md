# D203 - The typed-buffer format table is measured from the assembler, both directions


**Status:** decided (2026-08-21)

A typed buffer access carries a seven-bit format in bits 25:19 saying how many components
it moves, how wide each is, and how the bits become a number. Getting it wrong produces a
shader that runs and draws the wrong thing.

`orbistoun-gen buffer-formats` derives the whole table: it assembles `format:N` for every
code, reads the field back to confirm the code it asked for is the code it got, and records
the name the reference prints. The structure is in the name - `BUF_FMT_32_32_FLOAT` is two
thirty-two-bit components read as floating point - so parsing the name yields the meaning.

Same footing as the encoding and operand tables, and for the same reason: a transcribed
table cannot be checked without the document that produced it.

### The sweep has a blind spot, and it needs the opposite question

Codes that print no name are **two different facts**. Code 1 is the *default*, which the
disassembler omits exactly as it omits any modifier sitting at its default. Codes 78 to 127
are *reserved* and print back as bare numbers. Code 0 is explicitly `BUF_FMT_INVALID`.

Only the numeric sweep cannot separate the first from the second, and the default is a real
format a real shader uses. So there is a second pass asking the question the other way
round: build candidate names from the component layouts and types the first pass already
observed, assemble each **by name**, and see which code comes back. Nothing is invented -
every candidate is a recombination of parts already measured - and a name that assembles
into a given code is a measurement.

That recovered code 1 as `BUF_FMT_8_UNORM`. Reading it off the sequence would have got the
same answer and would have been a guess.

**It also needed a fix to be trustworthy.** The recovery takes the name from *what was
asked for*, which means pairing each output with its input line. Refused lines produce no
output, so pairing positionally without accounting for them shifts every later result by
one - silently, into answers that still look plausible. `assemble` now reads the
diagnostics for refused line numbers and carries the surviving index with each encoding.

### What the table refuses to say

Whether a format can be translated. A normalised eight-bit component is a real format
whether or not there is code to convert one, and omitting it would record the limits of the
table's first consumer as facts about the hardware. `is_plain_words()` is where a consumer
draws that line: all-32-bit components move words unchanged, anything narrower needs
unpacking and conversion, and that is the consumer's problem.

A code with no meaning returns `None` rather than the nearest real format. Picking a
neighbour would turn a broken shader into one that draws.

