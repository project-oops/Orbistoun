# D279 - A dump is binary, and the size guard was measuring the wrong thing


**decided** · 2026-08-25 · `check` stopped at a 6MB file that is the audit trail itself

The provenance guard fails any file over a megabyte outside `assets/`, on the reasoning that
this is "the shape a dump takes even when the extension is disguised, which is the case an
extension list cannot catch". It began failing on `symbols/generated.json`, which had grown
from 655 names to 30,086 as the strings reader worked through the probe module.

That file is the **opposite** of a dump. It is UTF-8 JSON, every entry a name beside a
derivation record saying where it came from and on what date - the artefact
[PROVENANCE.md](../PROVENANCE.md) requires in order to answer the question this guard is
protecting. Failing on it is the guard firing on its own evidence.

So the rule now distinguishes what it was always trying to distinguish: **a large binary file
fails; a large text file is listed and does not.** A firmware dump, a decrypted title, a
disguised container - all binary. A guard that says "over a megabyte" catches those by
accident of size, and catches an audit trail by the same accident.

**What protection this gives up, stated rather than glossed:** a large *text* file that is
genuinely console-derived - a hex dump, or a symbol list lifted from somebody else's database
- no longer fails on size. That is acceptable here and only here, because the sharp
instrument for exactly that case already runs two steps later in the same command:
`symbols_audit` re-derives every name in `symbols/*.json` from this repository's own inputs
and refuses the ones it cannot (D242). Size was the crude proxy; content-aware re-derivation
is the real check, and it was already there.

Large text files are still printed on every run, because a guard that stops saying anything
is how a threshold becomes a thing nobody remembers.

