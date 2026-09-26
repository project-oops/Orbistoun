# orbistoun-llm

Local-first language-model access, as a generic question-and-answer service.

It models what this machine can run, which backends are configured and in what order, and
the wire formats they speak. Every failure names what went wrong; no reply is ever invented
to avoid an error.

## Isolation

The crate depends on no other orbistoun crate except `orbistoun-env`, for the names of the
variables it reads. Its callers have different jobs - proposing vocabulary for a name search,
proposing a stub semantic, ranking open questions, summarising a run - and a service shaped
around one of them is a service the rest fight. [orbistoun-propose](../orbistoun-propose/README.md)
depends on this crate, never the other way round.

The contract is `Request` in, `Reply` out. Nothing here knows what a trace, a finding, an
import or a title is.

The storage root is an argument, not a lookup through `orbistoun-paths`. `orbistoun-paths`
guarantees that orbistoun never writes outside its resolved root, and several gigabytes
landing in an ambient user cache would break that guarantee with no error. The caller that
holds the guarantee decides where bytes land.

## Resolution with no configuration

```text
Host::probe()          what is this machine - cores, RAM, accelerator
     |
select::recommend()    a model sized for it, and why
     |
Config::seeded_for()   an ordered registry, written once, then owned by a person
     |
Llm::ask()             first compatible + configured entry that answers
     |
ManagedEngine          fetches a GPU runtime and a model, starts both, talks to them
```

The ladder, in the order it is tried:

| Entry | What it is | Reaches a GPU |
|---|---|---|
| `managed` | a `llama-server` this process downloads and supervises | yes, any vendor |
| `ollama` | a model server already running on this machine | yes, if one is installed |
| `local-cpu` | in-process, on the processor | no |
| hosted | a hosted provider's API | not this machine's |

No hosted entry outranks a local one. No server, daemon, key or install is required, and
every step is overridable by editing one file.

```rust,no_run
use orbistoun_llm::{Llm, Request};

let llm = Llm::open("./.orbistoun/ai")?;
let reply = llm.ask(&Request::new("Name one thing.").with_max_tokens(64))?;
println!("{} said: {}", reply.model, reply.text);
# Ok::<(), orbistoun_llm::Error>(())
```

## Properties

- **Local outranks hosted.** A trace, a fault address and a guest's own strings are project
  material. The seeded ladder puts this machine first and reaches a hosted provider only
  when told to.
- **Deterministic by default** - temperature zero, fixed seed - so an identical request gives
  an identical answer and a result is attributable to a change. The seed is a parameter
  because a proposer needs the opposite: its output is checked by an oracle, and greedy
  decoding repeats itself within a round as well as between rounds.
- **Attributable.** A `Reply` carries which entry answered, which model, and everything tried
  before it. A run report embeds its inputs so a difference between runs is blamed on the
  change rather than drift, and a model that silently fell back to a smaller one has
  drifted.

## The catalogue

`data/models.toml` holds every model and endpoint, their sizing, and which are eligible to be
chosen automatically. Adding, retiring or re-sizing a model is an edit to that file.

`arch` names a loader that must exist; an unknown value is refused by name. The failure it
prevents is silent: a model read through the wrong architecture's loader produces fluent,
wrong output rather than an error.

`wire` is the protocol, not the provider: `openai` for the OpenAI-shaped chat-completions
request, `anthropic` for the Messages API, which takes a top-level `system` field, an
`x-api-key` header and a pinned `anthropic-version`, returns an array of content blocks, and
rejects `temperature` rather than ignoring it. A test pins each of those.

## GPU runtime

The managed runtime is fetched the way a model is fetched: a prebuilt Vulkan `llama-server`,
started as a child process, spoken to over the OpenAI-shaped wire, and stored under the
supplied root so a portable install stays portable. Compiling an accelerator in would need a
vendor toolkit at build time and produce a binary that does not load elsewhere; Vulkan covers
every GPU vendor, and a machine that runs orbistoun has a working Vulkan driver.

The device is named on the command line from `--list-devices`, never inferred. An unusable
device makes the runtime refuse to start, so a running runtime is proof of the device; a
silent fallback to the processor would make "it answered" evidence of nothing.

The in-process engine is processor-only. It is pure Rust (candle), though `tokenizers` with
default features builds a C regex library (`onig_sys`), and the crate's dependency tree is
what `deny.toml`'s licence allow-list admits.

Replies are not streamed: every reply asked for is bounded and small, and the caller reads
the whole answer before acting on it.

## Checks

| Check | Needs a network | Catches |
|---|---|---|
| `catalog` tests | no | a malformed shipped catalogue, a duplicate id, sizing that is not monotonic, an arch with no loader |
| `select` tests | no | a hand-pick model chosen automatically, a CPU sized against VRAM, an unmeasured machine not saying so |
| `config` tests | no | a GPU entry on a machine with none, a hosted entry hidden rather than shown as waiting, a repair that rewrites on every load, a key written to disk |
| `online` tests | no | both wire formats, in both directions, including a refusal reported as a refusal and an error body quoted into a log |
| `embedded` tests | no | construction touching disk, a zero-length file counting as a download, a chat template that never hands over the turn |
| `runtime` tests | no | the release pin, the device listing parsed from captured output, a CRLF becoming part of a device id, an archive entry escaping its directory |
| `embedded_model_loads_and_answers` | yes | the whole in-process path - download, GGUF parse, loader, template, stop token |
| `gpu_runtime_starts_and_answers` | yes | the whole GPU path - runtime download, unpack, device enumeration, start, answer |

The network tests are `#[ignore]`d and download a model:

```bash
cargo test -p orbistoun-llm --release -- --ignored embedded_model
```
