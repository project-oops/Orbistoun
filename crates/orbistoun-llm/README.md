# orbistoun-llm

Local-first language-model access, as a generic question-and-answer service.

**Models:** what this machine can run, which backends are configured and in what
order, and the wire formats they speak.

**Deliberately fakes:** nothing. Every failure names what went wrong; no reply is ever
invented to avoid an error.

## The isolation is the design

This crate has **no `orbistoun-*` dependency.** That is not a staging accident. There
will be several callers with different jobs - proposing a stub semantic, proposing
vocabulary for a name search, ranking open questions, summarising a run - and a service
shaped around whichever one came first is a service the rest fight.
[orbistoun-propose](../orbistoun-propose/README.md) is the first, and it depends on this
crate rather than the other way round.

So the contract is `Request` in, `Reply` out. Nothing here knows what a trace, a
finding, an import or a title is.

The one thing it would otherwise want is `orbistoun-paths`, for somewhere to put a
multi-gigabyte download. That is an **argument** instead. The reason matters:
`orbistoun-paths` guarantees that orbistoun never writes outside its own resolved root,
and several gigabytes landing in an ambient user cache would break that guarantee with
no error anywhere. The crate holding the guarantee is the crate that decides where
bytes land.

## What it does with no configuration at all

```text
Host::probe()          what is this machine - cores, RAM, accelerator
     ↓
select::recommend()    a model sized for it, and why
     ↓
Config::seeded_for()   an ordered registry, written once, then owned by a person
     ↓
Llm::ask()             first compatible + configured entry that answers
     ↓
ManagedEngine          fetches a GPU runtime and a model, starts both, talks to them
```

The ladder, in the order it is tried:

| entry | what it is | reaches a GPU |
|---|---|---|
| `managed` | a `llama-server` this process downloads and supervises | **yes, any vendor** |
| `ollama` | a model server already running on this machine | yes, if you installed one |
| `local-cpu` | in-process, on the processor | no |
| hosted | Claude, ChatGPT, Gemini | not yours |

Nothing hosted outranks anything local.

No server, no daemon, no key, no install. Every step is overridable by editing one
file, because none of it is a decision anybody should have to accept.

```rust,no_run
use orbistoun_llm::{Llm, Request};

let llm = Llm::open("./.orbistoun/ai")?;
let reply = llm.ask(&Request::new("Name one thing.").with_max_tokens(64))?;
println!("{} said: {}", reply.model, reply.text);
# Ok::<(), orbistoun_llm::Error>(())
```

## Three properties, and why each is load-bearing

**Local outranks hosted.** A trace, a fault address and a guest's own strings are this
project's material. The seeded ladder puts this machine first and reaches a hosted
provider only when told to.

**Deterministic by default** - temperature zero, fixed seed - so an identical request is
an identical answer and a result can be attributed to a change rather than to the
weather.

That is right for anything whose output is *believed*, and **wrong for a proposer**,
which is why the seed is a parameter rather than a constant. A proposer's output is
checked by an oracle, so a suggestion is worth nothing until arithmetic agrees, and what
it needs from a second round is a *different question*. Greedy decoding does not merely
repeat between rounds - it repeats within one, and the first measured round returned
twenty suggestions of which fourteen were the same word (D219).

**Attributable.** A `Reply` carries which entry answered, which model, and everything
tried before it. D046 makes a run report embed its own inputs so a difference between
runs can be blamed on the change rather than on drift - a model is such an input, and
one that silently fell back from a 4B to a 0.6B has drifted.

## The catalogue is data

`data/models.toml` holds every model and endpoint, their sizing, and which are eligible
to be chosen automatically. Adding a model, retiring one, or changing what a machine
gets is an edit to that file (principle 5).

One field is not free-form: `arch` names a loader that must exist, and an unknown value
is refused by name. The failure that prevents is silent - a Qwen3 GGUF read through a
Qwen2 loader does not error, it produces fluent output that is wrong.

`wire` is likewise the protocol, not the vendor. Three of the four shipped endpoints
speak the OpenAI-shaped request and one does not. **Two sibling projects both list
`api.anthropic.com/v1/chat/completions`**, which is not an endpoint that exists; the
Messages API takes a top-level `system` field, an `x-api-key` header, a pinned
`anthropic-version`, returns an array of content blocks, and rejects `temperature`
outright rather than ignoring it. A test pins each of those.

