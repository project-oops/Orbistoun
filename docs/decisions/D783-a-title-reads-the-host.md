# D783 - A title reads the host keyboard

**Status:** decided
**Date:** 2026-10-09
**known_by:** measured (the calls' answers: `100-input/keyboard-and-media-gates`,
`101-input-ext/keyboard-lifecycle` and the census, `reports/hardware/20261009-151440-eboot.obs.log`);
assumed (what a keyboard with nothing held reads: the probe console's keyboard was a remote control
presenting itself as one)

`libSceKeyboard` is served, and the keyboard a title opens is the host's, as the network a title
asks about is the host's. `sceKeyboardInit` answers 0. `sceKeyboardOpen` gives the signed-in user a
small positive handle, refuses a second open for them with `0x80da0004`, any other user with
`0x809b0001` and all-zero arguments with `0x809b0081`. `sceKeyboardReadState` writes the whole
96-byte record and answers 0: present at `0x10`, the count of keys down at `0x14`, and up to sixteen
USB HID usage codes at `0x20`, the layout oops-sdk reads and SeaShell navigates by; handle `-1`
answers `0x80da0003`. `sceKeyboardClose` frees the handle and answers 0, or `0x80da0003` for `-1`.

The keys a read reports are the ones a shell has said are held. A run with no shell owning a
keyboard reports a keyboard present with nothing held. A key the window has bound to a pad button
is reported to the pad, not to the keyboard, so one press never reaches a title twice; the window's
wiring follows this decision.

**Why:** every oops-apps title opens the keyboard through oops-sdk at start-up - Craft,
SuperTuxKart, Ship of Harkinian, TSHP and Neverball among them - and each was answered a
placeholder. The user chose the host keyboard on 2026-10-09.
