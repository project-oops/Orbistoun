# 2026-09-01 (/loop) - Windows fs-base backstop; PPSA28061 FURTHER to the JSON/ULT wall


(worker) tls_backstop module: remembers the guest thread pointer per thread, and the vectored fault
handler restores it and retries on an access violation only when the base has reverted to zero (D434).
This is the Windows path for guest TLS that D433 identified as needed. PPSA28061 FURTHER: 56->60 imports,
fs:[0] wall gone; now aborts gracefully on `sce::Json::Initializer::initialize failed: 0x7fff0001` -
`_sceUltMutexCreate` (libSceUlt) returned the placeholder, the JSON init read it as failure. Same shape
the _Mtx_* family fixed. No regression on the other two titles. Next: implement the libSceUlt mutex
family onto sync.rs. tls_backstop unit-tested (the no-pointer and remember paths; the restore needs a
real fault to observe).

