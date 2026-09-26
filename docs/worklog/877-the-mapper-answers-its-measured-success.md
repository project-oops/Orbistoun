# 877. sceKernelMapperGetParam answers its measured success, and PPSA28061 moves

**2026-09-26**. An audit on the obSCEne side pointed out that the mapper's success is already
measured. `reports/report-1790100990.txt` has it twice, in `137-kernelcall/mapper-param` (a plain
call) and `166-agc/mapper-after-init`. Both answer `rc 0x0` and fill the 56-byte, size-prefixed
structure the same way:

| offset | value |
|---|---|
| `+0x00` | `0x38`, the caller's size, left untouched |
| `+0x08` | `0x80000000000` |
| `+0x10`, `+0x18`, `+0x20` | `0x88000000000` |
| `+0x28` | `0x40` |
| `+0x30` | `0x2663` |

orbistoun still answered `0x80020006`, from the 2026-09-10 cold call (b7e2) that this later sweep
supersedes. Earthion's engine aborts on any non-zero answer (worklog 516).

**Change.** `mapper_get_param` writes those 48 bytes, never past the caller's declared size, and
answers 0. Test: `the_mapper_fills_what_the_console_filled` replays the console's buffer byte for
byte and checks a smaller declared size is not overrun. It was watched failing with the old refusal.
The knowledge entry cites the new sweep and records the old value as superseded.

**Measured:** PPSA28061 goes FURTHER, from 25 to 61 distinct imports and from 394 to 1,013 calls, to
a later `abort`. It is Earthion's first movement since 2026-09-22.
