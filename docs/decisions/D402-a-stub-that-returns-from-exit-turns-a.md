# D402 - A stub that returns from `exit` turns a clean shutdown into a crash


**measured** - 2026-08-30

With the landing zone in place (D400), `elfldr` runs its whole startup and then stops on its own
`ud2`. Reading the strings it resolves says exactly what happened, and none of it is a guess:

| what it resolves | what it does with it |
|---|---|
| `sceKernelDlsym` | bootstraps its resolver, through handoff field zero |
| `getpid` | takes the address, adds ten, and that is its syscall entry |
| `exit` | calls it with a status, and places `ud2` after the call |

Between the second and the third it calls **syscall 649** to bring up its runtime linker, gets
nothing (this implements no such call), prints **`Unable to initialize rtld`**, writes the status
through handoff field five, and exits.

So the run is not crashing. It is **shutting down cleanly, for a reason it states**, and then
crashing only because `exit` came back.

### Why `ud2` after a call is worth recognising generally

A compiler emits an undefined instruction after a call it has proved cannot return. It is the
program saying *if you are reading this, the thing I just called broke its contract*. That makes
it one of the few places a guest gives an unambiguous verdict on the emulator rather than on
itself - and this project has been reading it as an ordinary fault.

Every stub here returns, because a stub is a function and functions return. For the great
majority that is right. For the handful that must not - `exit`, `_exit`, `abort`, a thread's
final call - returning does not merely give a wrong answer, it resumes a program that had
finished, at an instruction chosen to be invalid.

### What this does not do yet

Nothing is implemented here. The finding is that **a stub needs a way to be terminal**, and the
shape of that is not obvious enough to assume: ending the guest thread, unwinding to the
worker, and reporting a clean exit are three different behaviours, and the difference between
"the guest exited with status 0" and "the guest crashed" is exactly the sort of thing this
project must not get casually wrong. `elfldr` also exits with a **non-zero** status here, so the
first implementation would be reporting a failure - which is correct, and would look like a
regression in every report that counts faults.

Recorded so the next person meets the question rather than the `ud2`.

