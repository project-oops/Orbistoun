# 813. `sceAgcDriverSubmitCommandBuffer` is named and implemented, and both fully-owned baselines move to the wall that was always there: the guest waits for a GPU completion orbistoun cannot yet produce — verdict BACK, kept on purpose

**2026-09-24** — the submit hash worklog 809 left, `0x145f597e80e9876f`, the gap between the GL
context building a draw and a draw being handed over.

## The name

The open-toolchain SDK declares it weak and tries it before `sceAgcDriverSubmitDcb`
(`gl_context.c`, `agc_draw.c`): `sceAgcDriverSubmitCommandBuffer(queue, desc)`. The name was proved
the project's usual way — `orbistoun-cli names` over Neverball's eboot reproduced the hash from the
grammar (`prefix-module-verb-object-object`), recorded `found = "generated"` in
`symbols/generated.json` (30,186 names). The string also sits in both eboots, so the module-strings
pass names it too.

## The implementation

`submit_command_buffer` reads the 16-byte descriptor from **argument one** — argument zero is the
queue handle `sceAgcDriverCreateQueue` wrote (worklog 809), orbistoun's own opaque object — and goes
down exactly the path `submit_dcb` takes with its argument zero; the shared body is now
`submit_described`. It answers `SubmitDcb`'s measured `0x0`. That the same code holds for this entry
point is written down as an assumption: obSCEne's `arm2-submit-shape` rows record an arity of 2 and no
fence argument, which agrees with the guest, but no rc was measured for this symbol. The knowledge
entry is `guest-observed`; `submit_command_buffer_reads_the_descriptor_after_the_queue` pins that the
descriptor is read from argument one and that argument zero is never read as one.

## What it did to the runs, and why that stays

Both baselines go **BACK**:

| | before | after |
|---|---|---|
| Neverball | 49 imports, 20,000,000 calls (budget), 475 frames | 23 imports, 38,296 calls, 38,209 of them `sceKernelUsleep` |
| cube | 21 imports, 150,996 calls | 14 imports, 38,249 calls, 38,210 `sceKernelUsleep` |

The first submit either guest makes is the GL context's hardware clear self-test. Before, it landed on
the stub, the placeholder read as *the driver refused the command buffer*, the context marked its
hardware path failed, and the game carried on painting with drawing switched off — the "progress" of
worklogs 809-812 was a guest that had given up on the GPU because orbistoun gave it an answer the
console never gives. Now the submit succeeds, and the guest does what it does on hardware: it polls
the fence the stream's two closing `RELEASE_MEM` packets write (`0xbeefcafe`, then the GPU clock
counter at `fence + 8`), a hundred thousand `usleep(10)`s, before it proceeds. Orbistoun executes
nothing, so nothing writes the fence, and each poll runs its full bound — at the host's sleep
granularity, longer than the 20-second run.

**Writing the fence here would be D705's declined shortcut.** D705 decided that driver-work
completion is posted only when execution lands, because posting a completion for work that never ran
is a claim that retired work produced a result it did not. Writing `0xbeefcafe` from a submit that
rendered nothing is that claim in its plainest form, so it is not done. D705 also says how this
retires: orbistoun processes a submission synchronously, so when execution lands (`36c0`) the fence is
written by the work that ran, the instant the executed submit returns. That is the rendering-path
cluster in the inbox — the decoded frame dropped at `render.rs` (`REQ-...1f07`), the handover that
renders at most one frame — and it is now the thing standing between both baselines and their next
frame. Reverting to the stub would buy the old call counts back with a wrong answer.

## Gate state

`crates/orbistoun-gpu/src/agc_driver.rs` (declaration, `submit_command_buffer`, `submit_described`,
test), `crates/orbistoun-hle/data/knowledge/libSceAgcDriver.toml` (the entry),
`symbols/generated.json` (the name). `orbistoun-gpu` 92 tests pass. `./bin/orbistoun check` green,
worklog index regenerated, identity scan clean. No commit.
