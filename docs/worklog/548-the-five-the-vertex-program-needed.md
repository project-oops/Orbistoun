# 548. The five the vertex program needed: named by the reference, four translated, one blocked

**2026-09-14** - orbistoun-gen, orbistoun-shader and orbistoun-translate, after worklog 545

Worklog 545 fed the GL cube's oracle records through the submission pipeline and stopped at
byte 0 of the vertex program: `s_inst_prefetch` had no name, so there was nothing to
dispatch on. Five opcodes were in that state, all of them the vertex program's. They are
named now, by the reference assembler and disassembler in the toolchain VM, which is the
only way a name enters this repository.

| family | opcode | name | operands | translated |
|---|---|---|---|---|
| SOPP | 0x00 | `s_nop` | 16-bit immediate | yes, as nothing |
| SOPP | 0x10 | `s_sendmsg` | 16-bit immediate | **no - blocked, §4** |
| SOPP | 0x20 | `s_inst_prefetch` | 16-bit immediate | yes, as nothing |
| VOP2 | 0x16 | `v_lshrrev_b32_e32` | vgpr, source, vgpr | yes |
| VOP2 | 0x28 | `v_add_co_ci_u32_e32` | vgpr, implicit `vcc`, source, vgpr, implicit `vcc` | yes |

The corpus census moved from 186 of 200 instructions translatable to **196 of 200**, and
says `FURTHER`, clearing all five keys. The oracle test's vertex program now runs its
prefetch, both moves and the nop, and stops at `0x14` - the send-message, instruction six -
in both records.

## 1. Two new sources, and why they are two

`tools/shader-fixtures/primitive.s` is the fixture: the vertex program's prologue in its own
order, then more of each instruction so every length is asserted by something following it.
It is hand-written and assembled, like `unreached.s`, and for the same reason - no compiled
fixture reaches these - with the same weakness stated there: the instructions are ones
somebody thought of, not ones a compiler reached for. The reference still decides the bytes
and the boundaries, so a wrong length or opcode field fails exactly as it would for a
compiled one. Registered in `differential.rs`'s fixture list, which is checked against the
directory, so it cannot be present and unread.

`tools/shader-fixtures/probes/primitive.s` is the operand probe. The three SOPP immediates
sample the full sixteen bits, because the assembler accepts `s_nop 0xffff` and
`s_inst_prefetch 0xffff` - measured, not assumed - and probes that stayed near the values the
shader uses would leave a two-bit and a three-bit window explaining every sample as well as
the real one. The solver refuses an ambiguous width rather than picking, so the probe has to
reach.

`s_waitcnt` remains the one unsolved opcode in the family, as before: it packs three
counters into one field and the reference prints whichever are not at maximum.

## 2. The send-message code is measured, like the export targets

The reference prints a message it knows by name - `sendmsg(MSG_GS_ALLOC_REQ)` - which is text
no bit field explains, so the probes use values it prints as numbers and the differential
test carries the name-to-code mapping. That mapping is one entry, `9`, and it was measured
rather than transcribed: the assembler encodes the instruction as `0xbf900009`, and the
console-run vertex program's word at that position is `0xbf900009`. Same measurement, two
independent producers.

## 3. What the four translations are

- `s_nop` and `s_inst_prefetch` emit nothing, matched explicitly beside `s_endpgm`,
  `s_waitcnt` and `s_clause`. The nop is wait states for a hazard the host does not have;
  the prefetch is a cache hint with no architectural effect, so a shader with it and one
  without compute the same thing. That is what makes dropping them a translation rather than
  a shortcut, and why "emits nothing" and "nobody handled it" are still different arms.
- `v_lshrrev_b32_e32` joins the short-form arithmetic, as a **logical** shift right. The
  guest has a separate `v_ashrrev_i32` for the sign-propagating one, so reading this as
  arithmetic would be right for every address and wrong for every negative value.
- `v_add_co_ci_u32_e32` joins the carry arithmetic. The long form was already translated and
  the short form differs only in leaving its carry registers implicit at `vcc` - which the
  solved layout records as implicit operands in the same positions, so the existing code
  indexes both the same way. The only thing that had to change was the question "does this
  name take a carry in", which is now a function rather than one constant.

## 4. `s_sendmsg` is blocked, and that is the honest answer

