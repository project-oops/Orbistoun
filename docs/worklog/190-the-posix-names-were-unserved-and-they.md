# The POSIX names were unserved, and they were never missing behaviour


Asked what more the OS layer needs, and answered it from the corpus rather than from a list
of console features: four titles import fourteen system libraries this project declares
nothing for. The two biggest are graphics (318) and online (~190), and **neither is system
software** - worth knowing before deciding that "more OS" is what makes titles run further.

The OS-layer one is `libScePosix`, at forty-nine imports, none served. Not because the
behaviour was missing: a title imports `pthread_create` from `libScePosix` and
`scePthreadCreate` from `libkernel`, and only one of those was declared. A NID is the hash of
a name, so the other resolved to nothing while its twin had worked for months.

Twenty-four of the forty-nine have an implemented vendor twin. They now delegate to the same
function pointer, so nothing is duplicated and the spellings cannot drift.

### What the tooling caught, all of it mine

The delegation table failed its own test on the first run: the delegates are split across two
crates - threads in the kernel, files in the filesystem shim - and one was assumed to hold
them all. The test asserted an exact count rather than "at least one", which is why it failed
instead of quietly serving eighteen.

The knowledge entries were then rejected twice. `found_by = "static"` was wrong: the database
re-derives these as **`published-standard`**, which is *better* provenance than was claimed.
The guard cross-checks the claim against the database, and that is the second time today it
caught an overstatement rather than an understatement.

And a column mistake nearly produced a wrong answer: `symbols` prefixes implemented lines
with `*`, which shifts every field, so reading `$3` gave the library instead of the name and
reported seven implementations instead of two hundred and twenty-five. Caught because seven
was implausible, not because anything checked.

### Said plainly

Forty-nine declared, twenty-four served, **zero called** in any run this project can do
today - the title reaches the vendor-named twins and stops before the POSIX ones. Ahead of
demand, like the pad shim, and recorded that way so a count of twenty-four does not read as
something unblocked.

