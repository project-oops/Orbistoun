# 767. GTA polls input without opening a pad; the handler bring-up has one caller, an input-manager-init that never runs before the poll

**2026-09-21** — worklog 766 repointed PPSA04263 (Grand Theft Auto V)'s null-vtable at the input
bring-up's trigger and called it a trace question. This traced it. The bring-up has exactly one caller,
and that caller - an input-manager-init - does not run before the update loop polls, so GTA reaches its
input poll with the manager uninitialised. The wall is now a bounded call-chain climb rather than an
open question.

## The trace: polled, never opened

A normal run's import census, filtered to input:

- `libScePad::scePadReadState` - **666 calls** (the per-frame poll)
- `libSceKeyboard::sceKeyboardReadState` - 263, `libSceMouse::sceMouseRead` - 188
- `libScePad` otherwise: **nothing**. No `scePadOpen`, no `scePadInit`, no `GetHandle`. The only
  `libSceUserService` call is `sceUserServiceGetGamePresets` (1), never `Initialize`.

So the update loop is polling a pad subsystem **nothing ever opened**. That is the outward face of the
null-vtable: the poll at `image+0x1967680` reads `element[0].field_0x2d0` (still the static-init
relocation `image+0x5b37e98`, an un-constructed handler) because the code that opens the pads and
redirects the field to the constructed default has not run.

## The bring-up's one caller

A full-text scan of direct call sites:

- `image+0x1965c00` (the bring-up: four-pad open loop + the `field_0x2d0` redirect at `image+0x1965fbc`)
  has **exactly one caller: `image+0x197bf38`**.
- `image+0x1967680` (the faulting poll) has **six** callers (`0x502164`, `0xd80167`, `0x114c24d`,
  `0x1966701`, `0x19669ad`, `0x2633eac`) - the update path is reached many ways and clearly runs.

`image+0x197bf38` sits inside an **input-manager-init**: the code just before it zeroes a block of
globals at `image+0x5d734xx`/`0x5da62xx`, then `call`s the bring-up, then `call 0x1966470`, then indexes
the same `element` array (`image+0x5521c20`, stride `0x4d0`). This is the one-time initialisation of the
input manager, and the bring-up is its subroutine.

## What is established, and what is not

Established: the bring-up never runs before the poll (worklog 765's watchpoint: `image+0x1965fbc` never
wrote `field_0x2d0`), and its only caller is this input-manager-init. So the manager-init itself does not
run before the update loop reaches the poll. The poll runs on guest thread `0x5e2d00049b00`; GTA spawns
many threads (`scePthreadCreate` 178), so init and poll are plausibly on different ones.

Not yet established: **why** the manager-init does not run first. Two shapes remain, and the next step
separates them - climb one level (find `image+0x197bf38`'s enclosing function and *its* callers) and
either (a) a gate skips the manager-init on a flag orbistoun answers wrongly, or (b) it is a threading
order - the init runs on a thread that is blocked or not yet scheduled when the poll thread starts,
which would move the question from "which branch" to "which wait". A write-watchpoint on one of the
init's own globals (e.g. `image+0x5d73438`) confirms whether it runs at all and on which thread, cheaply,
before more disassembly.

## Gate state

No code changed - a characterisation that traces the input census, finds the bring-up's single caller
and names the input-manager-init as the missing step, and frames the remaining question as a
gate-versus-threading fork with a cheap watchpoint to split it. `./bin/orbistoun check` green, worklog
index regenerated, identity scan clean. No commit.
