# D307 - An import that names data was getting a function


**decided** · 2026-08-26

[D305](#d305---a-plain-name-is-a-nid-nobody-hashed-yet) made the open-toolchain payloads
readable, and reading them turned up something the vendor corpus never had: imports that
name **data**.

```
0x12d447fb770ecf6a  ?  __stderrp  [data]
0x9504b4a951af53ab  ?  __isthreaded  [data]
```

`__stderrp`, `__stdinp`, `__stdoutp`, `optarg`, `environ`, `__isthreaded` - `STT_OBJECT`
symbols reached through `R_X86_64_GLOB_DAT`, two per payload in the small ones.

### Why this is the worst shape of wrong answer

Interception writes an address into a relocation slot, and for code that address is a
thunk - the whole of principle 7, and it works. **For data the same answer is wrong in a
way that looks right.** A guest importing `__stderrp` loads the slot and then dereferences
what it found, so it reads the first bytes of x86 instructions as a `FILE *` and carries
on. Nothing faults, nothing reports, and the run looks like every other run until something
unrelated breaks a long way downstream.

That is exactly the failure principle 3 exists to stop, and the loader had no way to even
*notice* it: `RawImport` carried a name, a hash and an attribution, and nothing about what
the guest wanted in the slot.

### What was done

`st_info` states it outright - `STT_OBJECT` against `STT_FUNC` - so it is read rather than
inferred, and carried through `RawImport`, the survey, the wire record and the report. An
import naming data is marked `[data]` in a listing and counted in the summary:

```
34 imports, 27 unresolved
2 of them name data, not a function - a thunk is the wrong kind of answer there and
orbistoun has no other one yet
```

`Unspecified` is a third value and not a default dressed up: `STT_NOTYPE` is what most of
the vendor corpus carries, and collapsing it into "function" would assert something the
table never said.

### What was deliberately not done *(done in [D323](#d323---data-imports-get-storage-not-a-stub))*

**Answering correctly.** The precedent is already in this repository and it is
`process_argument_block`: *"Zeroed and never written... A guest reading a pointer gets null
and can check it."* The same shape fits here - hand the slot a zeroed, guest-owned block so
`fprintf(stderr, ...)` receives null instead of instruction bytes.

It is a separate decision because it is the first time the HLE layer would own **state**
rather than functions, and because a zeroed `FILE *` makes a guest take its own error path,
which is a behaviour worth choosing on purpose rather than arriving at while fixing
something else.

Until then the honest position is the one now reported: orbistoun knows these are data,
knows a thunk is the wrong kind of answer, and says so rather than quietly giving one.

### The corpus this was supposed to be about turned out not to be the payloads

Marking data imports was written for the homebrew payloads, which have two apiece. Pointing
the same command at the commercial titles:

| module | imports | name data |
|---|---|---|
| PPSA02664-app0 | 583 | 13 |
| PPSA03416-app0 | 583 | 13 |
| PPSA04263-app0 | 1410 | 19 |
| PPSA21564-app0 | 1733 | 20 |
| PPSA25872-app0 | 662 | 13 |
| PPSA28061-app0 | 733 | 5 |
| obSCEne | 35514 | 0 |

**Every title has them, and every one has been receiving a function thunk since the day the
loader was written.** What they are makes it worse:

- `_ZTVSt9bad_alloc`, `_ZTVSt13runtime_error`, `_ZTVNSt8ios_base7failureE`,
  `_ZTVN10__cxxabiv120__si_class_type_infoE` - **C++ vtables**, read by every `throw`,
  every `dynamic_cast`, every virtual dispatch through those classes.
- `_ZNSt5ctypeIcE2idE`, `_ZNSt6locale2id7_Id_cntE`, `_ZSt21_sceLibcClassicLocale` - the
  iostream locale machinery, touched during static initialisation.
- `_Stdout`, `_Stderr` - the stream objects themselves.
- `__stack_chk_guard` - the **stack protector canary**, in `libkernel`, read on entry to
  every function compiled with the protector and compared on the way out.

None of this faults. A thunk address is mapped and readable, so the guest dereferences it,
gets deterministic instruction bytes, and proceeds - which is why weeks of runs never
pointed at it. The canary is the clearest illustration: read the same wrong bytes twice and
the comparison passes, so the check silently stops checking anything.

This is not a claim that it explains any particular wall. It is a claim that a whole class
of guest state has been quietly wrong the entire time and nothing in the project could see
it, which is worth more than a diagnosis. **obSCEne having zero is the control** - a
freestanding probe imports no C++ runtime, which is exactly why it never surfaced there.

### Where the vocabulary lives

`orbistoun-proto` declares its own `ImportKind` instead of re-exporting the parser's. That
crate takes serde and nothing else on purpose, and a wire type reaching into a parser for
its vocabulary is how that stays true only until somebody is in a hurry. The conversion is
three lines at the service boundary, which is the right place for it.


