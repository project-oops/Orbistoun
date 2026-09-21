# 751. create_shader relocates the full group array under the guard

**2026-09-21** — the first code change on this wall in a while, and an honest one about its reach. The
PPSA02664 investigation (740-750) showed a shader header whose group-pointer array spans five slots
(`+0x18..+0x38`) but whose relocation covered only the middle three, leaving `+0x18`'s raw `0xa8` as the
pointer the workload walk dies reading. orbistoun's `sceAgcCreateShader` had the same three-of-five
list. This tick makes `create_shader` relocate the **whole** array, guarded — a real accuracy fix for
the API, correctly *not* claimed as the fix for PPSA02664, which does not call it.

## The change

`create_shader`'s relocation loop scanned `&[0x20, 0x28, 0x30]`. It now scans
`&[0x18, 0x20, 0x28, 0x30, 0x38]` — the five group-pointer slots the guest walks with stride 8
(worklog 748) — under the same guard it always had, `rel != 0 && rel < 0x1000`. That guard is what makes
the widening safe rather than a guess:

- A slot holding a relative offset (small, non-zero) **must** be relocated to become a valid pointer;
  left raw it is a bad pointer like `0xa8`. Relocating it is a necessity, not a value invented.
- A slot that is zero or already absolute is skipped. So the measured shader (obSCEne
  `166-agc/create-shader`, whose endpoints were zero) relocates exactly the three it always did, and its
  test still passes unchanged.

The measured behaviour is preserved by the guard, not by the field list; the field list only decides
*which slots are candidates*, and the array is five slots wide, measured on three of them and generalised
to the two the measurement's shader left empty.

## Held both directions

A new test populates the whole array (`+0x18=0xa8 … +0x38=0x58`, PPSA02664's shape) and asserts all five
become `header + offset`, including the two endpoints the old list skipped; then it runs a header with
empty endpoints and asserts `+0x18`/`+0x38` stay zero rather than becoming `header + 0`. The existing
`create_shader_fills_the_object_as_hardware_did` — the one verified against retail Unity pipelines —
passes untouched, which is the proof the widening did not disturb the measured case.

## What this does and does not do

It hardens the `sceAgcCreateShader` API: a title that calls it with a multi-group shader header now gets
every group pointer relocated, not three of five, and will not wall on an un-relocated `+0x18`/`+0x38`.
It does **not** clear PPSA02664's wall, because that title's Unity backend inlines the shader-object
construction and never calls `sceAgcCreateShader` (the census is empty of shader calls, 750). PPSA02664
still faults; its inlined loader's short relocation is a separate, still-open question (the input that
loader misreads, behind the tooling barriers 749/750 named). This change is the accurate-emulation
correction the evidence justifies on the code orbistoun actually owns — the SDK entry point — and it
leaves a correct `create_shader` for the next title that reaches it.

## Gate state

Changed `crates/orbistoun-gpu/src/agc.rs` (the relocation loop's slot list and its comment, with a
test). `cargo fmt --check`, `cargo clippy --all-targets`, and the crate's `create_shader` tests are
clean (3/3); the workspace gate is run below. `./bin/orbistoun check` otherwise unchanged from 750 —
still the same three generated-doc drifts from a prior session's uncommitted `compat/PPSA02664-app0.toml`
edit, not this change. Identity scan clean. No commit.
