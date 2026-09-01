# 2026-09-01 (/loop) - file error codes now the measured errnos (D439 cont.)


Applied the five file error codes the fs layer had deferred "until the probe answered": open(null)=EFAULT
(0x8002000e, new errno::FAULT=14), open(missing)=ENOENT (0x80020002), read/write/lseek(bad fd)=EBADF
sign-extended (0xffffffff80020009). Repurposed FAILED_DESCRIPTOR from -1 to the errno-derived value; open
now returns its own codes. All five verified equal to hardware via the probe. So the rejects-* error-code
sweep is 000/015/020/040(x5)/090/100 matching hardware; only 060-dlsym remains (handle validation).

SURPRISE/debt: orbistoun-fs unit tests have a test-isolation bug - `posix::tests::a_file_can_be_sent_
straight_into_another_descriptor` passes alone but fails in the suite even with --test-threads=1 (shared
filesystem state across `an_installation`/`exclusively()`), and `socket::tests` has a >60s hang. Both
pre-existing, unrelated to these changes (confirmed: the sendfile test passes in isolation before and
after). Flagged separately. core/kernel tests green.

