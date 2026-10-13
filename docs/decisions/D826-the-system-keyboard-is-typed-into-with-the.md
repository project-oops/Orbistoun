# D826 - The system keyboard is typed into with the controller

**Status:** decided
**Date:** 2026-10-13

A title that asks for text opens the system keyboard: `sceImeDialogInit` with a parameter block
naming a UTF-16 buffer and the most characters it takes, then `sceImeDialogGetStatus` each frame
until the dialog has finished, then `sceImeDialogGetResult`. On the console the system software
draws a keyboard over the title, the player types into it with the controller, and the title sees
none of those presses.

orbistoun does the same. `Init`, once the common dialog is initialised, opens a keyboard
(`orbistoun_shell::keyboard`) over the title's field, keeping the text already in it. While it is
open it has the controller - the window's pad, or an input script (D721), the same source the
title would read - and the title reads a neutral pad, connected with nothing pressed, as it does
when the shell has focus (D345). Each button that goes down is a press: the directions move a
highlight over a grid of keys, the lower face button presses the highlighted key, the left
deletes, the upper types a space, the right cancels and the right centre button ends the entry.
`GetStatus` reads the controller each time it is asked and answers running (1); once the entry
ends, the text goes into the title's buffer as UTF-16, cut to the most it takes and terminated,
the status reads finished (2) and the result's end status OK (0), or cancelled (1) with the buffer
left alone. Abort ends it aborted (2); Term closes it, after which every call answers as with no
dialog open (measured, `REQ-b874`). Before the common dialog is initialised `Init` answers
`0x80bc0015`, as measured.

The keyboard's view - its keys, highlight and text - goes to the front end as it opens, as each
press changes it and as it closes (`Event::Keyboard`), and the window draws it over the title's
picture. It is not drawn into the title's frames: the console composites its system software over
the title, not into it. An input capture records the controller as it was read, whether the title
or the keyboard read it, so playing a capture back types the same text.

The layout is a plain grid of our own - letters, digits and a few symbols, shift, space, delete
and done - reachable with the four directions and one button. No artwork, arrangement or
proportions of anyone else's are copied.

**Why:** a title waiting on the keyboard waits forever with no one to type. SuperTuxKart's first
screen is "Create User", whose name field opens it (`oops-deps/sdl2`'s `ShowScreenKeyboard`,
through `oops_dialog_ime_open`). Typing with the controller, through the input the rest of a run
uses, keeps a run repeatable and a capture replayable, and is what the console does.

**Assumed (`known_by = assumed`):** the parameter block's layout - the buffer at `+0x28`, its most
characters at `+0x24` - the status codes 1 and 2, and the end statuses 0, 1 and 2. They are what
oops-sdk's `system/dialog.c` uses, which opens this dialog on the console (its password field was
tried there), so a sibling's hardware use stands behind them, not a probe. That the title reads a
neutral pad while the keyboard is open is assumed from the console's behaviour with its other
system overlays (D345). A dialog opened while one is open, and `GetResult` before the dialog has
ended, are not modelled and answer the placeholder.