It sends a message to a fixed-function unit outside the shader core, and what the message
means is the whole content. The vertex program uses `MSG_GS_ALLOC_REQ` to tell the geometry
engine how many vertices and primitives the wave will export; the `exp prim` two
instructions later is legal only because that allocation was granted. A host vertex shader
makes no such request - the driver sized the output before the shader ran - so there is
nothing to translate it into, and translating it as nothing would drop the instruction the
exports after it depend on.

So it is in `BLOCKED` with that reasoning, which puts it in the worklist's "waiting on a
subsystem" section rather than at the top of the ranked list - the distinction that list
exists to draw. It waits on a decision about what a primitive shader is to this translator,
not on encoding work. That decision is still the next thing, unchanged from 545.

## 5. The toolchain VM could not mount the repository, so the tool was fixed

`tools/toolchain/run.sh` assumes `setup.sh` mounted the tree at `/home/ubuntu/orbistoun`.
Multipass on Windows ships with `local.privileged-mounts` off, and enabling it is a
privileged machine-wide setting a repository script has no business changing, so the mount
silently never happened and every command failed inside the guest with
`cd: /home/ubuntu/orbistoun: No such file or directory` - which reads as a broken VM.

`tools/toolchain/sync.sh` is the way round it: `push` copies the working tree in, `pull`
brings named files back, `verify` compares. `run.sh` now detects the missing mount and says
all of that instead of failing in the guest. Three things the sync had to learn, each from
breaking:

- **The sibling repositories come too.** This workspace has path dependencies on
  `../oops-libs` and `../selfish`, and without them cargo fails on an unreadable manifest.
  They are read out of the manifest rather than listed, so a dependency that moves does not
  leave the script pushing a set that no longer builds.
- **A file transfers to its full destination path, never to its directory.** Naming the
  directory truncated `mnemonics.toml` at exactly 4096 bytes, reported success, and delivered
  a table missing a quarter of its rows - which read as a generator that had lost them, and
  cost a real investigation before the byte count gave it away. `-` as a destination is no
  better: the shell's redirect turns 5218 bytes into 5566 by adding carriage returns.
- **The exit status is not the oracle; the bytes are.** `multipass transfer` fails on this
  host for every file whose permissions it cannot set on an NTFS volume, having copied the
  contents correctly. So the pull checks every file by checksum on both sides, and that
  comparison is its own subcommand precisely so it can be made to fail on demand - truncate a
  file, run `verify`, watch it refuse. It was made to fail, twice: once on a truncated table,
  once on its own first version, which sorted the two sides' file names under different
  locales and reported identical files as corrupt.

A directory now arrives through a staging directory and is swapped in, because the first
version deleted the local copy before a transfer that then failed - which lost the whole
recordings directory until `git checkout` brought it back.

## 6. Provenance

Names came from `llvm-mc` and `llvm-objdump` in the VM, which is where every name in
`mnemonics.toml` comes from; operand layouts were solved from assembled probes, not
transcribed. The instruction words in `primitive.s` are oops-sdk's own, written from the
published instruction set, and the send-message code agrees between the assembler and the
console. Nothing was disassembled from a vendor module and nothing was taken from the
repositories audited in worklog 541.

## Files

- `tools/shader-fixtures/primitive.s`, `tools/shader-fixtures/probes/primitive.s` - the two
  new sources.
- `crates/orbistoun-shader/tests/fixtures/primitive.{gcn,txt}`,
  `crates/orbistoun-shader/data/mnemonics.toml`,
  `crates/orbistoun-shader/data/opcode-operands.toml`,
  `crates/orbistoun-gen/tests/fixtures/transcripts/` - generated, and the recordings that let
  `./bin/orbistoun tables` replay the solve with no toolchain.
- `crates/orbistoun-shader/tests/differential.rs` - the fixture row and the measured
  send-message code.
- `crates/orbistoun-translate/src/model.rs` - four names supported, one blocked.
- `tools/toolchain/sync.sh` (new), `tools/toolchain/run.sh`, `crates/orbistoun-gen/README.md`.

## Next

1. `image_sample_lz` (MIMG 0x27) is now the only thing between the textured pixel shader and
   a translation: it is decodable and unnamed, and MIMG has no operand layout for any opcode.
   The same two files this unit added are the shape of the fix, and the toolchain path is
   working now.
2. The flat base code `0x7d` - `REQ-20260914T1402Z-b6d8` on the obSCEne bus, still open.
3. Decide what a primitive shader is to this translator, which is what `s_sendmsg` and
   `exp prim` are both waiting on.
