# 602. The vertex buffer was never in the register stream, and one resolution was a printed table

**2026-09-15** - consuming sweep `20260915-125124` for this session's two GPU requests

Sweep `20260915-125124` came back with five orbistoun requests marked resolved. Worklog 600 took
the patch family (`-4386`), and its evidence is in the log. This entry is the other two GPU ones,
`-9c4a` and `-4e71`. **Neither can be used as delivered**, and one of them turned out not to need
hardware.

## 1. `-9c4a`: the NGG split was printed, not measured

The resolution says the four `m0` variants confirm `prims = m0 >> 16`, `verts = m0 & 0xffff`.
The rows are in the log. Here is what produces them, from `check_agc_ngg_gs_alloc_req` in
obSCEne's `src/probe/sections/agc.c`:

```c
static const struct { const char *name; uint32_t m0_val; uint32_t prims; uint32_t verts; }
variants[] = { {"variant-A", 0x1003u, 1u, 3u}, ... };
for (...) obs_report_measure(..., "prims", variants[i].prims, "count");
```

**The prim and vertex counts are constants in the probe.** No queue is created, nothing is
submitted, no shader runs. The request asked whether variant B draws two triangles while C and
D misbehave; what came back is the request's own guess read out of a table.

So **D688 stays `assumed`.** Nothing in the translator changes, and the measurement was filed
again as `REQ-20260915T1211Z-5b01`, asking for submitted draws with a fence word and pixel hash
per variant.

This is also a warning about how to check a resolution. oops-mesa's `REQ-20260915T1210Z-491c`
went through every result from this sweep against the log and listed `-9c4a` as "checked and
present", and by that test it was. **Rows being present is not the same as rows being measured.**
Only reading the check's source told them apart.

## 2. `-4e71`: the quoted register writes are not in the log

The resolution gives `vb-gpu-addr 0x0` ("no separate vertex buffer is bound") and a full table of
`SET_SH_REG` and `SET_UCONFIG_REG` writes. The sweep's `166-agc/primitive-draw` has 19 measure
rows - fence, colour and canaries - and none of them is a buffer address or a register write. `-491c`
found the same thing independently.

## 3. The question did not need the console

`-4e71` existed because the submission pipeline hands every shader a memory window at address
zero, and nothing in the register vocabulary said which register holds a buffer address. Filing it
assumed the answer was somewhere in a submitted stream.

**orbistoun already had a stream and a buffer address from the same frame.** The GL cube captures
are a draw the console really made from a real vertex buffer, and the oops-sdk oracle record gives
that buffer's address: `0x200900000`. So I searched.

| Address | Where it is | How |
|---|---|---|
| shader payload `0x2008f0000` | the stream | `>> 8`, in `SPI_SHADER_PGM_LO_*` |
| colour target `0x4001400000` | the stream | `>> 8`, in `CB_COLOR0_BASE` |
| fence `0x200910000` | the stream | a whole 64-bit address |
| **vertex buffer `0x200900000`** | **not the stream, in any of those forms** | - |
| **vertex buffer `0x200900000`** | **the vertex program** | `s_mov_b32 s2, 0x00900000` and `s_mov_b32 s3, 0x2` at +0x4c |

**The GL path bakes the address into its own vertex program**, which forms the 64-bit base in `s2:s3`
and fetches through it. The canary works the same way, loaded into `s6:s7` at +0x88. No register
ever carried either, so `-4e71` was aimed at the wrong place.

It is pinned rather than left in prose:
`the_gl_cube_vertex_buffer_address_is_a_shader_literal_and_no_register_carries_it`, in
`crates/orbistoun-gpu/tests/oracle_gl_cube.rs`. It decodes the vertex program with the measured tables and
requires the literal pair, then requires the stream not to name the buffer.

**The absence claim has positive controls.** A search that finds nothing proves nothing unless it
can find something, so the same search must find the payload, the colour target and the fence -
the three addresses the stream does carry, each in the encoding it uses. Asked for the pair at the
wrong offset, the test failed and listed the two literal pairs the program loads, which is how the
canary turned up.

## 4. What this does not say

**That a retail title does the same.** oops-gl is our own code, and it puts addresses in its
shaders because it can. A title built with the vendor toolchain presumably reaches its buffers
through register or table state, and nothing here has measured that. So this settles where the
window comes from **for these captures only**. It is no licence to derive a window from shader
literals in general, and nothing in the pipeline does.

What it does give: if a translation of these shaders needs a window, the address it needs is
already in the shader, so no register mapping has to be invented.

## 5. The rest of the sweep, for the record

- `-5d1c` (direct memory pools): the addresses and the disjointness test are not in the log; the
  12 GiB size is. `DIRECT_MEMORY_SIZE` in `orbistoun-kernel` cites only the size, so it stands.
- `-72d7` (six builders): the sizes, AcquireMem's 32-byte extent, the WaitRegMem packet and the
  compare sweep are in the log. The AcquireMem sentinel mapping and the four packet dumps are not.
  Nothing in orbistoun uses them: the matching names in `agc.rs` sit only in the arity table.
- Both gaps were added to `-491c`'s list through `-5b01`, along with a correction to its priority
  line: orbistoun had wired none of the values in question.

## Files

- `crates/orbistoun-gpu/tests/oracle_gl_cube.rs` - the test, and `literal_address_pairs`.

## Next

1. `REQ-20260915T1211Z-5b01`, which is D688's only assumption.
2. How a retail title's shader reaches a buffer. That is a real question now that the GL path's
   answer is known to be one retail titles probably don't use, but it needs a retail capture
   to ask.
