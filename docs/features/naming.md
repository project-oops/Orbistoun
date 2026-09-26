# Names and hashes

A guest imports functions by hash, not by name. Turning hashes back into names is why some
imports appear as a readable function and others as sixteen hex characters: a name is shown
where the symbol database holds one, and a bare hash where it does not.

## The hash

A **NID** is the first eight bytes, little-endian, of a SHA-1 over the function's name plus a
fixed suffix. It cannot be inverted, so naming is generate and test: propose a name, hash it,
compare. A match is proof rather than evidence, which makes a proposed name cheap to check and
impossible to get wrong silently. [SYMBOLS.md](../SYMBOLS.md) describes the algorithm and the
database format.

## Reading the count

The detail panel's imports line, `N of M named`, is how many of the selected title's imports
the database can name. `orbistoun-cli verify <title>/eboot.bin` prints the same measure. A
bare hash is an import nothing has named. Orbistoun can still implement behaviour behind it,
but a function without a name is one nobody can look up, so an unnamed import is worth
reporting.

## Commands

```bash
orbistoun-cli verify <module>                  # how many of a module's imports are named
orbistoun-cli names <module-or-directory>      # search generated names for the unnamed imports
orbistoun-cli harvest <freebsd-checkout>       # rebuild the standard-library word list from FreeBSD's symbol maps
orbistoun-cli nid <name>...                    # hash names
orbistoun-cli symbols --filter <text>          # list the libraries and functions orbistoun declares
orbistoun-cli audit <database>                 # re-derive every name in a symbol database
```

| `names` option | Does |
|---|---|
| `--words <file>` | also try each line of a file, verbatim |
| `--words-from probe\|supplied` | record where that list came from; `supplied` (the default) never verifies, and an audit lists it separately |
| `--from-trace` | also try strings a previous run captured from guest memory |
| `--from-report <report>` | also name hashes a conformance probe reported the platform exports |
| `--out <file>` | write the names found to a symbol database |
| `--wanted <file>` | write the hashes still unnamed, as a prioritised work list |

A directory is one search: the unnamed imports of every module under it are pooled, and every
module's strings are tried against all of them.

`harvest` reads only FreeBSD's `Symbol.map` files, so a sparse checkout is enough:

```bash
git clone --filter=blob:none --sparse https://github.com/freebsd/freebsd-src
cd freebsd-src && git sparse-checkout set lib/libc lib/libthr lib/msun lib/libutil
```

## Where names come from

Names come from sources that can be named: published documentation, standards, open-source
implementations such as FreeBSD, and this project's own generated vocabulary. A name enters
the database only if this repository can re-derive it (D242), and `audit` checks exactly
that. Names are not taken from vendor binaries or from other projects' lists.
[PROVENANCE.md](../PROVENANCE.md) describes how a name is shown to be this project's.
