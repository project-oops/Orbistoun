# 2026-09-01 - PPSA28061's wall diagnosed: no guest thread pointer (D432)


(diagnosis, no code) Turned the new fault-report bytes on PPSA28061, the furthest title (56 imports).
It faults at image+0x43c4 = `mov rax, fs:[0]`, read of 0x0 - the fs (thread-self) base is zero. The
install mechanism exists and is tested (abi::thread_pointer::install, D061) but was never wired into
the run path because no title had declared a thread-local until this one. So the next subsystem is
clear: allocate a per-thread TLS block (.tdata copied, self-pointer at [TP]), install+read-back the fs
base, on the main thread at entry and on every spawn. Not started - a wrong fs base destabilises every
run, so it wants a clean start with tests, not the end of a long session. The `_sceUltUlthreadCreate`
(libSceUlt user-level threads) calls in the run are a separate subsystem behind it, not this fault.

