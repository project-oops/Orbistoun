# 2026-08-19 - The execution model, decided and stubbed


**Done.** The concept flagged as needing input (D096 aftermath) came back as
*predicated, with structured stubbed loudly*, and the foundation is laid.

- `orbistoun-spirv`: a module builder that emits words directly - no assembler, no
  text. Identifiers are allocated by the builder so the header bound cannot disagree
  with them.
- `orbistoun-translate`: the strategy seam. Predicated is a skeleton that reports
  translating zero instructions; structured is an error that says so at length.
- `tools/validate-spirv.sh`: `spirv-val` as the oracle, emitting on the host and
  validating in the VM so neither machine needs the other's toolchain.

**Verified.** Gate green. The minimal compute module **validates first try** and
disassembles exactly as intended - every opcode number proposed from memory was right,
which `spirv-val` is in a position to confirm and this crate is not.

**Surprises.**

- **A one-off gate failure in `orbistoun-abi`, a crate untouched by any of this.** It
  passed 8/8 on its own and three subsequent workspace runs were clean. The plausible
  cause is contention between test binaries that reserve *fixed* addresses - `abi`
  executes generated code at a fixed address and `mem` reserves ranges - and adding two
  crates changed how many run concurrently. Not chased, because it did not reproduce,
  but it is a latent fragility rather than noise and is now in the backlog.

- **`spirv-val` lives in the VM and `cargo` does not.** Splitting emit from validate
  across the two machines is better than installing a Rust toolchain in the VM, and it
  falls out naturally: the emitter writes a file, the validator reads one.

**Not done.** The predicated translator translates nothing - it emits a correctly
shaped module and reports zero instructions, so nothing downstream can mistake a
skeleton for a translation. Register modelling, the dispatch loop, and instruction
emission are all ahead. Structured reconstruction is deliberately absent.

