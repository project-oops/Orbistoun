# D212 - A language-model service, in a crate that knows nothing about orbistoun


**decided** · 2026-08-24 · directed by the user, who answered the five open questions
below rather than leaving them assumed

`crates/orbistoun-llm`. Host sizing, a model catalogue, an ordered backend registry, an
in-process engine that downloads what it needs, and two HTTP wire formats. **No
`orbistoun-*` dependency, and nothing depends on it yet.**

### Why it exists

[THE_LOOP.md](../THE_LOOP.md) marks two of its nineteen steps as a person's: read the top
finding and decide what it means, then implement it. Everything else runs unattended. This
crate is the machinery that lets something else attempt those two - not the attempt itself,
which is separate work with a separate oracle problem.

The ground was already prepared, which is worth naming because it makes this plumbing
rather than a new idea. `orbistoun-report` says of itself "logs are for humans; **this is
what an agent reads**"; `diagnose.rs` says its eventual consumer is "a human with a language
model, and later... the emulator repairing its own gaps"; `--json` exists "for a probe or an
agent to consume"; retention is sized against "an agent doing hundreds of runs". Findings
are already `Gap` + `Confidence` + evidence + action, as data. What was missing was the
thing that reads them.

### The isolation is the design, not a staging step

The callers arrive later and there will be several with different jobs - proposing a stub
semantic, proposing vocabulary for a name search, ranking open questions, drafting a `learn`
entry, summarising a run. A service shaped around whichever came first is a service the rest
fight.

