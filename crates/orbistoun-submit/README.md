# orbistoun-submit

What one machine has to contribute, gathered into one directory.

The loop does not need this repository, and that is the point: the third oracle in
`CLAUDE.md` is the guest itself, expensive per query and **expensive per person**. Somebody
running a binary against a title nobody here owns is turning the same oracle.

This crate holds what a submission *is* - measurements and title results - and how a
receiving machine checks one: by **re-deriving it locally and comparing**, never by trusting
it. It has no path to the emulator, the loader or a model runtime, so a bundle cannot
smuggle behaviour; it carries claims.
