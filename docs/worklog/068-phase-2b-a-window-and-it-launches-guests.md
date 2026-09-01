# Phase 2b: a window, and it launches guests


`orbistoun-gui` on egui (D161). Library on the left from `Service::discover_titles`,
per-title container and import summary on the right, and **Launch works** - phase 4 landed,
so the roadmap's "disabled until phase 4 can honour it" no longer applies.

The run view shows the progress verdict, the stack-conformance line, the ordered call tail
and the ranked import list - the same values the CLI prints, from the same code, because
the comparison moved down a layer (D160). `Verdict::label`/`summary` live on the type, so
the two cannot word one measurement differently.

Runs happen on a thread with the result arriving by channel, and the update loop asks for a
repaint while one is in flight - immediate mode only redraws on input, and a guest on
another thread is not input, so without that the spinner freezes and the result appears
only when the pointer moves.

Worker mode is checked before the window is created. `spawn_self` re-executes this binary
with a flag, so reaching window code in a worker process would open a second window on
every launch.

Verified: worker mode inside the GUI binary serves and exits cleanly; the window opens and
stays up; the CLI is byte-identical afterwards (46 imports, `image+0xecda`, 799 conforming
calls). Five crates clean on `-D warnings`, no failing suites.

Deliberately absent: any output surface. The guest runs in a child process while the window
lives here (D032), so presenting a frame needs a reparented window or shared images. There
is no frame yet, and building the mechanism before there is one is speculation by principle
12's own test.

