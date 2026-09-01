# D085 - The hash suffix is `supplied`, and the file now says so

**decided** · 2026-08-19 · prompted by the user asking where it came from

A fair question with an uncomfortable answer, and the answer belongs in the tree rather
than in a conversation that will not survive.

**It was recalled by a language model from publicly published material.** Stated in the
session with an explicit caveat that the recall might be wrong, then checked against real
imports - 66 of 468 published C names collided, where every other byte order and suffix
placement collided with none.

**It could not have been derived.** A sixteen-byte salt cannot be brute-forced, and a
known name-and-hash pair does not let you invert SHA-1 to recover it. Unlike every symbol
name in `symbols/`, this constant had to come from outside; nothing in this repository
could produce it.

**Which makes it `supplied`** - the exact category D073 defines for anything that came
from elsewhere, that never verifies, and that an audit lists loudly and separately. The
inconsistency was real: 352 names are audited on every commit, and the one constant that
makes all 352 work sat in a file describing it as "publicly documented and freely
available" with no record of how it entered the repository.

**Checkable is not the same as ours**, and the document blurred it. Both files now say
which is which.

Nothing about the value changes. What changes is that the provenance document no longer
has a hole where its own foundation should be - which matters more than it sounds,
because a provenance argument with one unexamined assumption at the bottom is worth
roughly as much as no argument.

