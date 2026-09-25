# D721 - pad input is recorded and replayed against the guest's flips

**Status:** decided
**Date:** 2026-09-25

## The question

D704 and D707 made a scripted pad: a file of timed pad states, configured as a pad source, that
the guest reads. Three things kept it from being a test tool:

**Terms.** A *capture* is what "capture input" writes. *Recording* elsewhere in this project means
video, which does not exist yet (the toolbar's disabled "record").

1. **Its steps are timed by the host's clock** (`at_ms`, from `Instant`). A script that presses
   Start at the menu at 14 frames a second presses it in the middle of loading at 30. A
   script's meaning changed with every speed-up this project made.
2. **Nothing writes one.** Reaching a crash five menus deep meant writing each press by hand and
   guessing the timing.
3. **It lives only in the shared `config.toml`.** That is the same file whose `pads` are the
   GUI's controller setup, so scripting a test run meant editing a person's controllers.

## The choice

**A step can be timed in the guest's own flips: `at_flip = N`.** The guest's frame count is its
progress, so a script keyed to it presses at the same point in the title whatever the host's
speed. It counts from where the script starts, at guest entry.

- `at_ms` remains, and a script uses one clock or the other, never both.
- A step without a time, or with both, is refused, as out-of-order steps already are.
- The flip count comes from the video shim (`orbistoun_video::flips_accepted`). It is handed to
  `orbistoun-input` as a function, so neither crate depends on the other.

**Input is captured only when somebody asks,** as a script in that same format. Recording every
live run on its own was tried first, and the operator rejected it: a person's input is theirs,
and capturing it is their decision, made visibly.

- The GUI toolbar has an input section after screenshot/record:
  - **"capture input"** toggles a capture. While a title runs it captures from now. With nothing
    running it arms the next launch, which captures from its entry, so the capture replays from
    where it began (`Request::CaptureInput`, and `Request::Run::capture_input` for an armed one).
  - **"playback input"** lists captured files, newest first. While a title runs it plays the
    chosen one from now. Otherwise it arms the next launch to play it from its entry
    (`Request::PlayInput`, `Request::Run::input_script`).
  - A run's capture and playback end with it.
- What is captured is what the guest *read*. Each time `scePadReadState` hands the guest a
  different state from the last, a step is written, stamped with the flip since the capture
  began. Capturing what the window sent would include presses the guest never polled, and would
  replay them at flips where the guest was not looking.
- Each step is appended to the file as it happens, not saved at the end. A run that crashes
  leaves a capture of exactly what led there.
- The GUI names the file `<logs>/input/<title>-<unix ms>.toml`, and the toolbar shows it on
  hover.

**A run can name its own script:** `orbistoun-cli run <module> --input <file>`, carried in the run
request (`Request::Run::input_script`). It takes precedence over a script source in
`config.toml`.

- A test names its script in the command, leaving the shared configuration alone.
- A capture replays headless with `--input <file>`.
- `./bin/orbistoun run <title> -- <args>` now passes its extra arguments through, as its usage
  already said it did.

## Not done, and why

**Per-title overrides** were the other way to carry a script per title. A run does apply them (the
worker resolves them for its title with `Resolved::for_run`), but a script is a property of one
run, not of the title: the same title is played by hand, replayed, and tested with different
scripts. So it is a flag on the run.

**More than one port.** A script still drives port 0.

## Why it is accurate

Replaying delivers the recorded states at the recorded flips, through the same `scePadReadState`
path live input takes. The guest cannot tell a replayed run from the one that was recorded,
except where it polls more than once a frame and the host's timing within a frame differed. That
is the one case flips do not pin, and it is the same state either way until the next flip.
