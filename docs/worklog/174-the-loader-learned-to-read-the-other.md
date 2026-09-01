# 2026-08-26 - The loader learned to read the other half of the world


A question about running the open-toolchain payloads turned into a correction of a recorded
measurement and then a working loader path. D305, D306.

### D163 said this route was closed, and it was wrong

> "No dynamic segment at all. Payloads resolve everything through the loader's own function
> table at run time, so there is no import list."

Twenty-three payload ELFs re-measured, and every one carries `PT_DYNAMIC`, `DT_NEEDED`, a
real `.dynsym`, and between 3 and 207 named undefined symbols. orbistoun's own refusal had
already changed - from "no PT_DYNAMIC segment" to "lacks a string table, symbol table, or
hash table" - and nobody read the new message.

**A stale measurement kept a whole class of guest out for six days.** Worth more than the
fix: the error was not in the reasoning, which was sound from what it had, but in nobody
re-running it when the tool's answer changed underneath it.

### The two blockers, one of them silent

`DT_GNU_HASH` states no symbol count, and twenty-one of twenty-three carry only that. Walking
the bucket and chain arrays gets it.

The other was worse. `imports_from_symbols` ended in `if let Some(decoded) =
decode_symbol_name(name)` and dropped anything that was not `NID#lib#mod` **without a word**.
So the two payloads carrying `DT_HASH` got past the first gate and reported:

```
entry 0x0
0 imports, 0 unresolved
```

A module needing eighty-five things, reporting that it needs none. That is the claim
principle 3 forbids an import list from making, and `Container::imports` has a guard for
exactly it which did not cover this path.

### Hashing the name is not a second scheme

The tempting design is two resolvers - NIDs for vendor modules, strings for homebrew. There
is no need: `libSceNet.sprx` exports `socket`, and the NID it publishes **is** the hash of
that name. A plain name is a NID nobody hashed yet.

**Checked before it was built**, because everything rested on it: `socket`, `bind`, `listen`,
`accept`, `malloc`, `pthread_create`, `memcpy`, `sysctl`, `kqueue` and `getifaddrs` are all
in this repository's own hash-confirmed database, so each already hashes to a NID that
appears in real vendor modules. The relation was established by the naming loop long ago;
this only stops throwing it away.

| payload | imports | nameable | registry answers |
|---|---|---|---|
| elfldr 0.25 | 24 | 24 | 8 |
| klogsrv 0.9 | 34 | 33 | 7 |
| shsrv 0.20 | 41 | 39 | 10 |
| ftpsrv 0.21.1 | 85 | 84 | 24 |
| pldmgr 0.5.1 | 160 | 159 | 42 |

`readelf` agrees on all five exactly. The first pass here was one too high on every row - a
trailing blank line counted as an import - which is a reminder that a number produced by
the thing being tested is not a measurement until something else has said it too.

**The naming problem was already solved for these.** One or two per payload are unnamed.

### Then it linked, entered, and stopped somewhere much later

`klogsrv` now parses, resolves, maps, relocates **completely** and enters. Six entry-setting
combinations later (D306): the stack convention makes no difference at all, and the first
argument register decides everything - the entry point calls through a pointer it takes from
`rdi`. Handed the image address it jumps to `1`, which is the argument count read as a
function pointer.

Stopped there rather than inventing the structure it wants.

### Two things I got wrong on the way

Told the user `sceKernelDlsym` was "the item that decides whether this is native or a hack"
and offered to start with it. **No payload in the corpus imports it** - or any dynamic-loading
function. Measured after asserting, which is the wrong order.

And the entry fault was read as a relocation problem for a while. It was not: `reached Linked`
only fires when the tally is complete, so the evidence that relocation was fine was already
on screen in the first run.

### And reading them turned up something the vendor corpus never had

Imports that name **data**. `__stderrp`, `optarg`, `__isthreaded` - `STT_OBJECT` symbols
reached through `GLOB_DAT`, two per payload in the small ones, and every one of them was
being handed a function thunk.

That is the worst shape of wrong answer available: the guest loads the slot, dereferences
what it found, reads x86 instruction bytes as a `FILE *`, and carries on. Nothing faults.
`st_info` states the difference outright, so it is now read and carried through to the
report (D307). Answering it correctly - a zeroed guest-owned block, the shape
`process_argument_block` already argues for - is left as its own decision, because it is
the first time the HLE layer would own state rather than functions.

### The gate, and why it never finishes

Two separate things, and one of them was me.

**Runaway orphans.** Two `orbistoun_propose` test binaries from earlier killed gate runs
were still alive at **5.2 and 1.2 CPU-hours**, pegging six cores and starving the gate that
was actually running. That is also what made the earlier run take 1.21 hours - it was
competing with its own orphans.

**The vocabulary suite is a production-scale sweep in an unoptimised build.** Tens of
millions of SHA-1 candidates per round, and the code says so itself - *"the sweep beside it
takes under a second in release"*. But `check` runs `cargo nextest run --workspace` in
debug, and `[profile.dev.package."*"]` optimises **dependencies only** - the sweep loop is
workspace code. Measured:

```
cargo test --release -p orbistoun-propose --lib    35 passed in 67.88s
cargo test          -p orbistoun-propose --lib     still running at 17 minutes
```

A `[profile.test]` line would fix it. Not applied - optimising deps but not workspace
crates looks like a deliberate line somebody drew, and this is a build-config decision
rather than part of the loader work.

**And the gate failure was mine.** `orbistoun_propose` exited `0xffffffff` - an external
kill, not a test failure. Clearing the runaways caught the live one too. Re-run in release:
all 35 pass. The other two failures were real and are fixed: `cargo fmt`, and a
`[`experiment`]` doc link left dangling by the D293 crate split, pointing at a module that
now lives in `orbistoun-turn`.


