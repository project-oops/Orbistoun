# 2026-08-24 - `orbistoun-gen`: the table generators, and the seam that makes them checkable (D209)


**Done.** `crates/orbistoun-gen` - five commands solving the shader data tables from
assembled probes, 65 tests, and a replay check that runs in `./orbistoun.sh check`.

### The constraint everything follows from

The solvers need `llvm-mc` built with the AMDGPU target. **No machine in this project's
normal setup has one** - the LLVM most hosts carry omits that target, and on Windows
`llvm-mc` is not shipped at all. `tools/toolchain/setup.sh` builds a VM that has it.

That is worse than inconvenient. Nothing could check that a committed table still matched
what produces it, so a hand-edited row would have survived indefinitely - and a table of bit
fields is exactly the kind of thing somebody edits when a value looks wrong.

### The seam

`assembler::Source` is a live `llvm-mc` **or a replay of a committed recording of one**.
Replay needs nothing installed, so `./orbistoun.sh tables` regenerates and diffs anywhere.
772 KB of recordings cover the two generators whose output is committed; `encodings` writes
no file to diff and its sweeps are 9.5 MB, one file of which exceeds the provenance guard's
own 1 MB limit.

Verified against a live reference assembler in the VM before being trusted: `buffer-formats`
and `operands` byte-identical, all ten `.gcn` fixtures byte-identical, and `encodings` -
which writes nothing - matching on both output and exit status, including the two families
it cannot solve. `SOPC` and `SOPK` fail the same way every time; that is the solver refusing
rather than guessing.

### The bug the replay found and a live run could not

Replaying produced a **different** operand table: `EXP:0` and `VINTRP:2` gone.
`derive_symbolic_codes` makes forty-seven probes and every one used the same recording key,
so replay handed all of them one canned answer; the codes came out empty and the two families
needing them dropped out.

Invisible live - each call re-invokes the assembler and gets the right answer. Wrong only on
replay, which is the mode that has to be trusted. Keys are content-derived now, so a
recording matches by *what was asked* rather than by the order it was asked in.

**The diff caught it**, which is the entire argument for having one.

### The sharpest edge

`fixtures` and `operands` on a machine with no toolchain skip every source - and the skip
path has already deleted each `.txt` on the way past. Without a guard the run then writes a
`mnemonics.toml` with no mnemonics in it, prints "0 fixtures", and exits zero. The reference
output the differential suite exists to compare against would be gone, silently, on most
machines.

Both refuse now, naming the missing tool. The test holding them to it runs *because* this
machine is in that state.

### A dead-code warning that was a real bug

`split_operand` came out unused. The operand solver needs its **own** splitting - modifiers
dropped, `attr3.y` split into the two fields it is, `offset:16` reduced to its value - and
the shared whitespace splitter is right for the encoding solver and wrong here. Wired to the
shared one, every flat access, every typed buffer access and every interpolation would have
reported as unsolvable, with the solver hunting for bits encoding the word "off".

A dead-code warning is a weak signal for a bug that size. It was the only one there was.

### Also

- The prose guard fired on a line-continued literal in the new crate and was the reason it
  became a `concat!` before it reached anything.
- `tools/toolchain/setup.sh` installs Rust, and records why apt's will not do (Ubuntu 24.04
  ships 1.75; this workspace is edition 2024) and why `CARGO_TARGET_DIR` is required - the
  repository is a mount, a build script written there cannot be executed, and the failure is
  a bare "Permission denied" naming a path under `target/` that reads as a corrupt checkout.
- `./orbistoun.sh site` no longer suggests a way to serve the page. It is static HTML whose
  only script is a `document.write` of the year, so `file://` renders it exactly as Pages
  does.

### One process note worth keeping

Verification was reported as *blocked on a toolchain VM*. **The VM already existed** - named
in `tools/toolchain/run.sh`, sitting stopped in `multipass list`, and `setup.sh` starts it if
it exists. Nobody had looked.

The failure is not being wrong about the state; it is making a confident statement about a
state nobody examined, which is the same shape as a guard reporting success without checking.
A session's work was deferred on it.

