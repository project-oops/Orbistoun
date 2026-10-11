# D791 - The shell answers a message dialog at once, with OK

**Status:** decided
**Date:** 2026-10-11

A message dialog a title opens is shown to nobody. `sceMsgDialogOpen` answers 0 and the dialog
runs; `sceMsgDialogUpdateStatus` reports it running once and then finished;
`sceMsgDialogGetResult` then reports the OK button, with a zero result code; and
`sceMsgDialogTerminate` returns the dialog to none, so the next one initialises again. Status
codes are none 0, initialised 1, running 2 and finished 3, and the OK button's id is 1.

Assumed (`known_by = assumed`): the status codes, the result layout `{ mode, result, buttonId }`
and the OK button's id are the common dialog's as public SDK headers give them, used as an oracle,
not measured. obSCEne could not bind `libSceMsgDialog` to measure the cycle (REQ-md01, twice:
every symbol unbound by link and by `dlsym`), so a measurement replaces these when one exists.

**Why:** the user chose it on 2026-10-11 over waiting on md01 and over a dialog the user
dismisses. PPSA02664 opens a user-message dialog at start-up and, refused, retries it every second
and never leaves its first scene.

**Rejected:**
- Waiting on md01: it failed twice for want of a way to bind the library.
- Holding the dialog open until the GUI user dismisses it: a real prompt, but a new shell surface;
  this answer can be replaced by it later without the title seeing a difference.
