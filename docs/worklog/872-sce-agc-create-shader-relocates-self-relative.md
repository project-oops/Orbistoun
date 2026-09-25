# 872. sceAgcCreateShader relocates every offset self-relative, as the console does

**2026-09-25**. Every relative offset in a shader header is **self-relative**: the absolute address
is the field's own address plus its value. The console's capture says so for each field
(obSCEne `166-agc/create-shader`, header at `0x7eeffb490`, raw bytes from the probe's
`agc_retail_hdr_full_0`):

| field | raw | console wrote | field + raw |
|---|---|---|---|
| `+0x08` | `0xd8` | `+0xe0` | `+0x08 + 0xd8` |
| `+0x20` | `0x70` | `+0x90` | `+0x20 + 0x70` |
| `+0x28` | `0x38` | `+0x60` | `+0x28 + 0x38` |

Worklog 529 read `+0x08` as "base + offset + 8" and the others as base-relative. The first matches
by coincidence; the others do not. `create_shader` relocated the group pointers `+0x18..+0x38` as
`header + rel` and the sub-table entries as `sub_table + rel`. Both are now `field + rel`. The unit
test had asserted `header + 0x70`, which was the belief and not the capture. It now asserts the
capture, and was watched failing with the sub-table put back base-relative.

**What it cost PPSA25872.** Its slot table 0 landed eight bytes short, on `0xffff` padding, so every
constant slot read as unmapped. The right table (`ffff 8000 …`, table 3 `8004`) carries the valid
bit on each mapped entry.

| title | result |
|---|---|
| PPSA25872 | FURTHER: +14 distinct imports, +64,573 calls. Past the constant upload, to a read of `0x52` at `the title's own modules+0xe3b20`. |
| PPSA02664, PPSA03416 | Wall moves from `image+0x3f8f0` to `image+0x3f840` in the same routine, 2 imports fewer each. |
| PPSA28061 | same |

**The Unity pair converge, and 790's null is named.** At `image+0x3f840` the upload now gets a
*valid* register index, `0x4e`. That is at least `0x20`, so it goes to the extended user-data buffer
at `[obj+0x18]`, which is null. The lazy setup (`image+0x3f300`, then `image+0x3ede0`) sizes that
buffer from `[obj+0x34] & 0x3fffffff`. When that is zero it stores null and returns. So worklog 790's
`array[1]+0x18` null is this extended buffer, and the question for all three Unity titles is what
sets `obj+0x34`, a count derived from the shader. The two lost imports are the old path the wrong
table sent them down; the relocation is the console's, measured.

The object moves between runs (`0x740001ec78a0`, `0x740001cc78a0`), so an address watchpoint cannot
catch the writer of `+0x34`. Scanning the code around it for stores to `+0x34` found it instead:
`image+0x3f230` binds a shader and sets `obj+0x34 = *(u16 *)(sub_table + 0x28)`. That `u16` is 0 in
the title's header and in the probe's.

**Two bytes the model does not write.** Replaying this relocation on the probe's own header gives
42 changed bytes with extent `0x105`. The console measured 44 with extent `0x130`. So
`sceAgcCreateShader` writes two more bytes, and a count at `sub_table+0x28` would be exactly that.
The log kept only the first 64 bytes, so REQ-20260925T2056Z-5d19 asks for the full before/after
`0x130` bytes, rather than guessing the value.
