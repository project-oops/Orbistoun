# A documentation sweep, because six units of change left claims behind


Went looking for statements that were true when written and are not now. Seven, and the
instructive part is that most were written *this session*.

- **D202 described an implementation that changed under it within the hour.** It said the
  exclusion was "a single bitmask, `mask | opcode`". Then D105's split opcode put a fourth
  opcode bit in the second word, where a first-word mask cannot reach, and the code became
  `{word: bits}` - without the decision that justifies it being updated. Corrected, and the
  general form in its title is the version to hold.
- **`reserved_bits_for` said "two sources" and had three.**
- **`encodings.toml` still told the reader every half-precision variant decodes as its
  counterpart**, six edits after that stopped being true, and still said the family was
  refused pending a resource model.
- **D202's inventory count** said twelve entries; it is seven.
- **REFERENCES.md was missing three derived tables** - per-opcode operands, mnemonics, and
  the new formats - so a third of the generated data had no provenance row.
- **D128 gained its second defect and did not mention it.** The modifier-stripping bug is
  the same failure as the one D128 was written about, pointing the other way, and that is
  worth having in the entry rather than only in a log.
- **TESTING.md had no row for the shader crates at all**, which is where most of the test
  weight now is.

### Surprises

**The stalest documentation was the newest.** Nothing from months ago needed touching. What
needed correcting was written in the last few hours - a decision recorded, then the code it
described edited twenty minutes later for a reason that seemed unrelated at the time. The
rule "record it as it is made" is right and it has this failure mode: a decision is written
once and the code keeps moving.

**Two of the seven were caught by grep for a phrase, not by reading.** Searching for the
claim rather than the topic - `"pending a resource model"`, `"single bitmask"` - found
things reading around the area had not. Worth doing deliberately after a run of changes:
list the sentences that would be wrong if the change is right, and grep for them.

**One entry was deliberately left wrong.** The worklog entry recording D105's gap says a
field cannot be written in two pieces, which is now false. It is annotated as overtaken
rather than rewritten - a log edited to look correct stops being evidence of what was
believed when.

