# Phase 6's contents, built ahead of it


Phase 6 is reached when a guest runs far enough to submit a command buffer. That is
gated on phases 4 and 5. The **contents** of phase 6 - decoding shader bytecode,
translating it, and turning a submission into something a backend can run - are not
gated on anything, and are being built in parallel so that when the first submission
arrives the loop is already fast.

This deliberately runs ahead of the dependency spine (principle 6), which is a real cost
and worth stating: **everything below is verified against material generated here.** It
cannot be wrong in any way its own tests would notice. The whole point of the ordering is
that the first real submission is a *check*, not a beginning.

The endpoint is unchanged: framebuffer diffing, the only cheap mechanical oracle this
project will ever have.

| Step | What | State |
|---|---|---|
| **G1** | Instruction decode; every encoding family checked against a reference disassembler | **done** - 18/18 families, differentially checked over 10 fixtures |
| **G2** | Per-opcode operand layouts solved from probes, not transcribed | **done** - 66 opcodes, zero probes rejected |
| **G3** | SPIR-V emission, with the module checking its own identifiers | **done** - and it has since caught a malformed new opcode before a driver saw it |
| **G4** | Translation: registers, guest memory, execution mask, comparisons, per-lane divergence | **done** |
| **G5** | Control flow, as a dispatch loop rather than reconstructed structure | **done** |
| **G6** | Execution on a real device, as the test oracle | **done** - 225 tests on the shader side, a large fraction executing |
| **G7** | Instruction breadth | **87%** - 110/127, 6/10 fixtures complete; **unblocked work now genuinely exhausted** |
| **G8** | Submission pipeline: packets to register writes to a shader to a running module | **structure done**, synthetic input only; survives arbitrary and truncated streams |
| **G9** | Packet vocabulary verified against something external | **harness done**, corpus empty - needs a capture |
| **G10** | Resource model: descriptors, buffers, images, render targets | **not started**; its *guest side* is documented and no longer needs a capture - see below |
| **G11** | Graphics pipelines in the Vulkan backend; today it dispatches compute only | **not started** - needs G10 |
| **G12** | Framebuffer diffing | **not started** - needs G11 and a guest that draws |
| **G13** | Subgroup fidelity, the level that would actually be used at speed | **done** (D146) - one invocation per lane, mask by ballot; reports the subgroup width it needs |
| **G14** | Performance: collapse the single-block dispatch loop, persist the shader cache | **deferred** until there is something to measure |

### What each remaining step is waiting on

**G7 - instruction breadth.** The unblocked work is done. What used to be listed here as
"blocked on documentation" - the three division helpers - is translated and executing
(D143, D144); the published reference for this generation was fetched for the encoding
families and turned out to specify all three. The last instruction that needed nothing
structural, `v_fmac_f32`, is done too.

Everything still refused needs the resource model or the graphics pipeline: `exp`, the
typed buffer accesses (MTBUF), parameter interpolation (VINTRP) and image sampling (MIMG).

**Down to ten worklist entries and three fixtures**, from thirteen and four. The untyped
accesses (MUBUF) are translated, and the typed ones are decoded - operands solved for all
five opcodes, and the format field measured - with only their translation left. Updated
2026-08-21.

**G10 - the resource model** was listed as needing a real submission. That is now only
half true, and the half that changed is worth acting on.

Its *guest* side is **documented**: a buffer resource descriptor is a 128-bit value held
in four consecutive scalar registers, its fields are named, and the shader-visible use of
them is given bit by bit. Building that decoder unblocks MUBUF and MTBUF without a capture
- six of the thirteen remaining blockers and two of the four remaining fixtures.

**That prediction held.** MUBUF is translated through exactly that decoder, and MTBUF is
decoded on top of it: the descriptor and the addressing are shared, and what a typed access
adds is a format conversion. Its operands are solved and its format table is measured
(D203); the translation is what remains, and it needs no capture either.

One caveat, checked rather than assumed: the general buffer *addressing equation* is a
**figure** in the published PDF, so a text extraction drops it. Reading it means reading
that page as an image, which is a step rather than a given.

Its *host* side - what a descriptor becomes in Vulkan - is designable but not verifiable
without real data. `exp` and VINTRP need render targets and fragment inputs, which is G11.

**G11 - the graphics pipeline**, and with it `exp`, which blocks more shaders than
anything else on the worklist and is the reason no fragment shader translates.

An export is not a memory write. `exp mrt0 v0,v1,v2,v3` means *this is colour zero*, and
the destination is a **render target** - a thing that exists only inside a graphics
pipeline. Three things stand between here and there:

1. The translated module has to be a **fragment** shader: execution model `Fragment`, with
   output variables. Every module emitted today is a compute dispatch writing a storage
   buffer.
2. The backend has to build a **graphics pipeline** - render pass, attachments, a draw. It
   dispatches compute and nothing else.
3. Something has to say **which attachment `mrt0` is** - its format, size and address.
   That is register state the guest writes, and it cannot be invented (D104).

Only the third needs a capture. In the order worth doing them:

| | Step | Needs a capture | What it buys |
|---|---|---|---|
| **b** | Render pass, a draw with a hand-written fragment shader, and read the attachment back | no | **the oracle**, and it does not exist yet |
| **a** | Solve the export's operand layout by probe | no | decoding which targets a corpus exports to |
| **c** | Emit `Fragment` modules with output variables | no | somewhere for a translated export to go |
| **d** | Which registers configure colour buffer zero | **yes** | the mapping D104 refuses to invent |