A sibling project's equivalent trait takes `(item, allowed_tags, required, hints,
background)` and returns tag suggestions. That works there because there is exactly one job.
Copying the shape here would put naming, diagnosis and stub semantics through one signature
and make each slightly wrong. So the contract is `Request` in, `Reply` out, and nothing in
the crate knows what a trace or an import is.

**The path consequence is the interesting one.** The single thing this crate would otherwise
want from the workspace is `orbistoun-paths`, for somewhere to put a multi-gigabyte
download. It takes a root as an *argument* instead - which is not merely isolation hygiene:
`orbistoun-paths` guarantees orbistoun never writes outside its own resolved root, and
several gigabytes landing in an ambient user cache would break that guarantee with no error
anywhere. The crate that holds the guarantee is the crate that decides where bytes land.
Both sibling implementations write to an ambient cache; neither has that guarantee to break.

### Five questions, answered rather than assumed

1. **Engine: candle.** The inference is pure Rust, so the offline path needs no C++
   toolchain and `cargo build` keeps working on a machine with nothing installed - which
   matters more here than raw speed, given how much of this repository's tooling already
   needs a VM to get a toolchain. **Pinned to 0.11 or later for a specific reason**: it is
   the first release carrying `quantized_qwen3`. A sibling pinned to 0.8 reads Qwen3 weights
   through the Qwen2 loader and records its own coordinates as unverified - reading one
   model family's weights with another's loader does not fail, it produces fluent output
   that is wrong. That is why `arch` is a closed enum refused by name rather than a string.
2. **Synchronous.** This workspace has no async runtime. Inference is CPU-bound work a
   runtime cannot speed up, and an HTTP call here is one round trip rather than a fan-out.
   Importing tokio to await two things in sequence would be the largest dependency in the
   tree paying no rent.
3. **Download on first use of a model** - not at startup, not on construction, not from
   `check`. Opening the service writes one config file and touches nothing else, and a test
   asserts exactly that. Several gigabytes arriving because somebody ran a lint is the
   failure being prevented.
4. **A `run-llm` entry point**, enabling the model-driven workflow and falling back to a
   person when it fails. Not built here; this crate is its foundation.
5. **Named `llm`, not `ai`.**

### Choices this crate makes that its two references do not

**The catalogue is data.** `data/models.toml`, not a `const` array. Principle 5's test is
"if answering the question requires a rebuild, it is in the wrong place", and "which model
should this machine run" is exactly that. The split matches `encodings.toml` and
`table::classify`: the data says what, the code says how, and an `arch` with no loader
behind it is refused rather than approximated.

**Local outranks hosted in the seeded ladder.** Not a performance claim. A trace, a fault
address and a guest's own strings are this project's material, and the default must not be
to post them to somebody else.

**Deterministic by default** - temperature zero, fixed seed. This project's only measure of
progress is running a title, changing one thing, and running it again. A proposer that
answers differently each time makes that measurement meaningless, so sampling is a decision
rather than a default.

**A reply is attributable.** It carries which entry answered, which model, and everything
tried before it, plus `fell_back()`. The same argument as D046 making a run report embed its
own inputs: a model is an input, and one that silently dropped from a 4B to a 0.6B has
drifted - precisely the confusion D046 exists to prevent.

**The CPU entry is sized separately from the accelerator entry.** Both references derive one
from the other. A machine with 12 GB of VRAM and 4 GB of free system memory would otherwise
list a CPU fallback that cannot load - and would find out after the download.

**The Messages API is not treated as OpenAI-shaped.** Both references list
`api.anthropic.com/v1/chat/completions`, which is not an endpoint that exists. It takes a
top-level `system` field, an `x-api-key` header and a pinned `anthropic-version`, returns an
array of typed content blocks, and **rejects `temperature` outright rather than ignoring
it** - so this crate does not send one, and says so in `describe()` rather than dropping a
caller's parameter silently. A refusal arrives as a *successful* response with an empty
reply, so it is reported as a refusal: "proposed nothing" and "was not allowed to propose"
are different facts, and only one is worth retrying.

### What is deliberately not decided here

**Whether anything that comes back through this crate may be recorded as knowledge.**
Principle 1 names a model in the loop as a third route to contaminated provenance - a thing
that has read the public internet can *recall* an answer and present it as reasoning - and
this crate does not address that. It moves bytes. The accounting mechanism already exists
(`known_by`, and the deliberate absence of a value meaning "I already knew it"), and how a
proposal enters it belongs to the caller.

One case is worth flagging now because it touches the *naming* provenance rather than the
knowledge vocabulary: if a model proposes a whole symbol name and the hash confirms it,
`audit` cannot re-derive that name, which dents PROVENANCE.md's "re-derivable from this
repository alone". If the model instead proposes **words** for `vendor.toml`, the name that
lands is `generated` at a real index and the audit is untouched. Words-not-names needs no
new provenance category at all. Not settled here; recorded so that it stays a decision
rather than becoming a discovery.

### Known limitations, written down rather than left to be found

Only NVIDIA accelerators report memory, via `nvidia-smi`; AMD and Intel report nothing, so
those machines get the catalogue default. `orbistoun-gpu-vulkan` already loads Vulkan at
runtime and could enumerate device-local heaps on any vendor, which is the better answer and
was left out to avoid coupling this crate to `ash` for one number.

`host.rs` is written to be **lifted whole**. It has no knowledge of models and no dependency
on the rest of the crate, because D046's requirement that a report embed its own inputs
applies to the machine as much as to the config - two runs compared across two machines are
not comparable today, and nothing says so.

The dependency tree grew by roughly two hundred crates, by far the largest single addition
this workspace has taken - and **exactly one of them was outside the licence allow-list**,
which is a better result than expected. `webpki-roots` ships the Mozilla CA bundle under
CDLA-Permissive-2.0, a *data* licence rather than a software one. Handled the same way the
font crate was: a scoped exception with its reasoning, not a blanket allowance, because the
argument is about a certificate bundle and would not transfer to a code dependency arriving
under the same identifier (D208).

The inference stack is also not free of native code: `candle-core` depends on `tokenizers`
with default features, which builds a C regex library. No C++ toolchain, which was the
claim; not no toolchain at all.

