# D186 - printf, because the guest was explaining itself into a void


**decided** · 2026-08-21

Two titles aborted during static initialisation after calling one unnamed `libc` function
eight times. The name search reached it from the published-standard word list: **`printf`**,
hash-confirmed.

It had been called seventy-six times across the corpus and discarded. The guest had been
saying why it was giving up the entire time, and the emulator was throwing the message away
and then reporting that the guest stopped for reasons unknown.

Implemented on the renderer `snprintf_s` already had, and refusing the same formats for the
same reason (D183) - a half-rendered diagnostic is worse than none, because it is the text
somebody then reasons from. Output goes to the host's **error** stream: a worker's stdout
carries the JSON protocol its parent is parsing.

### What it said immediately

```
[SCE] scePthreadMutexattrInit(&mutexAttr) returned 0x7fff0001 in
      .\PlatformDependent/PS5/Source/Threads/PlatformMutex.cpp(21) :
```

Four such lines, naming four functions the search could not reach, with the source file and
line they were called from - and quoting `0x7fff0001`, this project's own unimplemented
placeholder, back at us.

**The cheapest oracle in the project turned out to be the guest's own error handling**, and
it cost one ISO C function to read.

