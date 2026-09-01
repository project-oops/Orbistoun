# D293 - The dispatcher and the naming loop become two crates


**decided** · 2026-08-26 · so a shim can run a turn without a model runtime in the binary

`orbistoun-propose` is two things. One proposes **words** for the hash oracle and needs a
model: `suggest`, `vocabulary`, `bank`, and the `orbistoun-suggest` binary. The other turns
the **loop**: `turn`, `experiment`, `axis`, `trial`, which are rules and boots.

They share nothing. Measured rather than assumed - each cluster references only its own
modules, and the four dispatcher files reference `orbistoun_llm` **zero times**.

The reason to act on it is concrete. `turn::turn` runs a whole turn unattended and no command
calls it, so the loop is reachable only through an opt-in test. Putting it behind
`orbistoun-cli` means the CLI depends on `orbistoun-propose`, which drags candle, reqwest and
tokenizers into a binary that has no use for them.

A feature flag would hide it. **A crate boundary makes it impossible**, which is the argument
this project already made for `orbistoun-gpu` having no dependency on `ash`: *"host-API
leakage is impossible rather than discouraged, and `cargo` polices it instead of code
review."* Same shape, so the same answer - `orbistoun-turn` for the dispatcher, and
`orbistoun-propose` keeps the name it earned, since "propose" was always about proposing
names.

The seam test in principle 12 is whether it pays now or only hypothetically: it buys a shim
that can run the loop without a GPU runtime linked into it, today.

`Error` splits with them. The dispatcher uses exactly one variant of the five - `Reply`, for a
run that could not be made - and the other four are about models and grammars. A shared error
type across this boundary would have been the coupling the split is for.

