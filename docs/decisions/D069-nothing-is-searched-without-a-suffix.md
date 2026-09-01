# D069 - Nothing is searched without a suffix, and a miss on standard names indicts it

**decided** · 2026-08-19

`names` refuses to run without `--suffix-hex`. Without the real suffix every hash is
wrong, so the search grinds through the whole space and confidently reports nothing -
which reads as "these names are not it" when it means nothing of the kind. Refusing is
the honest answer (principle 3).

There is also a free check on the suffix itself, and the tool now states it. The
published standard-library names are fixed by standards rather than guessed, so if a
module links a C library and **not one** of roughly 470 of them matches, the names are
not what is wrong. Reported as a warning rather than a failure, because a module that
genuinely links no C library is a legitimate case.

Verified end to end with an arbitrary suffix against a real 733-import executable: 251
million candidates tried, zero named, and the warning raised - exactly the behaviour a
wrong suffix should produce.

