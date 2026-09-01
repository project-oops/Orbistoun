# 2026-08-24 - A language-model service, isolated from everything


**Done.** `crates/orbistoun-llm`: measures the machine, sizes a model catalogue to it,
writes an ordered backend registry, downloads a model on first use, and runs it in this
process - or talks to an endpoint, over either of two wire formats. 57 tests, none needing
a network. Reasoning in D212.

**What it unblocks.** [THE_LOOP.md](../THE_LOOP.md)'s steps 17 and 18 - read the top finding,
decide what it means, implement it - are the two a person still does. This is the machinery
for attempting them, not the attempt. Nothing calls it yet, and that is the next piece of
work: an entry point that turns the loop with a model and falls back to a person when it
fails.

**The shape, and why.** No `orbistoun-*` dependency at all. The callers arrive later and
there are several of them with different jobs, so the contract is `Request` in, `Reply` out
and nothing in the crate knows what a trace or an import is. The one thing it would
otherwise want - somewhere to put a multi-gigabyte download - is an argument, because
`orbistoun-paths` guarantees orbistoun never writes outside its own root and a download
landing in an ambient cache would break that with no error anywhere.

### Surprises

**The reference implementations both have the same wrong endpoint.** Two earlier
projects of mine each list `api.anthropic.com/v1/chat/completions` as an OpenAI-compatible provider. That is
not an endpoint that exists - the Messages API takes a top-level `system` field, an
`x-api-key` header and a pinned version header, returns an array of typed content blocks,
and **rejects `temperature` outright rather than ignoring it**. Copying the pattern
faithfully would have produced a provider that 404s, and the 404 reads like a network
problem. `wire` is therefore the protocol rather than the vendor, and a test pins each
difference.

The related trap: **a refusal arrives as a *successful* response with an empty reply.**
A caller checking only the status code sees a healthy request that proposed nothing, which
is indistinguishable from having nothing to propose. Reported as `Error::Refused`.

**candle 0.11 already has `quantized_qwen3`.** An earlier embedded engine of mine carries a long
caveat about reading Qwen3 GGUFs through the Qwen2 loader and calls its own coordinates
unverified - that is a consequence of being pinned to 0.8, not a standing limitation. Worth
knowing before repeating the caveat. Reading one family's weights through another's loader
does not error; it produces fluent output that is wrong, which is why `arch` is a closed
enum refused by name.

**"candle is pure Rust" is half true and I asserted the whole of it.** The inference is -
no C++ toolchain, unlike a llama.cpp binding - but `candle-core` depends on `tokenizers`
with default features, which builds `onig_sys`, a C regex library. That is also what broke
the first build: `tokenizers` compile-errors unless one of `onig` or `fancy-regex` is
picked, and declaring a *different* version than candle's produces two copies in the tree
and builds the C library anyway. Pinning to match is the fix.

**A pipe hid a failed build.** `cargo check ... 2>&1 | tail -60` reports the exit status of
`tail`, so the first build "succeeded" with a compile error in its output. `set -o
pipefail` and `${PIPESTATUS[0]}` from then on. Worth remembering - the whole gate is shell
scripts full of pipes.

**The CPU fallback needed sizing twice.** Both references derive the CPU entry's model from
the accelerator's. A machine with 12 GB of VRAM and 4 GB of free system memory then lists a
CPU fallback that cannot load - and finds out after the download. `seeded_for` sizes it
against a host with the accelerator removed, and a test pins it.

### Also

Fixed a stale line in `CLAUDE.md`: principle 6 still listed `trace` in the dependency spine
after D211 deleted that crate. The spine is six.

`crates/orbistoun-llm` is green on `fmt`, `clippy -D warnings`, and its own tests. The full
`./orbistoun.sh check` has not been run since - it is the next thing.

