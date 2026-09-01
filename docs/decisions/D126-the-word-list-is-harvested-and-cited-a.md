# D126 - The word list is harvested and cited; a rule I invented cost the most important name

**decided** · 2026-08-20 · at the user's direction, with the source to be documented

The published-standard word list is now read from FreeBSD's own `Symbol.map` files rather
than written from memory. 2,497 names against the previous 470, and matches against a real
import table went from 66 to **123**.

**Cited, not asserted.** `github.com/freebsd/freebsd-src @ 2ff0ca5…`, recorded in the
generated file's own header and in [REFERENCES.md](../REFERENCES.md) with the exact commands
to reproduce it. Nothing is vendored: a symbol map is a list of names and versions, no
implementation was read, and the derived list carries its own provenance so a reader never
has to look elsewhere to find out where it came from.

The names are not trusted either - each is hashed against a real module's import table and
only a collision counts. A wrong entry costs a wasted hash and cannot produce a false
result.

### The mistake worth recording

The first harvest **skipped every symbol beginning with an underscore**, on my reasoning
that reserved names are implementation detail rather than interface.

That excluded `__cxa_atexit` - the single most-called import across every title examined,
53.5% of all calls, and the wall this project had been stuck behind for a day.

Programs import reserved names constantly; the C++ ABI is nothing but reserved names. The
filter now keeps them and defers to the distinction **the format itself makes**: FreeBSD
marks implementation detail with `FBSDprivate_*` version blocks, which are skipped because
FreeBSD says so.

The general shape: given a source that states a distinction, use it. A plausible rule
invented on top of it will eventually disagree with it, and the disagreement will be
invisible - a name that simply never appears looks exactly like a name that does not exist.

### Two things the harvest exposed by being blocked

- **The citation named a temporary directory.** A local path is not a citation; nobody
  re-deriving this has that directory, and a `mktemp` one actively misleads. The revision
  now leads and the path is dropped entirely when one is given.
- **`orbistoun-cli` could not build**, because it links the whole workspace and another
  session was mid-edit in the shader translator. The harvest has no business depending on
  that, so it now also exists as an example in `orbistoun-names`, which depends on
  nothing but hashing. Worth remembering as a general shape: a tool that links everything
  is a tool that stops working for reasons unrelated to it.

