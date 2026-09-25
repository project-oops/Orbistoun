# 832. Draws run at submit and are written back into the guest's target, and both baselines reach their next frame

**2026-09-24**: after worklog 830 both fully-owned baselines were stuck on the fence after their
first draw submission. D705/D710 retire a fence only for work that ran, and a draw was work nothing
ran until the guest had stopped. Worklog 831 laid the ground: whole-surface `64KB_R_X` tiling in both
directions, on run 18's 1080p readback. This unit is the rest, recorded as **D712**.

## The mechanism

- **The command processor** (`cp.rs`) asks `CpMemory::run_draws` once, at a stream's first draw.
  - If it answers yes, every draw packet passes as done, and the `RELEASE_MEM` after them retires.
  - It is asked only when nothing between the first draw and the last touches memory. Otherwise the
    stream stops by name, `Stopped::DrawsInterleaved`, before any draw runs.
- **The submit** (`agc_driver.rs`) answers through a `DrawExecutor` the worker installs. It checks the
  target first (`writable_target`): one `CB_COLOR0_BASE`, one resident target, `64KB_R_X`, and
  `8_8_8_8` `UNORM`.
  - `draw_over` reads the target out of guest memory, detiles it, puts it in `Rgba8` order, and hands
    it to the executor.
  - It then re-tiles the answer and writes it back through the checked write path.
- **The component order is now decoded.** `CB_COLOR0_INFO` (`0xA31C`, `gfx103.json`) gives
  `FORMAT`, `NUMBER_TYPE` and `COMP_SWAP`. The SDK's GL display targets are `COMP_SWAP=ALT`, bytes B,
  G, R, A (oops-sdk `gl_draw.c`; Mesa `ac_formats.c:614-619` gives `ZYXW` for four channels), so
  write-back exchanges the first and third bytes.
- **The worker's executor** (`render::execute_draws`) seeds the backend's target with the guest's own
  pixels. This is the new `VulkanBackend::seed_target`, the core of `REQ-...77fa`.
  - It drives the submission. Only a frame with no refusal and the right size is written back.
  - The run's ending presents that frame rather than drawing the submission a second time over itself.

## Surprise: host code on a guest stack

The first run ended the worker with status `0x40010006`, which is `DBG_PRINTEXCEPTION_C`, the
exception `OutputDebugString` raises.

- The submit handler runs on the guest thread's stack. The vectored handler rightly passes the
  exception on (`CONTINUE_SEARCH`). But frame-based dispatch then walks a stack that carries no unwind
  data and lies outside the thread's recorded limits, so the dispatch fails and the process ends.
- A graphics driver raises exceptions as ordinary business. The executor therefore runs on a **host
  thread of its own** (`std::thread::scope`) while the guest thread waits.
- The same holds for any future host-heavy HLE call: *host code that may raise needs a host stack.*

## What moved

**The cube (GLCB00001)** now runs all 5 of its submissions to completion, 4 with draws. The SDK's own
frame loop reports `frames-confirmed: 0x5` and `AGC hardware frame rendered and flipped`, where
before it confirmed one frame.

The last frame (`frame-1.bin`) shows the cube over the guest's own clear colour, `0x0d121f`. That
colour comes from the guest's DMA fill, carried through the seed; the backend's black is gone. The
cube is still drawn inside-out, because depth and cull are not applied (`REQ-...2ea9`).

**Neverball (NVRB00001)**:

- Its first draw submission ran at submit: 907 commands, 0 refused, 12.2 s, written back. Its fence
  retired, and **the game submitted its next frame**.
- That frame names 7 textures, including 256x256 and 512x512 images: the title screen's art.
- Its draws end in a device loss: `draw (mesh): Vulkan("device_wait_idle", ERROR_DEVICE_LOST)` after
  1.9 s. A translated mesh shader in the new frame faults or hangs the GPU.
- The drive error used to be swallowed; it is now printed. After the device is lost the end-of-run
  render drives nothing.

## Cost

Neverball's 450 draws took 12 s, because each draw copies the 1080p attachment in and out. The cube's
frames take 0.3-3 s. That is correct but slow, and it is noted in D712 as a cost item. It does not
justify posting fences early.

## Tests

`orbistoun-gpu` went from 104 to 112 unit tests. The new ones:

- **`cp.rs`, three tests:**
  - draws carried out together let the fence retire, with one executor call for three draws;
  - an executor that declines leaves the fence unwritten;
  - memory work between draws is refused by name before the executor is asked.
- **`registers.rs`:** `CB_COLOR0_INFO` decodes format, number type and swap, and only `8_8_8_8`
  `UNORM` in standard or alternate order qualifies.
- **`agc_driver.rs`, four tests:**
  - the `SWAP_ALT` exchange and its round trip;
  - `writable_target` refuses a second base, linear tiling, and a missing format;
  - a drawn frame lands tiled and blue-first over the target's own contents;
  - a target outside guest memory is refused unwritten, without the draw being run.

## Next

Neverball's second frame loses the device. Find which draw and which translated shader, for example
by driving that submission one draw at a time on a fresh device.

## Gate state

`./bin/orbistoun check` green, decisions and worklog indexes regenerated, identity scan clean. No
commit.
