# Controllers

`orbistoun-input` implements the guest's pad library itself; it wraps no host gamepad
library. It models up to four ports, opening and closing a pad, and the state a title reads.
The window reads the host's keyboard and, on Windows, its gamepads, and hands the result in.
Buttons are named by position, never by glyph: `south`, `east`, `west`, `north`, `l1`, `r1`,
`l2`, `r2`, `l3`, `r3`, `up`, `down`, `left`, `right`, `select`, `start`, and `shell`.

## What a title reads

A pad read answers the whole 120-byte state image measured on hardware. The window's pad state
is placed into it at the fields the SDK layout names (D713); with nothing pressed the title
reads the measured at-rest image. A port with nothing driving it is a pad nobody is holding,
which a title can enumerate like any other.

The `shell` button never reaches a title: the window keeps it for the shell overlay and power
menu (see [the user guide](user-guide.md#the-shell-button)). While the overlay is open, the
title reads a pad nobody is holding.

## The controllers pane

preferences - controllers sets the ports:

- controllers - how many ports, one to four.
- port N - what drives it: empty, keyboard or gamepad. A gamepad port names which of the
  host's connected gamepads it reads, counting from 0; buttons map by position (south is the
  bottom face button), the sticks and triggers are analogue, and the gamepad's home button is
  `shell`. One not connected reports a pad nobody is holding. Gamepads are read on Windows
  only; elsewhere a gamepad port reports a pad nobody is holding, and a run says so under "on
  the host".
- a key for each button and each stick direction. Each row lights, and says "down" or
  "pushed", while its key is held, so a mapping is checked where it is edited.

Key names are the window's: `ArrowUp`, `Enter`, `Backspace`, `Home`, or a single letter or
digit. Two buttons bound to one key, and a name nothing recognises, are listed in red under the
ports. Save writes the mapping to `config.toml`; it applies to the next run.

## Default keyboard mapping

Port 1 is the keyboard by default:

| Button | Key |
|---|---|
| up, down, left, right | arrow keys |
| south, east, west, north | `K`, `L`, `J`, `I` |
| l1, r1 | `Q`, `E` |
| l2, r2 | `1`, `3` |
| l3, r3 | `Z`, `C` |
| select | `Backspace` |
| start | `Enter` |
| shell | `Home` |

| Stick | Up, down, left, right |
|---|---|
| left | `W`, `S`, `A`, `D` |
| right | `T`, `G`, `F`, `H` |

In `config.toml`:

```toml
[[pads.ports]]
source = "keyboard"

[pads.ports.keys]
south = "K"
start = "Enter"

[pads.ports.axes]
left-up = "W"
```

## Pad scripts

A pad script drives a port from a file, so a run with no window can press buttons and two runs
of one script give the same input (D707). Each step sets the pad at a moment and holds it until
the next step; a press and its release are two steps.

```toml
[[step]]
at_ms = 2000            # milliseconds from the start of the run
buttons = ["start"]

[[step]]
at_ms = 2100
buttons = []            # released

[[step]]
at_ms = 3000
left_stick = [0.0, -1.0]   # x, y in -1.0..1.0, y positive down
triggers = [0.0, 1.0]      # left, right in 0.0..1.0
```

A step names one clock: `at_ms`, host milliseconds from the start of the run, or `at_flip`,
the guest's flips since the start, which presses at the same point in the title however fast
the host runs it. A script uses one clock throughout, and steps out of order are refused rather
than sorted.

A step can wait for the title's phase instead of a time (D987). `wait_for_draws = N` holds it
until the guest has closed a frame of at least `N` draws, and no earlier than its own time; the
steps before it run until then, and the steps after it count their time from the moment it took
effect. A world frame is far heavier than any menu, so in-world input lands in the world however
long the menus and loads before it took:

```toml
[[step]]
at_flip = 4000          # not before flip 4000
wait_for_draws = 5000   # and not before a frame of 5000 draws

[[step]]
at_flip = 6             # six flips after that frame
right_stick = [1.0, 0.0]
```

The run logs `input script: step N took effect at ...` when a wait is met.

Name a script as a port's source in `config.toml`, relative to the directory holding it:

```toml
[[pads.ports]]
source = "script"
path = "inputs/press-start.toml"
```

or give one for a single run with `orbistoun-cli run <title>/eboot.bin --input <script>`.

## Capturing and replaying input

The toolbar records what a title reads from its pad and plays it back (D721).

- capture input - starts recording, into the running title from now, or with nothing running
  from the start of the next launch. The file is `input/<title>-<unix ms>.toml` under the logs
  directory, a pad script keyed to flips. Press stop capture to end it; hover "captured" for
  the path.
- playback input - a menu of the newest captures. The chosen file plays into the running
  title from now, or with nothing running from the start of the next launch. stop playback
  ends it.

A capture and a playback end with the run, and nothing replays into the next launch unless
chosen again. `orbistoun-gui --title <name> --playback <file>` launches a title with a capture
playing, and `orbistoun-cli run --input <file>` replays one without a window.
`orbistoun-cli run --capture-input <file>` captures without a window.

## Checking that the title answers

`orbistoun-cli motion <frames> --input <capture>` says whether a held stick moves the picture
(D988). Keep a sequence of frames from the title's first heavy frame
(`ORBISTOUN_FRAME_AT=busy:5000,busy-every:1` with `ORBISTOUN_FRAME_DIR=<frames>`), capture the pad
the guest read, and the command measures the change between each two consecutive kept frames,
split by whether a stick was held between them:

```bash
orbistoun-cli motion <frames> --input <capture> --region 0.25,0.25,0.5,0.5 --from 5900 --expect 2
```

`--region` is `x,y,width,height` as fractions of the frame; `--expect` fails the command unless
the pairs with a stick held changed that many times as much as the pairs at rest.
