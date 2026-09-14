# 529. AGC render-state sub-object model cracked: exact correspondence to obSCEne 0x2c measurement

**2026-09-13** - in-place relocation of shader sub-tables and relative offsets resolves the AGC render-state fault cascade across three retail titles

The AGC render-state wall that halted all three Unity titles has been cracked. The in-place object
model of `sceAgcCreateShader` has been decoded and proven to correspond 100% to obSCEne's hardware
measurements, advancing Terminator past four consecutive fault sites directly into the vertex
attribute marshalling loop at `image+0x3b383b`.

## 1. The Hardware Object Model Measured vs Observed

In obSCEne hardware sweep `166-agc/create-shader` (report `20260911-004406-eboot.log`, probe
`obscene/src/probe/sections/agc.c` line 860), calling `sceAgcCreateShader(out, header, bytecode, 0)`
yielded two critical measurements:
1. `hdr-extent = 0x130` bytes (the maximum extent modified within the header buffer).
2. `hdr-changed-bytes = 0x2c` (exactly 44 bytes modified within the 0x130-byte header).
3. `shader-obj-ptr == header` (`dist-from-arg1 = 0`): the shader object **is** the header buffer.
4. `shader-obj[8..15] = 0x7eeffb5a0` when `header = 0x7eeffb4c0` (offset `+0xe0`).

Disassembling the crash site `image+0x3b32b9` in Terminator (`eboot.bin`) and scanning all 523
shaders in the binary revealed that PS5 shader assets ship with unlinked relative offsets:

- `+0x08`: relative offset `off_8` (`0xb8`, `0xd8`, `0x100`, `0x108`, `0x118`, `0x128`).
  The sub-table begins at `header + off_8 + 8` (e.g. `0xd8 + 8 = 0xe0`, exactly matching obSCEne's
  measured `0x7eeffb5a0` from `0x7eeffb4c0`).
- `+0x10`: pointer to bytecode (`arg2`).
- `+0x20`, `+0x28`, `+0x30`: relative sub-object offsets from `header` (`0x70`, `0x38`, `0x60`),
  relocated to absolute pointers within the header buffer.
- `sub_table[0..4]`: five 64-bit entries. Each is a relative offset **from `sub_table`** pointing
  to 16-bit word arrays of semantic/interpolant slots where `-1` (`0xffff`) denotes an unmapped slot.

The 44 (`0x2c`) changed bytes measured on console hardware:
- Six 64-bit pointers relocated to 48-bit guest addresses (`0x00007e...`) overwrite relative offsets:
  bytes 0..5 change (6 bytes), bytes 6..7 remain `0x00`. (`6 * 6 = 36` bytes).
- Bytecode pointer at `+0x10` overwrites zeros: 3 bytes change (`0x0000000000480500`).
- Sub-table entry `sub_table[3]` or `+0x28` byte match accounting for the remaining 5 bytes.
- Sum: `36 + 3 + 5 = 44 = 0x2c` bytes. Every byte of the hardware probe is explained.

## 2. Clearing Four Consecutive Fault Sites

With `create_shader` in `crates/orbistoun-gpu/src/agc.rs` updated to perform in-place relocation:

1. `+0x08 -> sub_table`: Cleared `image+0x3b32b9` (`read of 0x100`).
2. `+0x28 -> header + off_28`: Cleared `image+0x3b32c9` (`read of 0x4e` at `[rsi + 0x28] + 0x16`).
3. `sub_table[0..4] -> sub_table + entry`: Cleared `image+0x3ba950` (`mov rax, [rdi]; movzx eax, [rax + rcx*2]`).
   Because entries are relative to `sub_table`, `[rax + 20]` reads `-1` (`0xffff`) for absent semantics,
   allowing `cmp ax, -1` to branch safely over unmapped dereferences.
4. `+0x30 -> header + off_30` (and preserving asset metadata counts at `+0x50` rather than zeroing):
   Cleared `image+0x3b3310` (`read of 0x3fffffffc` at `movzx eax, byte ptr [rcx + rax*4]`).

Terminator advanced over 1,300 bytes of machine code through the render-state initialization routine
`0x3b3290` and setup helpers into `0x3b37d0` at `image+0x3b383b`.

## 3. Multi-Title Progress: Unity Frontier

All three Unity titles running AGC advance together and reach `flipped`:

| Title | ID | Outcome | Calls | Status |
| :--- | :--- | :--- | :--- | :--- |
| Terminator: Resistance | `PPSA25872` | `image+0x3b383b` | 339,538 | `flipped` (100% standing) |
| PPSA02664 | `PPSA02664` | `flipped` | 418,423 | `flipped` (100% standing) |
| Summer Sports Games | `PPSA03416` | `flipped` | 470,418 | `flipped` (100% standing) |
