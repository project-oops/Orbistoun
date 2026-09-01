# D435 - libSceUlt mutexes and condition variables, and the wall that turned out to be online


**measured** - 2026-09-01 (user-directed, /loop)

Past the TLS wall (D434), PPSA28061 aborted on `sce::Json::Initializer::initialize failed: 0x7fff0001`,
and following the placeholder led into libSceUlt - the user-level-thread (fibre) library. Built its
synchronisation primitives, in a new `ult` module in the kernel (where `sync` lives): the mutex family
(`_sceUltMutexCreate`/`Lock`/`Unlock`/`TryLock`/`Destroy`) and the condition-variable family
(`_sceUltConditionVariableCreate`/`Signal`/`SignalAll`/`Wait`/`Destroy`), each mapping onto the same
`GuestMutex`/`GuestCond` the pthread and `_Mtx_*` families use - real mutual exclusion, one vendor
library over. A condition variable binds its mutex at creation here (unlike the pairs whose wait is
handed the mutex each time), so a per-cond map remembers the binding. `_sceUltUlthreadCreate` is
answered "created, not scheduled": a cooperative thread does not run until yielded to, and no scheduler
is built, so it records the thread and returns success without running its entry - honest for the state
a title checks at creation, and the scheduler is left named as the next step rather than faked by
running the entry synchronously (which would hang on a worker's first blocking wait).

**But the mutex/condvar/ulthread were the game's *other* threads - the "Save Thread" and kin - not the
JSON initialiser's blocker.** Implementing all of them left PPSA28061 at exactly 60 imports and the same
abort, which is the tell: the initialiser's `0x7fff0001` comes from elsewhere. The complete finding list
names it: `libSceNpCppWebApi::0xa9721c01ca796f63`, an **unnamed** function called once, from the NP
(network-platform) C++ Web API - the online leaderboard client. So PPSA28061's remaining wall is the
online subsystem, exactly what the prior note recorded ("libSceJson2/libSceNpCppWebApi init"). That is a
different kind of gap from a missing local call: the leaderboard needs a network the offline emulator
does not have, and making its init succeed would be a policy decision about faking online success, not a
mechanical implementation. Recorded as such rather than stubbed on my own call. The libSceUlt work
stands regardless - it is correct infrastructure the game uses, and a title that gets past the online
wall will need it.

