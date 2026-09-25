# 851. One device thread, and write-watched guest memory

**2026-09-24**. The latest Neverball package (the one that runs perfectly on hardware) is synced and
renders correctly here. It is now at 10 fps, from 5-6.

- **One device thread:** every backend call ran on a thread spawned and joined for it, which cost
  ~400 us against ~90 us of work, three times a submission. `worker::device_thread::on_device`
  keeps one host thread and waits for its answer. A call made from the device thread runs in place.
  It is tested, including borrows, a nested call and a panic.
- **Write-watched guest memory:** guest reservations carry `MEM_WRITE_WATCH`. `orbistoun_mem::watch`
  turns the host's record into epochs (`mark` and `written_since`) that several askers share. Its
  test makes the write it must see.
  - The target's unchanged check asks the watch first and compares bytes only when the watch cannot
    say, or says the target was written: 0.27 to 0.11 ms per submission.
  - The texel cache reuses a texture nothing has written since it was read.
  - Marks are taken before a read and after our own write-back, so a racing write is seen, never
    lost.
- **Textures:** `ContentHasher` hashes a texture's rows where they lie, to the same value as
  `content_hash` over the gathered texels (tested against the original algorithm). A texture is
  gathered again only when its hash changes. Prepare went from 0.47 to 0.32 ms per submission.
- **Accuracy:** gl1-probe gives the same 110 verdicts and every sampled pixel as the previous build.

**Measured and ruled out:**

- Neverball calls `sceKernelGetProcessTimeCounter` ~500k times a second. The GL context times its
  own work around every call to `0x2e93e0`, found by disassembling the running eboot. But one
  guest call round trip is 12 ns (`thunk/tests/call_cost.rs`, ignored, prints), so this is ~25
  ms/s, not the guest's ~31 ms a frame.
- That 31 ms is the guest's own GL command building.

**What is left:** ~52 submissions a frame at ~1.2 ms each, run synchronously while the guest waits
in its submit. On hardware, submit only queues the buffer and the command processor runs alongside
the CPU.
