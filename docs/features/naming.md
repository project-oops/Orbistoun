# Names and hashes

A guest imports by **hash**, not by name. Turning those hashes back into names is a large part of
what Orbistoun does, and it is why some entries in the library show a readable function and
others show sixteen hex characters.

## Why it is hard

The hash is the first eight bytes of a SHA-1 over the function's name plus a fixed suffix, big
endian. It is one-way. There is no table to look the answer up in, and nothing to invert.

So naming is **generate and test**: propose a name, hash it, compare. That sounds hopeless and is
not, because the check is exact. A collision is not evidence - it is proof. Nothing else in this
project gets an oracle that good, which is why the naming work is worth doing at all.

## Doing it

```bash
orbistoun names            # what is named and what is not
orbistoun harvest          # take names from a lawful source and test them
orbistoun learn            # record what was established
orbistoun ask              # ask a model for vocabulary to try
```

`harvest` reads published sources - FreeBSD's own symbol maps, for one - and tests every name in
them. It is cheap and it is where most answers come from.

`ask` exists because the remaining names are not in any list. When the shapes are known but the
*words* are not, a model proposing candidate vocabulary is genuinely useful: every suggestion is
checked by the hash, so a wrong one costs nothing and a right one is proved. This is the one
place in the project where a model's guess is admissible, and it is admissible precisely because
nothing is taken on trust.

## What a bare hash in the library means

The title imports something nothing has named yet. That is not necessarily a blocker - Orbistoun
can implement behaviour behind an unnamed hash - but an unnamed import is one nobody can reason
about, so it is worth reporting.

## Where names may come from

Names come from **sources that can be named**: published documentation, open-source
implementations, and standards. The FreeBSD lineage of the platform's C library makes a great
deal of it legitimately knowable.

Names are **not** taken by reading vendor binaries, and every recorded behaviour carries a field
saying how it came to be known - `published`, `measured`, and so on. That accounting is what
makes the work shareable rather than merely usable, and it is checked by the build rather than
left to good intentions.

If you contribute a name, the source matters as much as the answer.
