# D975 - Every module the translator makes is valid as spirv-val decides

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A translated module is valid SPIR-V for a Vulkan 1.3 device as `spirv-val --target-env vulkan1.3`
decides it, not as a driver does. A driver may accept an invalid module, miscompile it or fault on
it, and another driver may refuse what the first ran, so a module that dispatches and draws
correctly on one device has shown nothing about its validity. Grand Theft Auto V's kept
translations held 106 invalid modules of 1981, every one of which the development device ran.

**The validator.** `orbistoun_spirv::validator` runs `spirv-val` on a module's words and answers
valid, invalid with the validator's message, or unavailable. It finds the program at
`ORBISTOUN_SPIRV_VAL`, then in the `Bin` directory of the SDK `VULKAN_SDK` names, then on `PATH`.
Unavailable is never valid: a caller that asked and could not get an answer says so.

**The standing checks.** `orbistoun-translate`'s `validity` tests translate each shape a title's
programs were found to make invalid, and a program of each instruction the translator names at
each stage - a dispatch with the wave whole and one invocation a lane, a pixel shader, a primitive
shader - and validate every module; without a validator each says it skipped.
`orbistoun-cli shader-census <title> --validate` translates every program a title's store keeps
again with this build and validates each module, lists every invalid one by program and context
with the validator's message, records it in the TSV's `valid` and `invalid` columns, and exits
non-zero where any is invalid. The census is how a title's real programs are held to this rule; the
tests are how the translator is.

**What the builder holds.** Two rules no caller can keep reliably are kept by `Builder::finish`,
the way it already keeps one declaration per non-aggregate type:

- An entry point names every variable its interface must: every global variable from SPIR-V 1.4,
  every input and output below it. A translation declares a variable when the program first needs
  it - an image the first time one is read - after the entry point is written, so a list collected
  up front misses it.
- A fragment entry point's integer and 64-bit float inputs are decorated `Flat`
  (`VUID-StandaloneSpirv-Flat-04744`), built-ins among them: the subgroup index a pixel shader
  reading its quad declares is such an input, and the code declaring it does not know its stage.

A rule that depends on what an instruction means stays with the translation of that instruction:
a layered fetch's bounds choice (D897) selects between two four-component vectors with a condition
spread across four, at the texel's own type.

**What is refused.** Workgroup memory belongs to the compute and mesh stages alone
(`VUID-StandaloneSpirv-None-04645`). A pixel shader's invocations share none, so a pixel shader that
reaches its local data share is refused rather than given a module no device may accept. No title's
kept pixel shader does; the sweep over every instruction found it.
