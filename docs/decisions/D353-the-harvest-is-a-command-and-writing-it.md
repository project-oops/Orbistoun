# D353 - The harvest is a command, and writing it twice found a bug


**decided** · 2026-08-27

D352 widened the FreeBSD checkout and harvested 903 constants - with a throwaway script that
existed nowhere afterwards. `docs/REFERENCES.md` sets the standard the table then failed:
*"anyone should be able to fetch the same material and follow the same reasoning to the same
table."* Nobody could.

`orbistoun-gen constants <checkout> --revision <commit>` is the fix, and it is not a new
shape - `orbistoun-gen` is already one module per generated table, and `orbistoun-cli
harvest` already does exactly this for names. The source path becomes an argument, which
matters more than it did this morning: the throwaway script hardcoded `<clones>/freebsd-src`,
and everything moved to `<OOPS>` an hour later.

`--revision` is **required**. A table that does not say which revision it came from cannot
be checked against anything, which is most of the reason to harvest rather than remember.

### Writing it twice found a bug in the first one

The command produced **911** constants where the script produced 903. The eight it had been
silently dropping:

```
AT_EACCESS  AT_RESOLVE_BENEATH  FD_RESOLVE_BENEATH  IP_MULTICAST_IF
KERN_PROC_INC_THREAD  NET_RT_IFLISTL  O_RESOLVE_BENEATH  ST_INFO_HW_HPREC
```

Every one has a **comment that runs onto the next line**. The script's regex required the
comment to close on the same line, so it rejected the whole definition rather than the
comment - losing `AT_EACCESS = 0x0100` and `IP_MULTICAST_IF = 9` entirely, with nothing
saying so.

**That is the failure this project keeps meeting from a new direction.** A harvest that
silently drops what it cannot parse produces a table that looks complete, and there is no
way to tell a constant that is absent because it does not exist from one absent because the
extractor tripped. It was found by having two implementations disagree, which is the only
reason anybody looked.

### And the fix had the same shape one level down

Taking those eight meant their comments end mid-sentence. `AT_EACCESS` would have been
described as *"Check access using effective user"* - which stops precisely where it stops
meaning anything, and reads as a whole sentence. An unterminated comment now ends in `...`.
One character, and the difference between a description and a description-shaped fragment.

### What is deliberately not harvested

`#define X (Y | Z)` is skipped rather than evaluated. Working out what it comes to means
reproducing a decision somebody made about how to compose it, and the number is not the
point - where it came from is. Function-like macros are code. Lower-case names are internal.

The headers are a **list, not a directory walk**: every entry is one somebody needed, and a
walk would pull in hundreds nobody has looked at, making the file larger, the provenance
question vaguer, and "why is this constant here" harder to answer.

