# D352 - The harvest took names because the work was naming


**decided** · 2026-08-27

`docs/REFERENCES.md` says it plainly: *"Symbol **names**, and nothing else… It contains no
implementation, no structure layouts, and **no constants**."* The FreeBSD checkout is
shallow, `blob:none`-filtered and sparse to `lib/` for exactly that reason.

That was right. The naming loop needs names - a NID is a hash of one - and 22 MB bought
2,497 of them.

**The work has moved from naming to implementing, and implementing needs numbers.** Eight
functions went in before this and the gap cost three answers in one session (D343, D350):

- `SIGPIPE` was recovered from the **guest's own call argument**, which was luck.
- A `sysctl` MIB was recorded as `[1, 14, 8, 0]` with its meaning left open.
- `errno` was left unset, because `ENOENT`'s value was not derivable from anything lawful
  here - and setting an invented one is how a wrong constant gets copied.

Sockets would have been much worse. `socket(AF_INET, SOCK_STREAM, 0)` cannot be mapped onto
a host socket without knowing what those are, and a wrong value creates the wrong kind of
socket: a silent, late failure of precisely the shape principle 3 exists to stop.

### Widening is within the boundary, not an exception to it

A `#define AF_INET 2` in a public header is an **interface fact** - the same category as a
symbol name, and arguably more clearly so, because it *is* the ABI. It is not implementation
and not a structure carrying creative choices. FreeBSD is BSD-2-Clause and `CLAUDE.md` names
it oracle #1, *"lawful, citable, and the strongest reference available"*. The old
restriction was self-imposed and tighter than the principle requires.

Sparse to `lib/` plus `sys/sys`, `sys/netinet` and `include`: 22 MB to **25 MB**. 903
constants from six headers into `crates/orbistoun-hle/data/abi-constants.toml`.

### The measurement and the citation agree, and neither came from the other

Four constants were confirmed against measurements taken **before** the harvest existed:

| constant | header | measured independently |
|---|---|---|
| `SIGPIPE` | 13 | `klogsrv` passed `0xd` to `signal` |
| `CTL_KERN` | 1 | MIB[0] of the dumped call |
| `KERN_PROC` | 14 | MIB[1] |
| `KERN_PROC_PROC` | 8 | MIB[2], from a caller the symbol table names `find_pid` |

`KERN_PROC_PROC` is commented *"only return procs"*, so `[1, 14, 8, 0]` is **enumerate all
processes** - which is what a function called `find_pid` would ask for. D350's open question
is answered, and answered by two routes that never touched each other.

### Two things that keep it honest

**They are FreeBSD's numbers, not the target's.** The target is FreeBSD-*derived*, which is
why they are worth having and also why they are not facts about it. Each is `published`
about FreeBSD and `assumed` about the guest; the data file says so in its own header, and a
guest passing a value that disagrees is what would show it.

**Read, never retyped.** `abi_constant` parses the file. A value copied into Rust would be
**untraceable** - a reader could no longer tell a harvested constant from a remembered one,
which is the entire distinction `known_by` keeps. A test pins `SOL_SOCKET` at `0xffff`
precisely because it is `1` on several other platforms: a table built from recall would
differ there first, and that test would catch it.

The guest confirms the whole path. `main.c:278:sysctl: error 0` became **`error 2`** -
harvested `ENOENT`, through this project's `__error`, into the guest's own diagnostic.

