# 825. Each draw carries its own user data — the cube's twelve per-draw vertex positions in its buffer and its texture table reach the command list; the backend refuses them by name until a shader reads them

**2026-09-24** — worklog 824's first live frame showed one face where the cube draws twelve. The reason
is structural: a translated shader's scalar register file starts as all zeros (`CONSTANT_NULL`,
`wavefront.rs`), and nothing loads the **user data** the stream sets before each draw — the values the
hardware puts in a shader's first scalar registers before its first instruction. The cube passes each
draw's byte position in its vertex buffer that way (`SPI_SHADER_USER_DATA_GS_0`, landing in `s8`), so
all twelve draws read position 0 and drew the first triangle twelve times; its pixel shader receives the
address of its texture descriptors the same way (`SPI_SHADER_USER_DATA_PS_0/1`, in `s0:s1`), which
Neverball's textured draws will need.

Feeding user data to a shader is three pieces — the pipeline capturing it per draw, the translator
preloading it at entry, the backend supplying it per draw. This is the first.

## What was built

- **`RenderCommand::SetUserData { stage, words }`** (`orbistoun-gpu` `backend.rs`): a stage's 32
  user-data words (`USER_DATA_WORDS`, `SPI_SHADER_USER_DATA_*_0..31`), for the draws that follow.
- **The pipeline emits it per draw** (`push_geometry_commands`, now given the stream's register
  writes): before each draw, each stage's words as they stand — every register's last write in an
  earlier packet — emitted only when they changed. `USER_DATA_REGISTERS` names the two sets: the
  fragment stage's `0x2C0C` (measured: the cube capture put the descriptor-table pointer there,
  `data/packets.toml`) and the vertex stage's `0x2C8C` (`SPI_SHADER_USER_DATA_GS_0`, byte `45616` in
  Mesa's `gfx103.json`, where the SDK writes its per-draw vertex position) — the `GS` set because the
  vertex program runs as the NGG geometry stage on this generation (D688).
- **The Vulkan backend refuses it by name** — *"SetUserData (no translated shader reads user data
  yet)"* — rather than accepting state no draw applies. The two console-triangle tests that asserted
  nothing is refused now assert that nothing *but* `SetUserData` is, with a note that they return to
  zero when the backend feeds user data; any other refusal still fails them.
- **Test `each_of_the_cubes_twelve_draws_carries_its_own_vertex_offset`**: oracle record B's stream
  submitted, the vertex stage's word 0 read before each draw — twelve draws, twelve distinct values.

## What the cube's submission now says

```
SetUserData { stage: Vertex,   words: [0, ...] }
SetUserData { stage: Fragment, words: [0x14A0000, 0x7400, ...] }    -> s0:s1 = 0x7400_014A_0000
Draw { vertices: 3, ... }
SetUserData { stage: Vertex,   words: [144, ...] }
Draw { vertices: 3, ... }
SetUserData { stage: Vertex,   words: [288, ...] }
...
16 command(s) driven, 13 refused
  refused    13  SetUserData (no translated shader reads user data yet)
```

The vertex positions step by 144 bytes — three vertices of 48 — exactly the stride the cube's vertex
program reads with, and the pixel stage gets its descriptor table in the GL payload. The frame is
unchanged (one face): the words are carried and not yet read. Next: the translator preloads a module's
scalar registers from its stage's user data at entry, and the backend supplies each draw's words (push
constants), at which point the refusals go and the twelve faces should appear.

## Gate state

`crates/orbistoun-gpu/src/backend.rs` (`SetUserData`, `USER_DATA_WORDS`), `lib.rs` (the export),
`pipeline.rs` (`USER_DATA_REGISTERS`, `user_data_at`, `push_geometry_commands`, the test),
`crates/orbistoun-gpu-vulkan/src/lib.rs` (the named refusal), `tests/console_triangle.rs` (the two
assertions). All 32 test binaries of `orbistoun-gpu`, `orbistoun-gpu-vulkan` and `orbistoun-worker` pass.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
