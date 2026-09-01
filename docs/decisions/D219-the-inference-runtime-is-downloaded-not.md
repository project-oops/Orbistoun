# D219 - The inference runtime is downloaded, not compiled in; and a proposer samples


**decided** · 2026-08-24 · requirements given directly: it must run on the GPU, with no
action from the user, in a portable install, downloading whatever it needs

Two decisions that arrived together because the second was only measurable once the first
was true.

### Why the accelerator cannot be a build feature

The first attempt followed a sibling project: `candle`, with an accelerator behind a
cargo feature, off by default, shipped "as a separate artifact". Three requirements kill
that, and the third kills it twice.

- **A vendor toolkit at build time.** `candle-core/cuda` compiles its own kernels, so a
  machine without `nvcc` cannot produce a build that uses its own hardware. This one has
  a 16 GB card, a current driver, and no toolkit; `--features cuda` will not build here.
- **A binary that will not load elsewhere.** The artifact links a runtime, so it is not a
  build anybody can be handed.
- **NVIDIA only.**

Checking the siblings rather than assuming settled it: **neither runs inference on a GPU
either.** One keeps its `cuda` feature off by default, with the same comment this
project had copied; the other references only `LLamaSharp.Backend.Cpu`. They are fine on the
processor because they ask a 0.6B model a one-sentence question. This project asked a 4B
model for forty words and waited four minutes.

### What replaced it

**A prebuilt `llama-server`, fetched the way a model is fetched**, started as a child
process, and spoken to over the OpenAI-shaped wire this crate already had. "Download
whatever it needs" is exactly the permission that makes this legal, and it satisfies
every requirement at once: the GPU is used, nothing is installed, and the runtime lands
under the same supplied root as the models, so portable stays portable.

**Vulkan rather than CUDA**, and size is the smaller reason:

| backend | download | vendors |
|---|---|---|
| Vulkan | **34 MB** | NVIDIA, AMD, Intel |
| CUDA | 250 MB plus a 391 MB redistributable | NVIDIA only, matched per toolkit version |

**orbistoun translates guest command streams to Vulkan.** A machine that can run this
project at all has a working Vulkan driver by definition, so it is the one accelerator
interface this project may assume - and assuming it costs nothing not already assumed.
The archive also carries processor backends chosen at load time, so a machine with no
usable device still runs, without a second download and without a build feature.

Verified end to end: download, unpack, enumerate, start on `Vulkan0`, answer - seventeen
seconds.

### The device is named, not inferred

`--device Vulkan0`, from `--list-devices`, rather than `-ngl` alone.

**This is the difference between a claim and a check.** Falling back to the processor is
silent and successful, so "it answered" passes identically either way. Naming the device
makes an unusable one a *refusal to start*, which means a live runtime holding a device is
one the runtime itself accepted. There is nothing left to infer.

The route not taken was parsing the log, and it is worth recording why: the device
selection *is* logged, but only at a verbosity that also prints a line per layer of the
model, and the format is a debug stream rather than an interface. `--list-devices` is
user-facing output with a stable shape. **The first version parsed the log anyway,
against a format recalled rather than captured, and reported "no accelerator" on a
machine that was demonstrably using one.** It also discarded stdout, which is where the
enumeration goes. Both mistakes said *processor* about a working GPU.

A side benefit: asking the runtime is vendor-neutral, which retires the "only NVIDIA
reports memory" limitation the `nvidia-smi` probe was written down as having.

### A processor is capped by what it can work through

Sizing had been "the largest model that fits in memory", and thirty-two gigabytes fits a
four-billion-parameter model and then works through it at about one token per second.
**Fitting and being usable are different questions and only the first is about memory.**
Memory is now a floor and the core count a ceiling, which is the shape the sibling
project had and this one had dropped.

### And a proposer samples

`orbistoun-llm` defaults to temperature zero, for a good reason that does not reach the
proposer in `orbistoun-propose`: determinism is right where a result must be attributable
to a change, but a proposer's oracle is a hash, so a proposal is worth nothing until
arithmetic agrees and **what a round needs is variety**.

Greedy decoding did not merely repeat between rounds, it repeated *within* one: the first
live round returned twenty suggestions of which fourteen were the same word, and all six
survivors shared a prefix. `Request` gained a `seed` - fixed by default, so an identical
request is still an identical answer - and the proposer varies it. Without that, a second
round is not a second question.

Two prompt findings from the same run, both measured rather than reasoned:

- **Showing whole identifiers and asking for "words" produces compounds.** The model
  returned `Schedparam` and `Schedpolicy` because that is what the examples looked like.
  The prompt now shows the seam - `sceKernelAllocateDirectMemory = sce + Kernel +
  Allocate + Direct + Memory` - and no prose has to describe the shape.
- **Asking per position beats asking in general.** The one word that worked, `Async`, was
  a *suffix*, and the suffix list is the shortest in the grammar. A model told which
  position it is filling answers a much narrower question.

### What this cost, and what it bought

Six rounds asking for vocabulary at large, on the processor: **two names**, in
thirty-seven minutes. Nine rounds asked per position: **six names**, all real
(`sceAgcDcbAcquireMem`, `sceAudio3dObjectReserve`, `sceSaveDataLoadIcon` among them), each
verified against a per-slot control so the existing vocabulary gets no credit.

### The bug the wiring exposed

`Llm::engine` rebuilt its engine on every ask, so the in-process engine re-read gigabytes
of weights **per question**. Engines are now cached by entry id. Most of a four-minute
round was not inference.

### The `cuda` feature is now dead weight

Kept for the moment, and worth removing: the managed runtime beats it on every axis -
more vendors, no toolkit, smaller download, and it works from a binary somebody was
handed. Nothing should reach for it.

