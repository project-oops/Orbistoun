# Phase 0 - Synthetic fixtures *(reduced; rejections already covered inline)*


A generator that emits containers byte-by-byte, covering the cases **a compiler will
never emit**: truncated segments, offsets past end-of-file, overlapping ranges, absurd
entry counts, a wrapper whose stated ELF offset lies outside the file.

**Originally this came first**, on the reasoning that no real container could ever
live in this repo so one had to be synthesised. Real material now exists on disk
*outside* the repo, exactly as intended (D050), so the parser can be developed against
genuine input and this phase narrows to the error paths - which remain essential,
since a parser is only trustworthy if its rejections are tested.

Lands under `tests/fixtures/synthetic/`, which `.gitattributes` and the CI provenance
guard already reserve.

**Observable result:** every malformed shape has a fixture and a test asserting the
parser rejects it with a specific error rather than panicking or accepting it.

