# D500 - Every A/V/input subsystem implements the lifecycle and not the data path

**measured** - 2026-09-03 (declared-vs-bound across every subsystem crate, against the 65-run corpus)

`orbistoun-cli status` reports **674 functions declared, 647 implemented** - 96%, which reads as
nearly finished. Measured per subsystem against what guests actually call, it is not what that
number suggests, and the shape is identical in all three places:

| | declared | bound | the unbound ones |
|---|---|---|---|
| input (`libScePad`) | 13 | 9 | `scePadReadState`, `scePadReadStateExt`, `scePadRead`, `scePadReadExt` |
| audio (`libSceAudioOut`) | 5 | 2 | `sceAudioOutOpen`, `sceAudioOutOutput`, `sceAudioOutSetVolume` |
| graphics (`libSceGnmDriver`) | 6 | 1 | five of the six submit/dispatch entry points |

**A guest can open a pad, set its vibration and its light bar, and cannot read a button.** It
can initialise and close an audio port and cannot open one or write a sample. In the corpus,
`scePadReadState` is **519 of the 547 pad calls ever recorded** and it is a stub.

## Why this is worth a decision and not just a worklog line

Because 96% is the number on the status page, and it is measuring the wrong thing. Declared
surface is chosen by us; the four pad functions nobody implemented are declared, so they count
against the denominator and the ratio still reads 96%.

**The lifecycle calls are the easy half and they are also the half that looks like progress.**
`scePadInit` returning success is indistinguishable from a working input stack until something
asks which buttons are down - which is principle 3's argument, occurring one level up from the
individual stub.

The honest summary of capability is not a percentage. It is: *the guest reaches the interface,
and no data crosses it.*

## What this does not mean

**It is not a backlog and nothing here is behind.** `docs/ROADMAP.md` puts the first frame at
Phase 6, which has not begun, and Phase 5 (threading) is the one in progress. There is even a
roadmap item named *"Phase 6's contents, built ahead of it"* - the 14,600 lines of
translate/shader/SPIR-V machinery that exist with no caller. The data paths are unstarted on
purpose, in dependency order, which is principle 6.

So this decision changes no priority. It records that **the coverage figure cannot be read as
capability**, so nobody reads 96% and concludes the subsystems work.

## Two of the three are blocked on layouts, and that is the real constraint

`scePadReadState` needs the pad state structure and `sceAudioOutOutput` needs the port
parameters. Neither can be written without inventing a layout, which principle 1 forbids and
D008 forbids in the probe. They belong in obSCEne's backlog 022 sweep - `libScePad` is censused
there, so a signature is what unblocks it, not a console run.

The graphics case is different again: orbistoun declares **Gnm**, the previous generation's API,
and the corpus shows guests calling **Agc** - `sceAgcCreateShader`, `sceAgcDriverGetDefaultOwner`
and four more - of which **zero are declared anywhere**. That is not a stub gap, it is the wrong
library modelled, and it is worth knowing before Phase 6 starts.
