# 693. Five statements the code beside them contradicted

**2026-09-19** — inbox `-243c`: five places read as true next to code, a decision, or a generated
figure that says otherwise. Two of them (the backend "dispatches compute only" and a swapchain "due")
tell the next session the wrong thing about where the renderer stands.

1. **AGC name provenance** (`crates/orbistoun-gpu/src/agc.rs`): the header claimed *"every name below"*
   is read from a real import table. It is not - the shader-linkage builders (`sceAgcCreateShader`, the
   interpolant/prim-state set) were added because obSCEne's `-e4f1`/`-9a41` requests measured them, not
   because PPSA02664 imports them. Restated as "real import tables, and a few names a measurement
   added", so the provenance claim principle 1 leans on is true.
2. **G9** (`docs/roadmap/015-...:28`): said `vocabulary.rs` walks *"the two `agc-gl-cube-fw1240`
   captures"*. It reads every `.toml` in `tests/captures/`, and there are three since worklog 677
   (`agc-set-cx-register-direct`). Stated as three.
3. **G11** (same file, `:30` and `:92-96`): the heading *"today it dispatches compute only"* and the
   list's *"Every module emitted today is a compute dispatch"* / *"It dispatches compute and nothing
   else"*. `VulkanBackend::execute` handles `Draw` and `DrawIndexed` (D550) and `orbistoun-translate`
   emits fragment modules (D553). The first two of the three steps are marked done, not open.
4. **The swapchain** (`docs/roadmap/014-...:5`, `:17`): named a swapchain as phase-6 work, on the same
   page that quotes D695 - *"No surface, no swapchain, no window"* - which settled it the other way
   (headless render, read back to bytes, uploaded as a texture). Dropped the swapchain from both
   places, pointing at D695.
5. **The ask count** (`docs/HANDOVER-OBSCENE.md:373`): *"744 open questions"* against `README.md`'s
   generated `790`. Updated to 790.

## Gate state

`orbistoun-gpu` builds (the agc.rs change is a doc comment); the contradicted phrases grep empty;
`./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; identity scan clean. No commit.