**Do (b) first**, before anything it is meant to check. Framebuffer diffing is described
above as the only cheap mechanical oracle this project will ever have, and it is currently
the one part of that sentence that is not built. It is also the only step whose value does
not depend on any of the others, and every later step is verified by it.

It is more building ahead of the spine, with the cost this section opens by stating. The
difference is that a framebuffer harness is checked against **itself** - draw a known
colour, read it back, compare - so it is a genuine oracle rather than a self-consistent
guess, which is exactly what the rest of this phase is short of.

**G9 is the highest-value thing a capture would buy.** The register vocabulary in
`crates/orbistoun-gpu/data/packets.toml` is transcribed and its own comment calls it the
least certain thing in the crate. A raw command buffer is data that still has to be read
*through* that table. A capture of a **builder call paired with the bytes it appended** -
the guest's graphics layer builds buffers through library calls before submitting them -
checks the table itself. That is the same move that found a wrong encoding row in G1, and
it is the difference between more data and an oracle.

**G10 - the resource model** splits in two, and this entry used to blur them.

The *shader-visible* half - what a descriptor contains and how an access computes its
address from one - is documented and is built. The *host* half - how a descriptor's base
address relates to anything the host allocated - is the one that should not be designed
blind: every other unknown here fails loudly, whereas a resource layout guessed wrong
produces a frame that renders and is subtly incorrect, which is the failure this project is
least able to detect.

Only the second half waits.

**G13 - subgroup fidelity** was recorded as blocked on hardware. That was wrong and is
worth being precise about, because it is the second time this table has misfiled the
difference between "cannot be done" and "has not been decided".

What is true: the level's straightforward design maps one guest lane to one host
invocation and needs the host subgroup to be as wide as the guest wavefront - sixty-four.
The GPU here is thirty-two wide, so that design cannot run. Measured, not assumed:
`cargo run --example subgroup-size -p orbistoun-gpu-vulkan`.

What does not follow: that the level is unavailable. A variant carrying **two** guest
lanes per invocation assembles the sixty-four-bit mask from two ballots rather than one,
and runs on thirty-two-wide hardware - which is most hardware, and is what is on this
machine. It would be checkable against the wavefront model by the generated-program
comparison that already exists.

It is a **design decision**, not a hardware limitation. D098 describes the one-lane
design; committing to the two-lane one is a concept the decision log does not contain,
which is why it waits for a person rather than being assumed into existence.

### Not blocked, and available now

**This section said "nothing with real payoff" and was wrong for the third time.** It is
left in below with what it claimed, because the pattern is more useful than the conclusion.

What it missed on 2026-08-21, none of which needed a capture:

- `v_mov_b32_e32` and `v_rcp_f32_e32` had **no operand layout at all**. A move decoded to a
  mnemonic and an empty list. The overlap rule excluded the opcode bits but not the
  family's own fixed bits, so the destination was ambiguous forever (D202).
- The differential suite could not have caught that. It checks every operand it decoded
  against the reference and passes vacuously when it decodes none. There is now a converse
  test asserting the set of instructions decoding nothing is exactly a written-down list.
- MTBUF's opcode was **half read** - three bits of four, the fourth in the second word - so
  every half-precision variant decoded as its counterpart (D105, closed).
- MTBUF's operands and format table, above.

The common thread: each was a question nobody had asked the existing tools, not a piece of
missing information. "Available now" was being read as "is there new material", when the
useful question was "does what we have actually say what we think it says".

The three previous claims failed the same way and this note is the fourth attempt at the
lesson, so it is worth stating flatly: **before recording this section as empty, run the
generators and read what they refuse.** They have refused something every time.

#### What it said

**Nothing with real payoff.** This is the honest state, arrived at twice by claiming it
too early and being wrong, so it is worth being precise about what was checked.

Depth on what already exists has been done: the differential oracle is a generated
property rather than a handful of examples (D136, and it found a wrong opcode number on
its first widened run), and the decoder and pipeline are tested against material that is
not instructions at all. Both were the answer last time this table said "blocked", and
neither is left.

What remains unblocked is marginal - widening a generator that has already found what it
was built to find, or driving the backend seam with a recording stub that consumes
nothing. Neither is worth the code.

*(Resolved, and the diagnosis in this paragraph was wrong - see the entry in
[BACKLOG.md](../BACKLOG.md). It was two tests in `orbistoun-abi` sharing a global array
across parallel threads, not address contention, and `orbistoun-mem` had nothing to do
with it. "Passes in isolation, fails in the workspace run" was read as a signature of
contention when it is equally the signature of any intra-binary race.)*

On instruction breadth specifically: of the twenty-one instructions still refused,
three are the division helpers (blocked on the published instruction set) and sixteen
need the resource model. `exp`, MTBUF, MUBUF, MIMG and VINTRP are all one dependency.

*(Superseded. The division helpers are done, MUBUF is translated, and MTBUF is decoded.
Ten opcodes remain refused across three fixtures: MTBUF's five need translation only, and
`exp`, VINTRP's three and MIMG's one need the graphics pipeline or a capture.)*

What remains that needs nobody:

- ~~The fixed-address test contention~~ - fixed; it was a shared global between two
  tests, not addresses at all.
- **G14**, the deferred performance work, if there is appetite to measure before there is
  anything real to measure on.

Everything else waits on a capture. That is the expected shape - a subsystem built ahead
of its inputs runs out of things it can check itself, and the useful response is to stop
adding to it rather than to keep going on material it generated.

