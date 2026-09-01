# D255 - Thirty-five per cent of what the model proposed was already in the shipped word list


**decided** · 2026-08-25 · measured, and it changes what to ask for

Of the twenty-six distinct words a model proposed and swept over thirty-six rounds, **nine
appear inside a name in `standard.txt`** - including `Group` and `Node`, the two it repeated
most. Three of the five words it has ever banked are in there: `Unset` in `unsetenv`,
`Object` in `kinfo_getvmobject`, `Resource` in `setclassresources`.

Those names are tried whole and never decomposed into parts, so the words are present in the
tree and unavailable to the grammar - which a parallel line of work found independently from
the other direction. Once that list is decomposed, a third of what this model produces
arrives for free and asking for it again buys nothing.

What no standard name contains is the vendor's own vocabulary. The same run produced `Dma`,
`Midi`, `Bios`, `Endpoint` and `Bandwidth` unprompted, and that two-thirds is the part worth
paying for. The prompt now says so in one sentence.

**Two things this rules out, and both were tempting.** Morphological *mutation* - `Group` to
`Groups`, `Grouping` - is a loop over words that already exist, and a loop does it
exhaustively in milliseconds; spending a five-to-twenty second model call on it is D231's
measurement again. *Combination* is what the grammar patterns already are, so more of it is
a line of TOML rather than a question for a model. The model's job is the words a loop
cannot invent, and nothing else.

