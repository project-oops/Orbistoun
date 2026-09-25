# 861. Input capture and playback on the toolbar, never automatic

**2026-09-25**. D721, amended.

Worklog 859 recorded every live-input run on its own. The operator rejected that: capturing a
person's input is their decision, made visibly. So nothing is captured unless somebody asks.

**The toolbar:**

- "📷 capture" is renamed "📷 screenshot", which is what it does.
- A new section after screenshot and record:
  - **"🎮 capture input"** toggles to "⏹ stop capture". With a title running, it captures from now.
    With nothing running, it arms the next launch, which captures from its entry, so the capture
    replays from where it began. The file is `<logs>/input/<title>-<unix ms>.toml`, named by the
    GUI, and its path shows on hover.
  - **"▶ playback input"** is a menu of the 20 newest captured files, plus "stop playback". With a
    title running, the chosen file plays from now; otherwise the next launch plays it from its
    entry.
- A run's capture and playback end with the run, on both sides.

**The protocol:**

- `Request::CaptureInput { to }` and `Request::PlayInput { script }` are answered on the worker's
  reading thread, like `Input`, with no reply.
- `Request::Run` gains `capture_input` for an armed capture, beside `input_script`.
- The worker's automatic `record_input` is gone. `capture_input` and `play_input` do what the
  toolbar asks.

**A capture bug the new test found.** Two changes inside one flip (a title reads its pad several
times a frame) wrote two steps at one flip, which is a script its own validation refuses. The
later change now goes to the next flip. Every state the guest saw is kept, and a press shorter than
a frame replays a frame long.

Tests: `input_is_captured_only_when_asked_and_reads_back_as_a_script` in the worker, and the input
crate's capture test, extended to changes within one flip.

The page-guard history retries its lock a bounded number of times. Two tests reading it at once
found that a single `try_lock` could answer "busy" to a real fault report as well.
