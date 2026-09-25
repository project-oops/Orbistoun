# 841. The running title is shown in the window

**2026-09-24**. Every flip streams to the window as it happens.

- **Video:** a flip observer detiles the flipped scanout, a BGRA surface in tiled mode 0. The
  result goes into an 8-slot RGBA frame-region ring, throttled to one frame per 50 ms, and is sent
  as `Event::Frame`.
- **Worker and GUI:** the worker streams events while a run is still going (`request_streaming`),
  and the GUI shows the newest frame in its central panel. `write_message` now writes each line in
  a single call, so two streams can never interleave mid-line.
- **Service:** the GUI's run defaults are now no time limit and no call budget. A title being
  played was being stopped at 20 s, and that looked like a crash.

**Result:** SeaShell's home screen and Neverball's menu show live in the window.
