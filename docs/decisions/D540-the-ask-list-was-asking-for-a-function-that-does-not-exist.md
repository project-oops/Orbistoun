# D540 - The ask list was asking for a function that does not exist

**decided** - 2026-09-04

Fourth pass over the ask list. The first three found it **incomplete** (D537), **unable to see
itself repeat** (D538), and **repeating itself at the source** (D539). This one found it saying
something **untrue**.

## Thirty-three entries, nine of them wrong

The premise read *"Semantics follow the POSIX analogue of the same name."* Twenty-four of its
members are `scePthread*` and `posix_pthread_*` calls, where that is exactly right. The other
nine are the event-flag and semaphore families:

```text
sceKernelCreateEventFlag  sceKernelDeleteEventFlag  sceKernelPollEventFlag
sceKernelSetEventFlag     sceKernelClearEventFlag
sceKernelWaitSema         sceKernelPollSema         sceKernelSignalSema  sceKernelDeleteSema
```

**POSIX has no function of any of those names.** There is no `CreateEventFlag`; semaphores are
`sem_post` and `sem_wait`, which is not what `SignalSema` and `WaitSema` are called. The
sentence was not vague, it was false - and it read exactly like the twenty-four beside it where
the claim holds.

## What the false sentence was covering

`sceKernelWaitSema` is `(semaphore, need, timeout)`. It had **one** open question, and that
question was the wrong sentence. Nothing recorded what the implementation does:

```rust
fn kernel_wait_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, sync::Blocking::Forever))
```

`sync::semaphore_wait(handle, until)` takes no count and no deadline. **Two of the three
arguments are ignored**, so a guest asking for three units with a timeout gets one unit and an
indefinite block - and the knowledge base said nothing about it while claiming the semantics
came from POSIX.

That is principle 3 one level up from where it usually bites: not a stub returning success, but
a *record* describing a call that is not the call.

Same shape in `sceKernelPollEventFlag`, where only bit 0 of the mode word is modelled, and
`sceKernelCreateEventFlag`, where the attribute word and the fifth argument are not.

## What replaced it

One sentence that is true of all nine:

> There is no POSIX function of this name: the semantics are inferred from the name, the
> argument shape and the rest of the family, not from a standard. Nothing on the target has
> confirmed them.

Plus, per entry, the split this audit keeps finding - **what orbistoun does** into `edge_cases`,
**what the platform does** into `assumptions`. `sceKernelWaitSema` went from one false question
to one recorded fact and three answerable questions, including what a `need` above one asks for
and whether the third argument's zero means poll.

And one inconsistency the reading turned up: a bad handle answers the **measured** ESRCH
(`0x80020003`) in the event-flag family and this project's **placeholder** (`0x7fff_0003`) in the
semaphore family. obSCEne measured only event flags, so whether the two families agree is now
recorded as a question rather than left as a difference nobody had noticed.

## The guard

`a_claimed_posix_namesake_is_one_the_harvest_lists` spells a vendor name into what it would be
in the C library - `scePthreadCondWait` to `pthread_cond_wait` - and requires that name to be in
`orbistoun-names`' harvest of FreeBSD's exports. A **transformation of the name**, not a
judgement about behaviour.

It separates the cases cleanly: all 32 current claimants map to a harvested name, and all nine
removed ones do not. Made to fail by putting the claim back on two entries.

What it cannot do is say a claim it passes is *true*: `pthread_cond_wait` existing proves the
name is real, not that the platform's call behaves like it - which is what the question is
admitting is unknown. It catches the claim that is wrong on its face.

## Two merges, and a count that went up

`Semantics follow the POSIX analogue of the same name` existed in two wordings differing by a
semicolon - the D538 defect again - and is now one premise across **32** functions. The errno
question is one premise across **28**, after the placeholder clause was split out of the
twenty-four entries that had glued it onto their question.

```text
before   713 questions   135 premises
after    745 questions   140 premises
```

**The count went up, and that is the correct direction here.** A false sentence was hiding real
unknowns; replacing it with true ones exposed them. This axis is not about shrinking the list -
D539 said so and this is the tick that proves it, because the honest move made the number worse.

## And a negative worth recording

The other half of the plan was to check whether the D539 pattern - one question with a symbol
inside it, hiding as many - recurs among the 108 singleton premises. **It does not.**

Two filters, both stated: replacing every backtick-quoted span with a placeholder groups **zero**
additional premises, and pairwise similarity over all 135 sentences finds five pairs above 0.72,
all of them two entries or fewer. D539 was the only instance, and it is closed.

Using similarity to *find candidates for a person to judge* is not the same as grouping by it,
which D538 refuses and still refuses.
