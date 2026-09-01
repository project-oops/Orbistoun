# User support, and the third thing found modelled and inert


The corpus imports sixteen functions from the user service, and reading them decided the
shape before any code was written: a title asks for a name **by identifier**, enumerates who
is signed in, and keys save data on which user. A single `user_name` field answers none of
those, so the shell now holds a list of users with stable identifiers.

`sceUserServiceGetUserName` is the payoff and the reason it was worth doing first. It is the
one call in that crate answering a console setting **unencoded** - every other one hands back
a number whose meaning is a measurement, while a name is a string somebody typed into the
shell and the guest reads as-is. The morning's argument for settings, with a caller at the
end of it.

### Where the care went

The size argument. That it is third is an assumption, and a wrong argument position writes
past a caller's buffer - the failure `sceUserServiceGetInitialUser` already warns about. It
is believed only up to sixty-four bytes; outside that the call is refused, because a refusal
is recoverable and a smashed stack is not. The test asserts the refusal and that the buffer
is untouched while refusing, not that the success path works.

### Three inert things, and one bug a test found first

`console::configure` had never been called. Written that morning, so every setting a person
chose stopped at the window and the guest read defaults - the third thing this session found
built and connected to nothing, after the pad shim's implementations and `Focus`.

`sceUserServiceGetInitialUser` answered a constant; it now answers whoever is signed in, and
the placeholder when the signed-in account has been deleted.

And the identifier allocator was wrong in a way only the test caught: deriving the next
number from the highest *live* identifier reuses one the moment its holder is deleted, and
save data is keyed on it - so a new person would silently inherit somebody else's saves. A
stored high-water mark now, the same shape as `HandleAllocator`. Written test-first, failed,
fixed. That is the third bug today found by writing the assertion before the code.

### The guard was right and I was wrong

Claiming `found_by = "static"` for the name failed `the_shipped_files_account_for_everything`
- the symbol database re-derives it as `generated`. The naming loop found this name and
confirmed it by hash, which is *stronger* provenance than a static string read, and the guard
cross-checking the claim against the database is what caught the overstatement.

### Automatic recording filed a scratch directory as a title

Listing `compat/` rather than assuming what was in it turned up `New folder (2).toml` - a
tracked record describing a loader payload, `outcome = "0x7ff61e175ee3"` (a host address),
three imports, 34% standing. Auto-recording derives the title from the module's containing
directory, so anything run from a scratch folder gets filed as a compatibility claim.

**Automation turned a latent looseness into pollution.** While a person typed `compat record`,
the directory name was their problem and they would not have typed it; taking the person out
removed the judgement that had been quietly doing the filtering. That is a cost of automating
something, not a bug in the automation, and worth naming as such.

A title id is now required to be an identifier, and a refusal is printed rather than skipped
(D347). Watched refusing a real `New folder (2)`, with no record written.

### The tree, honestly

The gui doc link that blocked the full gate is fixed - the other session landed it. The full
gate then failed anyway on `orbistoun-service`: `DataBlocks::build` has a new signature and
its caller has not caught up, in crates this session has not touched. Scoped gate over the
eight crates it did touch is what stands.

### The scoped gate was not scoping

A scoped run failed on `crates/orbistoun-libc`, a crate the scope excluded. **clippy lints
every workspace crate it compiles from source**, so `-p orbistoun-cli` - a shim depending on
nearly everything - linted most of the tree and attributed another session's finding to the
scoped crates.

That is worse than not scoping: it puts somebody else's failure under your name, and the
previous message here had already said "my crates are clean" while the run was failing on
theirs. `--no-deps` confines it, verified, and is applied only when a scope is given.

**What stays whole-tree is deliberate.** The static gates are facts about the repository, so a
scoped run that skipped them would claim something it had not checked - and can therefore
still be blocked by a file nobody in the scope touched. Written into D319 as a known edge
rather than left to be rediscovered.

The honest description of `--only`: the cargo steps scope, the repository checks do not.