## What is checked, and where

| check | needs a network | catches |
|---|---|---|
| `catalog` tests | no | a malformed shipped catalogue, a duplicate id, sizing that is not monotonic, an arch with no loader |
| `select` tests | no | a hand-pick model chosen automatically, a CPU sized against VRAM, an unmeasured machine not saying so |
| `config` tests | no | a GPU entry on a machine with none, a hosted entry hidden rather than shown as waiting, a repair that rewrites on every load, a key written to disk |
| `online` tests | no | both wire formats, in both directions, including a refusal reported as a refusal and an error body quoted into a log |
| `embedded` tests | no | construction touching disk, a zero-length file counting as a download, a chat template that never hands over the turn |
| `runtime` tests | no | the release pin, the device listing parsed from captured output, a CRLF becoming part of a device id, an archive entry escaping its directory |
| `embedded_model_loads_and_answers` | **yes** | the whole in-process path - download, GGUF parse, loader, template, stop token |
| `gpu_runtime_starts_and_answers` | **yes** | the whole GPU path - runtime download, unpack, device enumeration, start, answer |

That last one is `#[ignore]`d and downloads a model:

```bash
cargo test -p orbistoun-llm --release -- --ignored embedded_model
```

## Known limitations, written down rather than left to be discovered

- **`host.rs` still probes with `nvidia-smi`, so it sees one vendor.** No longer the
  limitation it was: `runtime::Runtime::devices` asks the inference runtime what it can
  address, which is vendor-neutral and answers the better question anyway - *can this
  backend use that device*, rather than *is a device present*. What remains is that the
  two probes disagree on an AMD or Intel machine, and only the second one is right.
- **`host.rs` is written to be lifted.** It has no knowledge of models and no
  dependency on the rest of this crate. D046 requires a run report to embed its own
  inputs so a difference between runs is attributable to the change; *the machine* is
  such an input, and nothing records it today. When there is a shared home for that,
  the file moves whole.
- **No streaming.** Every reply asked for here is bounded and small, and the caller
  reads the whole answer before acting on it.
- **The in-process engine is processor-only, by design.** A `cuda` feature used to sit
  behind it and was removed: it wanted a vendor toolkit at build time, produced a binary
  that would not load elsewhere, and covered one vendor. The managed runtime does the
  same job from a binary somebody was handed (D219).
- **The inference is pure Rust; the tree is not.** candle needs no C++ toolchain,
  unlike a llama.cpp binding - but `candle-core` depends on `tokenizers` with default
  features, which builds a C regex library (`onig_sys`). This crate adds roughly two
  hundred dependencies, by a wide margin the largest single addition this workspace has
  taken, and `deny.toml`'s hand-curated licence allow-list had to grow to accept them.

## GPU without anybody installing anything

The requirement was narrow: run on the GPU, with no action from the user, in a portable
install. Compiling an accelerator in fails it - that needs a vendor toolkit at build time
and produces a binary that will not load elsewhere. Asking for Ollama fails it outright.

So the runtime is **fetched like a model is fetched**: a 34 MB prebuilt `llama-server`,
started as a child process, spoken to over the OpenAI wire this crate already had, and
stored under the same supplied root so portable stays portable.

**Vulkan rather than CUDA** - 34 MB against 640 MB, every vendor rather than one, and
orbistoun is a Vulkan project, so a machine that can run it has a working Vulkan driver
by definition. That is the one accelerator interface this project may assume.

The device is **named** on the command line, from `--list-devices`, rather than inferred:
an unusable device makes the runtime refuse to start, so a running one is proof rather
than a hope. Falling back to the processor is otherwise silent and successful, which
means "it answered" is not evidence of anything.

## Status

Verified end to end on real hardware: download, unpack, enumerate, start on `Vulkan0`,
answer - seventeen seconds. tests, all but two runnable with no network.

`orbistoun-propose` is the first caller. Closing [THE_LOOP.md](../../docs/THE_LOOP.md)'s
steps 17 and 18 is the work in progress, and `docs/BACKLOG.md`'s *Automated
stub-semantics search* specifies the next proposer after that.
