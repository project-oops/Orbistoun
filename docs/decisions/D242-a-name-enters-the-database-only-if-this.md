# D242 - A name enters the database only if this repository can re-derive it


**decided** · 2026-08-25 · ruled before there was a reason to want the other answer

`obscene/docs/HARDWARE-PROBE.md` draws a boundary that is right and is written down: *what
a correct system does* travels freely - sizes, codes, layouts, behaviour - and *how another
implementation is written* does not. It does not cover **names**, and names are where the
pressure is: this project holds 737 and one executable still imports 565 it cannot name,
while name lists mined from other emulator projects exist and are reachable.

An argument for importing them is available and not stupid: a symbol name is a fact about an
ABI, so by the rule above it travels. **The answer is no**, and the reason is not legal.

`symbols-audit` reports a name as re-derivable *from this repository alone*, and that claim
is the whole product. A name arriving from an external list cannot carry it, and a database
mixing the two turns "unaccounted" from a signal that shrinks as the grammar learns into a
permanent population that means two different things. The audit would still pass; it would
have stopped measuring anything.

### What it forbids, precisely

- **An external name list may not be imported**, whether or not the hash confirms each entry.
  Confirmation establishes that a name is *correct*. It says nothing about where it came
  from, and provenance is the claim being made.
- **Nor as a candidate source.** Running someone else's 167,000 names through the hasher and
  keeping the hits is the same import with a filter in front of it. Every kept name would
  have arrived from outside; only the rejects would be ours.
- The bar is unchanged: a name is admissible when this repository's grammar, word lists and
  corpus can produce it and the NID hash agrees.

### Where this leaves the plateau

Every harvesting mechanism is at zero marginal yield, and this decision keeps it there until
a new *source* appears rather than a new list. The sources that qualify are named already: a
cited C++ ABI list, call-position inference, and a probe that answers `resolve` by name -
which reaches functions no title imports and which no collision search can ever match.

Deciding it now was the point. The choice costs nothing today and would have been made
badly on day three of a campaign with the naming yield at zero.

### A module's strings are one of its inputs, and whose module it is matters

The name search harvests identifier-shaped strings out of a module's own bytes. For a
commercial title that is unimpeachable and is how `sceKernelCreateSema` was found: the
string is the vendor's own spelling, sitting in the title's diagnostic text, and the hash
confirms it (D193).

Pointed at **obSCEne's** module the same mechanism does something else entirely. Its census
carries a name list mined from other emulator projects, so the strings in it are that list -
and a hit would be recorded as `Static`, from a module, which is among the strongest
provenance this project has. The front door refuses the list; this is the same list arriving
through a file.

So the rule is about the material, not the mechanism: **string harvesting is admissible from
a module built by the vendor and not from one built by another emulator project.** Word
lists and the generated grammar stay admissible everywhere, because a name they produce was
produced *here* - the hash confirming it is what makes it ours, whoever else happens to know
it.

Each hit records how it was found at the moment of discovery (D073), so this is a filter
rather than a matter of remembering.

### The unresolved edge, stated rather than smoothed over

`orbistoun-propose` asks a model for candidate **words**, never for a name, and keeps only
what the hash confirms (D214). A model has read the public internet, including the very
lists this entry refuses - so a word it proposes may be recalled from one. CLAUDE.md already
answers this in general: abstinence is unenforceable, **accounting is**, and a generated
record says it is a pattern and an index so anyone can re-derive it.

That answer holds for words and would not hold for names, which is the line this entry
draws. It is a real edge and it is recorded as one rather than declared clean.

