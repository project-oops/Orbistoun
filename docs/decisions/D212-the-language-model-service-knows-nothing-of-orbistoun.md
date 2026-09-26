# D212 - The language-model service knows nothing of orbistoun

**Status:** decided
**Date:** 2026-08-24

`orbistoun-llm` depends on no other orbistoun crate: request in, reply out, with a model
catalogue in data and an ordered backend registry. It takes its storage root as an argument,
downloads a model on first use of that model only, prefers local backends to hosted ones, runs
synchronously, and answers deterministically by default. A reply names the backend and model
that answered and everything tried before it.

**Why:** its callers have different jobs, and a service shaped around the first fights the rest.
Taking the root keeps the guarantee that orbistoun writes only beneath its own root. A trace and a
guest's strings are this project's material, so the default does not post them elsewhere. A
model that silently fell back is a changed input.

**Rejected:**
- A task-shaped interface: fits one caller.
- An ambient model cache: writes outside the resolved root.
- An async runtime: the largest dependency in the tree for sequential work.
