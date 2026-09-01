# tools

Offline generators and oracles. Nothing here runs as part of a build or a test: each of these
produces or checks material that is then committed, so the repository stays buildable on a
machine that has none of it installed.

**This file was destroyed and rewritten from the scripts themselves and from the references to
them in `docs/`.** If something you wrote here is missing, that is why - the originals of the
scripts are intact, only their index was lost.

## `toolchain/`

The VM the table generators run in. The generators need LLVM's reference assembler and
disassembler for the target GPU; those are not on every host this is developed on, and before
this existed the machine that had them was undocumented - which left the tables in
`crates/orbistoun-shader/data` readable by anyone and re-derivable by nobody.
[REFERENCES.md](../docs/REFERENCES.md) claims those tables are derived by experiment. This is
the experiment.

```bash
sh tools/toolchain/setup.sh                                   # create it, or bring it up
sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- operands
sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- fixtures
```

`run.sh` **fails rather than falling back to the host**: a generator that silently ran against
whatever assembler was on `PATH` is how a table acquires an unrecorded provenance.

Costs a VM and about 4 GB of disk. Nothing in the repository depends on it existing - delete it
with `multipass delete --purge` when you are done.

## `shader-fixtures/`

Assembly and LLVM IR the decoder is tested against, **generated from source, never extracted** (D089).
`families/` holds one file per instruction encoding family, which is what makes a gap in
coverage visible as a missing file rather than as a number nobody checks.

## `validate-spirv.sh`

Asks a real validator whether the emitted modules are valid SPIR-V. The emitter's own tests
check structure - magic word, bound, instruction packing - and cannot check validity, because a
crate asserting that it likes its own output proves nothing. `spirv-val` is the oracle, the same
way `llvm-objdump` is for the shader decoder.

Emitting and validating are split across two machines on purpose - the Rust toolchain on the
host, the SPIR-V tools in the build VM, neither needing the other:

```bash
cargo run -q --example emit-minimal -p orbistoun-spirv -- target/spirv/minimal.spv
multipass exec obscene-build -- sh /home/ubuntu/orbistoun/tools/validate-spirv.sh
```
