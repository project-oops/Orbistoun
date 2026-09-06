/* What a published C library does, recorded so orbistoun can be diffed against it.
 *
 * # Why this exists
 *
 * With no console available, a reference implementation of the same published interface is
 * the only live oracle: it is unboundedly parallel where hardware is one machine, and it
 * answers questions no amount of reading a standard settles - what a function leaves in an
 * out-parameter, what it does with input the standard calls unspecified, whether it writes
 * four bytes or eight.
 *
 * # The case list lives here, and only here
 *
 * A differential where each side carries its own copy of the cases is a differential that
 * silently stops comparing the same thing. So this emits the **inputs it used** alongside
 * the results, and the checker rebuilds the call from those. The two sides cannot drift,
 * because there is only one list.
 *
 * # What is recorded, and why the out-parameter matters most
 *
 * Return value, errno, and the out-parameter. The third is the one that bites: the failures
 * this project has actually had are a semaphore handle written eight bytes wide (D210) and
 * an attribute getter clobbering the caller's loop counter (D272). A return value that
 * matches while the wrong bytes land next door is exactly the bug a return-value-only
 * differential misses.
 *
 * Build and run it wherever the reference library lives:
 *
 *     cc -O0 -Wall -Wextra -o reference reference.c && ./reference
 */

#include <ctype.h>
#include <errno.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <string.h>
#include <time.h>
#include <wchar.h>

/* The library this was run against, printed so a record says what produced it. */
static void emit_provenance(void) {
#ifdef __GLIBC__
    printf("REF|library|glibc|%d.%d\n", __GLIBC__, __GLIBC_MINOR__);
#elif defined(__FreeBSD__)
    printf("REF|library|freebsd|%d\n", __FreeBSD__);
#else
    printf("REF|library|unknown|0\n");
#endif
}

/* Prints a text field, escaping anything that would not survive the record format.
 *
 * **A byte is not a character, and the interesting subjects are bytes.** `strcmp` compares as
 * `unsigned char` by definition, so `"\x80"` against `"a"` is *positive* - and an
 * implementation using a signed char answers backwards. Writing that byte raw produced a
 * record file that was no longer text, so the field carries `\xNN` and the checker decodes it.
 * The pipe and the backslash are escaped for the same reason: the format has to survive its
 * own content. */
static void emit_text(const char *text) {
    for (const unsigned char *at = (const unsigned char *)text; *at; at++) {
        if (*at == '\\' || *at == '|' || *at < 0x20 || *at > 0x7e) {
            printf("\\x%02x", *at);
        } else {
            putchar((int)*at);
        }
    }
}

/* One string-to-number conversion, with the end pointer the caller gets back.
 *
 * The end pointer is reported as an **offset into the subject**, not as an address: an
 * address is a fact about one process and an offset is a fact about the function. */
static void convert(const char *id, const char *fn, const char *subject, int base) {
    char *end = NULL;
    unsigned long long value = 0;
    long long signed_value = 0;
    errno = 0;
    if (strcmp(fn, "strtoul") == 0) {
        value = (unsigned long)strtoul(subject, &end, base);
    } else if (strcmp(fn, "strtoull") == 0) {
        value = strtoull(subject, &end, base);
    } else if (strcmp(fn, "strtol") == 0) {
        signed_value = strtol(subject, &end, base);
        value = (unsigned long long)signed_value;
    } else {
        fprintf(stderr, "unknown conversion %s\n", fn);
        return;
    }
    int failed = errno;
    printf("REF|case|%s|%s|s:", id, fn);
    emit_text(subject);
    printf("|i:%d\n", base);
    printf("REF|ret|%s|0x%llx\n", id, value);
    printf("REF|errno|%s|0x%x\n", id, failed);
    printf("REF|out|%s|end_offset|0x%zx\n", id, (size_t)(end - subject));
}

/* One search over a string, whose whole answer is a position or a count. */
static void search(const char *id, const char *fn, const char *subject, const char *set) {
    size_t answer = 0;
    errno = 0;
    if (strcmp(fn, "strspn") == 0) {
        answer = strspn(subject, set);
    } else if (strcmp(fn, "strcspn") == 0) {
        answer = strcspn(subject, set);
    } else {
        fprintf(stderr, "unknown search %s\n", fn);
        return;
    }
    printf("REF|case|%s|%s|s:", id, fn);
    emit_text(subject);
    printf("|s:");
    emit_text(set);
    printf("\n");
    printf("REF|ret|%s|0x%zx\n", id, answer);
    printf("REF|errno|%s|0x%x\n", id, errno);
}

/* `snprintf` into a buffer too small for the answer.
 *
 * Two things at once, and both are easy to get wrong: the return is the length the output
 * **would** have needed, not what fit, and the buffer must still be terminated. A shim that
 * returns the truncated length passes every test that only checks the string. */
static void truncating_format(const char *id, size_t room, int value) {
    char buffer[64];
    memset(buffer, '@', sizeof buffer);
    errno = 0;
    int needed = snprintf(buffer, room, "%d", value);
    int failed = errno;
    printf("REF|case|%s|snprintf|u:%zu|i:%d\n", id, room, value);
    printf("REF|ret|%s|0x%x\n", id, (unsigned)needed);
    printf("REF|errno|%s|0x%x\n", id, failed);
    /* The whole window, including what was left untouched past the terminator - which is
     * where an overrun shows up. */
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 8; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* `vsnprintf`, driven through a real `va_list`.
 *
 * # What this is actually testing
 *
 * Not the formatter - `sprintf`'s cases cover that. This tests the **argument source**. On this
 * architecture a `va_list` is not a pointer walking a stack: the System V psABI spills the first
 * six integer arguments into a register save area and puts the rest on the stack, and fetching
 * one reads from the save area until `gp_offset` reaches the end and from the overflow area
 * afterwards.
 *
 * **The crossover is the case worth having.** An implementation that only ever reads the save
 * area renders the first six arguments correctly and the seventh from whatever follows it, which
 * every case with six or fewer arguments passes over in silence. So the sweep here is over the
 * *number* of arguments rather than over the conversions.
 *
 * The values are emitted from a `va_copy`, so the record carries exactly what was rendered
 * rather than a second list the caller wrote out by hand and could get wrong. */
static void format_va(const char *id, size_t room, const char *format, int count, ...) {
    char buffer[64];
    va_list ap;
    va_list emit;
    memset(buffer, '@', sizeof buffer);
    va_start(ap, count);
    va_copy(emit, ap);
    int wrote = vsnprintf(buffer, room, format, ap);
    va_end(ap);

    printf("REF|case|%s|vsnprintf|u:%zu|s:", id, room);
    emit_text(format);
    for (int i = 0; i < count; i++) {
        printf("|i:%d", va_arg(emit, int));
    }
    va_end(emit);
    printf("\n");
    printf("REF|ret|%s|0x%x\n", id, (unsigned)wrote);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 20; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* `sprintf` with one argument, across the conversions and the flags.
 *
 * # Why the format is an input rather than a family of helpers
 *
 * The interesting failures in a formatter are not in any one conversion, they are in the
 * *specifier parser* - a width that is read as a precision, a `0` flag that is dropped for a
 * left-justified field, a `%%` that consumes the next argument. Passing the format string as
 * a case input covers all of that with one helper and makes each case say what it tests.
 *
 * **Unbounded on purpose.** `sprintf` has no size argument, so nothing can stop an overrun -
 * the buffer here is far larger than any of these render, and the poison past the end is
 * recorded so a case that did overrun would show it rather than corrupting the next case.
 *
 * The return is the length, which for `sprintf` is what it wrote rather than what it would
 * have written - the one place it differs from `snprintf` and worth pinning separately. */
static void format_int(const char *id, const char *format, long long value) {
    char buffer[64];
    memset(buffer, '@', sizeof buffer);
    errno = 0;
    int wrote = sprintf(buffer, format, (int)value);
    int failed = errno;
    printf("REF|case|%s|sprintf|s:", id);
    emit_text(format);
    printf("|i:%lld\n", value);
    printf("REF|ret|%s|0x%x\n", id, (unsigned)wrote);
    printf("REF|errno|%s|0x%x\n", id, failed);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 20; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* `sprintf` with a string argument - `%s` and its width and precision, which truncate a
 * string where the integer flags pad a number. */
static void format_str(const char *id, const char *format, const char *subject) {
    char buffer[64];
    memset(buffer, '@', sizeof buffer);
    errno = 0;
    int wrote = sprintf(buffer, format, subject);
    int failed = errno;
    printf("REF|case|%s|sprintf|s:", id);
    emit_text(format);
    printf("|s:");
    emit_text(subject);
    printf("\n");
    printf("REF|ret|%s|0x%x\n", id, (unsigned)wrote);
    printf("REF|errno|%s|0x%x\n", id, failed);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 20; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* A comparison, whose contract is the **sign** and not the value.
 *
 * ISO C says `strcmp` answers a value greater than, equal to or less than zero. It does not
 * say which one, and implementations differ: glibc returns the byte difference, others return
 * exactly the sign. So the sign is what gets compared and the raw value is recorded beside it
 * as information - asserting the magnitude would report a conforming implementation as broken,
 * which is a false alarm the differential must not manufacture. */
static void compare(const char *id, const char *fn, const char *a, const char *b, size_t n) {
    int answer = 0;
    int bounded = 0;
    if (strcmp(fn, "strcmp") == 0) {
        answer = strcmp(a, b);
    } else if (strcmp(fn, "strcasecmp") == 0) {
        answer = strcasecmp(a, b);
    } else if (strcmp(fn, "strncmp") == 0) {
        answer = strncmp(a, b, n);
        bounded = 1;
    } else if (strcmp(fn, "strncasecmp") == 0) {
        answer = strncasecmp(a, b, n);
        bounded = 1;
    } else if (strcmp(fn, "memcmp") == 0) {
        answer = memcmp(a, b, n);
        bounded = 1;
    } else {
        fprintf(stderr, "unknown comparison %s\n", fn);
        return;
    }
    if (bounded) {
        printf("REF|case|%s|%s|s:", id, fn);
        emit_text(a);
        printf("|s:");
        emit_text(b);
        printf("|u:%zu\n", n);
    } else {
        printf("REF|case|%s|%s|s:", id, fn);
        emit_text(a);
        printf("|s:");
        emit_text(b);
        printf("\n");
    }
    /* **No return value is recorded**, deliberately. The magnitude is not part of the
     * contract, so recording it would invite an assertion that a conforming implementation
     * fails. The sign is the whole finding and it goes in its own field. */
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|sign|%s\n", id,
           answer > 0 ? "positive" : (answer < 0 ? "negative" : "zero"));
}

/* A search returning a pointer, reported as found-or-not plus an offset.
 *
 * An address is a fact about one process; an offset into the subject is a fact about the
 * function, which is the same reasoning the end pointer already follows. */
static void find(const char *id, const char *fn, const char *subject, int needle, size_t n) {
    const char *at = NULL;
    int bounded = 0;
    if (strcmp(fn, "strchr") == 0) {
        at = strchr(subject, needle);
    } else if (strcmp(fn, "strrchr") == 0) {
        at = strrchr(subject, needle);
    } else if (strcmp(fn, "memchr") == 0) {
        at = (const char *)memchr(subject, needle, n);
        bounded = 1;
    } else {
        fprintf(stderr, "unknown search %s\n", fn);
        return;
    }
    if (bounded) {
        printf("REF|case|%s|%s|s:", id, fn);
        emit_text(subject);
        printf("|i:%d|u:%zu\n", needle, n);
    } else {
        printf("REF|case|%s|%s|s:", id, fn);
        emit_text(subject);
        printf("|i:%d\n", needle);
    }
    printf("REF|ret|%s|0x%x\n", id, at ? 1 : 0);
    if (at) {
        printf("REF|out|%s|offset|0x%zx\n", id, (size_t)(at - subject));
    }
}

/* `strstr`, the same shape with a string needle. */
static void find_substring(const char *id, const char *subject, const char *needle) {
    const char *at = strstr(subject, needle);
    printf("REF|case|%s|strstr|s:", id);
    emit_text(subject);
    printf("|s:");
    emit_text(needle);
    printf("\n");
    printf("REF|ret|%s|0x%x\n", id, at ? 1 : 0);
    if (at) {
        printf("REF|out|%s|offset|0x%zx\n", id, (size_t)(at - subject));
    }
}

/* `strlen`, and `memcpy` into the same poisoned window the bounded copies use.
 *
 * Both are here late and neither is exotic - they are the fifth and sixth most-called
 * functions in this project's whole recorded corpus, at ninety and ninety-eight thousand
 * calls, and between them they had no case at all. A differential that covers the rare
 * functions and not the universal ones is measuring the wrong end. */
static void length_of(const char *id, const char *subject) {
    size_t answer = strlen(subject);
    printf("REF|case|%s|strlen|s:", id);
    emit_text(subject);
    printf("\n");
    printf("REF|ret|%s|0x%zx\n", id, answer);
}

/* An absolute value. `INT_MIN` is deliberately absent: negating it is undefined, so a case
 * for it would be recording one implementation's choice as though it were the contract. */
static void magnitude(const char *id, const char *fn, long long value) {
    long long answer = 0;
    if (strcmp(fn, "abs") == 0) {
        answer = abs((int)value);
    } else if (strcmp(fn, "labs") == 0) {
        answer = labs((long)value);
    } else if (strcmp(fn, "llabs") == 0) {
        answer = llabs(value);
    } else {
        fprintf(stderr, "unknown magnitude %s\n", fn);
        return;
    }
    printf("REF|case|%s|%s|i:%lld\n", id, fn, value);
    printf("REF|ret|%s|0x%llx\n", id, (unsigned long long)answer);
}

/* `atoi` and its wider spellings, which are `strtol` with the error reporting removed.
 *
 * That is the whole reason to test them separately: no `endptr`, no `errno`, and the
 * standard says the behaviour is undefined if the value does not fit - so the inputs stay
 * inside the range and what is checked is the parsing, not the overflow. */
static void to_integer(const char *id, const char *fn, const char *subject) {
    long long answer = 0;
    errno = 0;
    if (strcmp(fn, "atoi") == 0) {
        answer = atoi(subject);
    } else if (strcmp(fn, "atol") == 0) {
        answer = atol(subject);
    } else if (strcmp(fn, "atoll") == 0) {
        answer = atoll(subject);
    } else {
        fprintf(stderr, "unknown conversion %s\n", fn);
        return;
    }
    printf("REF|case|%s|%s|s:", id, fn);
    emit_text(subject);
    printf("\n");
    printf("REF|ret|%s|0x%llx\n", id, (unsigned long long)answer);
    printf("REF|errno|%s|0x%x\n", id, errno);
}

/* `strpbrk`, the same pointer shape with a *set* rather than a substring.
 *
 * Sits next to `strcspn`, which answers the same question as a length, and the pair is the
 * point: an implementation that gets one right and the other wrong at the not-found end is
 * common, because `strcspn` answers the length of the string and `strpbrk` answers null. */
static void find_any(const char *id, const char *subject, const char *set) {
    const char *at = strpbrk(subject, set);
    printf("REF|case|%s|strpbrk|s:", id);
    emit_text(subject);
    printf("|s:");
    emit_text(set);
    printf("\n");
    printf("REF|ret|%s|0x%x\n", id, at ? 1 : 0);
    if (at) {
        printf("REF|out|%s|offset|0x%zx\n", id, (size_t)(at - subject));
    }
}

/* `strnlen`, which is `strlen` that promises not to read past a bound.
 *
 * The bound is the whole function: given an unterminated buffer it must answer the bound and
 * not run off the end, and given a short string it must answer the string's length and not the
 * bound. An implementation that just calls `strlen` passes the second and reads out of bounds
 * on the first, which no return-value comparison can see - so the unterminated case is fed a
 * deliberately short bound. */
static void bounded_length(const char *id, const char *subject, size_t n) {
    size_t answer = strnlen(subject, n);
    printf("REF|case|%s|strnlen|s:", id);
    emit_text(subject);
    printf("|u:%zu\n", n);
    printf("REF|ret|%s|0x%zx\n", id, answer);
}

/* `strlcpy`, whose **return is the contract**.
 *
 * It answers the length of the source - what it *would* have needed - not what it copied, so
 * `returned >= size` is the caller's truncation test. An implementation that answers the copied
 * length is wrong in exactly the way that makes every caller's truncation check silently pass.
 * Same shape of bug as `snprintf`'s return, and worth the same case.
 *
 * Both halves are recorded, because it also always terminates where `strncpy` does not.
 *
 * glibc grew `strlcpy` in 2.38; on an older library this case simply will not build, which is
 * the honest failure rather than a silently skipped comparison. */
static void bounded_string_copy(const char *id, const char *initial, const char *src, size_t n) {
    char buffer[16];
    memset(buffer, '@', sizeof buffer);
    strcpy(buffer, initial);
    size_t answer = strlcpy(buffer, src, n);
    printf("REF|case|%s|strlcpy|s:", id);
    emit_text(initial);
    printf("|s:");
    emit_text(src);
    printf("|u:%zu\n", n);
    printf("REF|ret|%s|0x%zx\n", id, answer);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 12; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* A bounded copy into a poisoned window, so what it writes *and does not write* both show.
 *
 * `strncpy` is the one everybody gets wrong in one of two directions: it pads the whole
 * remainder with NULs when the source is short, and it does **not** terminate at all when the
 * source is exactly `n` or longer. Both are visible only by looking at the bytes. */
static void bounded_copy(const char *id, const char *fn, const char *initial, const char *src,
                         size_t n) {
    char buffer[16];
    memset(buffer, '@', sizeof buffer);
    strcpy(buffer, initial);
    if (strcmp(fn, "strncpy") == 0) {
        strncpy(buffer, src, n);
    } else if (strcmp(fn, "strncat") == 0) {
        strncat(buffer, src, n);
    } else if (strcmp(fn, "memcpy") == 0) {
        /* Copies exactly `n` bytes and knows nothing about terminators, which is the whole
         * difference from `strncpy` beside it and is visible only in the window. */
        memcpy(buffer, src, n);
    } else {
        fprintf(stderr, "unknown copy %s\n", fn);
        return;
    }
    printf("REF|case|%s|%s|s:", id, fn);
    emit_text(initial);
    printf("|s:");
    emit_text(src);
    printf("|u:%zu\n", n);
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 12; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* A character class, swept over sixty-four code points at once.
 *
 * # Why a bitmap rather than a case per character
 *
 * These eleven predicates are total functions of one byte, so the only honest coverage is
 * every byte - and a case per character would be 1,408 of them, swamping every other case in
 * the file for functions that are among the simplest in it. One `uint64_t` per half holds the
 * complete answer for that half, so `0..127` is twenty-two cases and nothing is sampled.
 *
 * A mismatch is still diagnosable: the case id names the function and the half, and the XOR of
 * the two bitmaps names the code points. That is a decode step a per-character case would not
 * need, and it is worth it at sixty-four times the coverage.
 *
 * # Why `0..127` and not `0..255`
 *
 * Above 127 the answer is a property of the locale, and the C locale is the only one either
 * side is in. The console's table for the high half is a separate question - obSCEne's
 * `035-libc/getpctype` is the thing that would settle it - and pinning glibc's answer here
 * would record an assumption as though it were the contract.
 *
 * The argument is normalised to `int` through `unsigned char`, which is what the standard
 * requires of every caller and what a shim that takes a `char` gets wrong for a high byte. */
static void classify_half(const char *id, const char *fn, unsigned half) {
    uint64_t bits = 0;
    for (unsigned i = 0; i < 64; i++) {
        int c = (int)(unsigned char)(half * 64u + i);
        int answer = 0;
        if (strcmp(fn, "isalnum") == 0) {
            answer = isalnum(c);
        } else if (strcmp(fn, "isalpha") == 0) {
            answer = isalpha(c);
        } else if (strcmp(fn, "iscntrl") == 0) {
            answer = iscntrl(c);
        } else if (strcmp(fn, "isdigit") == 0) {
            answer = isdigit(c);
        } else if (strcmp(fn, "isgraph") == 0) {
            answer = isgraph(c);
        } else if (strcmp(fn, "islower") == 0) {
            answer = islower(c);
        } else if (strcmp(fn, "isprint") == 0) {
            answer = isprint(c);
        } else if (strcmp(fn, "ispunct") == 0) {
            answer = ispunct(c);
        } else if (strcmp(fn, "isspace") == 0) {
            answer = isspace(c);
        } else if (strcmp(fn, "isupper") == 0) {
            answer = isupper(c);
        } else if (strcmp(fn, "isxdigit") == 0) {
            answer = isxdigit(c);
        } else {
            fprintf(stderr, "unknown class %s\n", fn);
            return;
        }
        /* **The standard says non-zero, not one.** glibc answers a mask from its table, so
         * comparing the raw returns would compare an implementation detail. The contract is
         * the classification, and that is what this records. */
        if (answer != 0) {
            bits |= (uint64_t)1 << i;
        }
    }
    printf("REF|case|%s|%s|u:%u\n", id, fn, half);
    printf("REF|ret|%s|0x%llx\n", id, (unsigned long long)bits);
}

/* `tolower`/`toupper` at one code point.
 *
 * Per-character rather than swept, because the answer is a byte and not a bit - and because
 * the interesting inputs are the boundaries, where an implementation that adds 0x20 without
 * checking the range gives a plausible wrong answer. `@`, `[`, backtick and `{` are the four
 * characters either side of the two alphabets, and they are where that bug lives. */
static void map_case(const char *id, const char *fn, int input) {
    int answer = 0;
    errno = 0;
    if (strcmp(fn, "tolower") == 0) {
        answer = tolower(input);
    } else if (strcmp(fn, "toupper") == 0) {
        answer = toupper(input);
    } else {
        fprintf(stderr, "unknown mapping %s\n", fn);
        return;
    }
    printf("REF|case|%s|%s|i:%d\n", id, fn, input);
    printf("REF|ret|%s|0x%x\n", id, (unsigned)answer);
    printf("REF|errno|%s|0x%x\n", id, errno);
}

/* A string-to-float conversion, with the end pointer.
 *
 * The value is reported as its **exact bit pattern**, because a decimal rendering would hide
 * exactly the last-place differences worth catching. */
static void convert_float(const char *id, const char *subject) {
    char *end = NULL;
    errno = 0;
    double value = strtod(subject, &end);
    int failed = errno;
    uint64_t bits = 0;
    memcpy(&bits, &value, sizeof bits);
    printf("REF|case|%s|strtod|s:", id);
    emit_text(subject);
    printf("\n");
    printf("REF|ret|%s|0x%llx\n", id, (unsigned long long)bits);
    printf("REF|errno|%s|0x%x\n", id, failed);
    printf("REF|out|%s|end_offset|0x%zx\n", id, (size_t)(end - subject));
}

/* `strtof`, the same shape as `strtod` at single precision.
 *
 * Worth its own cases rather than being assumed to follow the double: **an implementation that
 * parses in double and narrows is not the same function.** Converting the decimal to the
 * nearest double and then to the nearest float rounds twice, and a value sitting near a float
 * halfway lands on the wrong side of it - a last-bit difference no case on `strtod` can see.
 *
 * Reported as the exact bit pattern, for the reason `strtod`'s cases give: a decimal rendering
 * would hide precisely the difference worth catching. */
static void convert_float32(const char *id, const char *subject) {
    char *end = NULL;
    errno = 0;
    float value = strtof(subject, &end);
    int failed = errno;
    uint32_t bits = 0;
    memcpy(&bits, &value, sizeof bits);
    printf("REF|case|%s|strtof|s:", id);
    emit_text(subject);
    printf("\n");
    printf("REF|ret|%s|0x%lx\n", id, (unsigned long)bits);
    printf("REF|errno|%s|0x%x\n", id, failed);
    printf("REF|out|%s|end_offset|0x%zx\n", id, (size_t)(end - subject));
}

/* Prints a raw byte blob, which a text field cannot carry.
 *
 * Element data contains NUL bytes - a four-byte `5` is `05 00 00 00` - so it cannot ride in a
 * NUL-terminated text field at all. Its own type, plain hex, no escaping to get wrong. */
static void emit_bytes(const void *data, size_t len) {
    const unsigned char *at = (const unsigned char *)data;
    printf("b:");
    for (size_t i = 0; i < len; i++) {
        printf("%02x", at[i]);
    }
}

/* The wide-character family, whose subjects ride in `b:` rather than `s:`.
 *
 * A wide string is element data, not text: `L'A'` is `41 00 00 00`, so a NUL-terminated text
 * field would stop *inside* the first element. `b:` is the type that exists for exactly this,
 * and carrying them needed no format change at all - which is what D529 found after fifteen
 * ticks of a recorded blocker that was never real.
 *
 * Both sides agree a wide character is four bytes: glibc here, and orbistoun says so where it
 * implements them. So the record carries the array verbatim and **nothing converts an
 * encoding** - a conversion in the record would mean testing the conversion instead of the
 * function.
 *
 * Every array includes its terminator, because that is what these functions read. */
static void wide_length(const char *id, const wchar_t *s, size_t elements) {
    printf("REF|case|%s|wcslen|", id);
    emit_bytes(s, elements * sizeof(wchar_t));
    printf("\n");
    printf("REF|ret|%s|0x%zx\n", id, wcslen(s));
}

/* As `compare`: the sign is the contract, the magnitude is not. */
static void wide_compare(const char *id, const wchar_t *a, size_t an, const wchar_t *b,
                         size_t bn) {
    int answer = wcscmp(a, b);
    printf("REF|case|%s|wcscmp|", id);
    emit_bytes(a, an * sizeof(wchar_t));
    printf("|");
    emit_bytes(b, bn * sizeof(wchar_t));
    printf("\n");
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|sign|%s\n", id,
           answer > 0 ? "positive" : (answer < 0 ? "negative" : "zero"));
}

/* As `find`: found-or-not plus an offset, because an address is a fact about one process. */
static void wide_find_last(const char *id, const wchar_t *s, size_t elements, wchar_t needle) {
    const wchar_t *at = wcsrchr(s, needle);
    printf("REF|case|%s|wcsrchr|", id);
    emit_bytes(s, elements * sizeof(wchar_t));
    printf("|u:%lu\n", (unsigned long)needle);
    printf("REF|ret|%s|0x%x\n", id, at ? 1 : 0);
    if (at) {
        printf("REF|out|%s|offset|0x%zx\n", id, (size_t)(at - s));
    }
}

/* `wcsncpy`, whose two halves are the whole point: it pads with terminators when the source is
 * short and does **not** terminate when it is not. The destination is poisoned so both are
 * visible in the recorded window rather than inferred from a length. */
static void wide_copy(const char *id, const wchar_t *src, size_t elements, size_t n) {
    wchar_t destination[8];
    unsigned char *raw = (unsigned char *)destination;
    memset(destination, 0x40, sizeof destination);
    wcsncpy(destination, src, n);
    printf("REF|case|%s|wcsncpy|", id);
    emit_bytes(src, elements * sizeof(wchar_t));
    printf("|u:%zu\n", n);
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < sizeof destination; i++) {
        printf("%02x", raw[i]);
    }
    printf("\n");
}

/* The comparison the sorting cases use, named in the record so both sides use the same one.
 *
 * **The two sides implement this separately**, which is the one place the differential's
 * "there is only one case list" property does not reach: a comparator is code, and the
 * reference cannot hand its own machine code to the checker. So the case *names* the
 * semantic - `int32-asc` - and each side supplies that semantic. A name the checker does not
 * know is reported rather than guessed at. */
static int compare_int32_asc(const void *a, const void *b) {
    int32_t x = 0;
    int32_t y = 0;
    memcpy(&x, a, sizeof x);
    memcpy(&y, b, sizeof y);
    return (x > y) - (x < y);
}

/* `qsort` over four-byte integers, recording the array before and after. */
static void sort_int32(const char *id, const int32_t *values, size_t count) {
    int32_t work[16];
    if (count > sizeof work / sizeof work[0]) {
        fprintf(stderr, "too many values for %s\n", id);
        return;
    }
    memcpy(work, values, count * sizeof work[0]);
    qsort(work, count, sizeof work[0], compare_int32_asc);

    printf("REF|case|%s|qsort|", id);
    emit_bytes(values, count * sizeof work[0]);
    printf("|u:4|s:int32-asc\n");
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|array|", id);
    for (size_t i = 0; i < count * sizeof work[0]; i++) {
        printf("%02x", ((const unsigned char *)work)[i]);
    }
    printf("\n");
}

/* `bsearch` over a sorted array of four-byte integers. */
static void search_int32(const char *id, const int32_t *sorted, size_t count, int32_t key) {
    const void *at = bsearch(&key, sorted, count, sizeof(int32_t), compare_int32_asc);
    printf("REF|case|%s|bsearch|", id);
    emit_bytes(sorted, count * sizeof(int32_t));
    printf("|u:4|s:int32-asc|i:%d\n", key);
    printf("REF|ret|%s|0x%x\n", id, at ? 1 : 0);
    if (at) {
        printf("REF|out|%s|offset|0x%zx\n", id,
               (size_t)((const unsigned char *)at - (const unsigned char *)sorted));
    }
}

/* `strtok` over a whole sequence, because one call cannot express it.
 *
 * Two things make this different from every other case here.
 *
 * **The answer depends on state carried between calls.** The first call takes the subject and
 * each later one passes null to mean "carry on", so a single case would only ever exercise the
 * first token. The record numbers the steps - `id#0`, `id#1` - and the checker runs them in
 * order against one buffer.
 *
 * **And it mutates its subject.** `strtok` writes a NUL over each delimiter it consumes, so
 * half the contract is invisible in the return values: a shim that answers the right tokens
 * without writing the terminators is wrong in a way only the bytes show. The buffer is
 * recorded after the last step. */
/* `strtok_r`, the same sequence with the place kept by the caller.
 *
 * The reentrant form differs from `strtok` in exactly one visible way, and it is the one worth
 * pinning: **the position lives in the caller's `saveptr`, not in a static**. A shim that
 * implements it by delegating to `strtok` answers every one of these cases correctly and is
 * still wrong - two interleaved sequences would then share a place - so the cases here are the
 * same shape as `strtok`'s deliberately, and what separates the two implementations is that
 * this one is handed storage and must use it.
 *
 * POSIX.1-2008. The subject is mutated exactly as `strtok` mutates it, so the buffer is
 * recorded for the same reason. */
/* Two `strtok_r` walks, interleaved, which is the only thing that tells it from `strtok`.
 *
 * `strtok_r` differs from `strtok` in exactly one way: the place it keeps is the caller's, not
 * a static. Every case that walks one string to its end passes for an implementation that
 * ignores the save pointer and delegates to `strtok` - **the sequences have to overlap** for
 * the difference to appear at all.
 *
 * So: start A, start B, continue A, continue B. A delegating implementation loses A's place
 * when B starts, and answers B's second token where A's belongs.
 *
 * The stream index rides as an ordinary `u:` argument - no format change was needed for this
 * either (D529, D531). Each stream owns a buffer, and both are recorded at the end, because
 * `strtok_r` mutates its subject and a shim that answers the right tokens without writing the
 * terminators is wrong in a way only the bytes show. */
/* `strftime`, whose surface is a specifier parser - the shape that hid two bugs in `sprintf`.
 *
 * **`struct tm` is the citable part and the only part that crosses.** ISO C fixes the names and
 * the order of the nine `int` fields, and both sides agree on them: glibc here, and orbistoun
 * where it reads them. The fields past the ninth - `tm_gmtoff`, `tm_zone` - are *not* recorded,
 * because orbistoun does not read them and `%Z`/`%z` are among the conversions it refuses.
 *
 * The nine ride as a `b:` blob for the same reason a wide string does: they are element data,
 * and a text field would stop inside the first one.
 *
 * **The buffer is recorded only when the call succeeded.** ISO C leaves the contents
 * unspecified when the result does not fit, so comparing them there would be comparing
 * something neither implementation promises - a false alarm the differential must not
 * manufacture. The return alone is the contract in that case. */
/* `strdup` and `strndup`, whose answer is an address and whose *contract* is the bytes behind
 * it.
 *
 * # Why the pointer is not compared and the contents are
 *
 * An address is a fact about one process - the same reasoning the search family already
 * follows, one step further. What crosses is found-or-not, and then the copy itself **including
 * its terminator**, because the terminator is the half of `strndup` that is easy to lose.
 *
 * # The case that distinguishes them
 *
 * `strndup(s, n)` copies at most `n` bytes and then **always terminates**, so it may write
 * `n + 1`. An implementation shaped like `strncpy` - which pads to `n` and does not terminate
 * when the source is longer - answers an unterminated buffer, and every case where the source
 * is shorter than `n` passes for it. So the source must be *longer* than `n` for the
 * difference to appear at all (D535). */
static void duplicate(const char *id, const char *fn, const char *subject, size_t n) {
    char *copy = NULL;
    int bounded = 0;

    if (strcmp(fn, "strdup") == 0) {
        copy = strdup(subject);
    } else if (strcmp(fn, "strndup") == 0) {
        copy = strndup(subject, n);
        bounded = 1;
    } else {
        fprintf(stderr, "unknown duplication %s\n", fn);
        return;
    }

    printf("REF|case|%s|%s|s:", id, fn);
    emit_text(subject);
    if (bounded) {
        printf("|u:%zu", n);
    }
    printf("\n");
    printf("REF|ret|%s|0x%x\n", id, copy ? 1 : 0);
    if (copy) {
        size_t len = strlen(copy);
        printf("REF|out|%s|copy|", id);
        /* Through the terminator, which is the byte the bounded form can lose. */
        for (size_t i = 0; i <= len; i++) {
            printf("%02x", (unsigned char)copy[i]);
        }
        printf("\n");
    }
    free(copy);
}

/* The single-precision half of the same split, and the pair that pins the rounding rule.
 *
 * `roundf` is half-away-from-zero; `nearbyintf` is the current rounding mode, which is
 * half-to-**even** by default. They disagree at exactly the halfway values, so having both in
 * the corpus means neither can be implemented as the other without a case saying so - which is
 * the break D533 used, kept as coverage rather than only as a demonstration.
 *
 * # NaN is here only where the answer is specified
 *
 * A NaN payload is **not** fixed by the standard: `sqrtf(NaN)` may answer any quiet NaN, and
 * comparing bit patterns there would test which one this glibc happens to produce. `fabsf` is
 * different - it clears the sign bit and the payload survives - so that is the one NaN case,
 * and the transcendental-style trap is avoided for the same reason it was in D533. */
static void math_float(const char *id, const char *fn, float x) {
    float answer = 0.0f;
    uint32_t in = 0;
    uint32_t out = 0;

    if (strcmp(fn, "sqrtf") == 0) {
        answer = sqrtf(x);
    } else if (strcmp(fn, "fabsf") == 0) {
        answer = fabsf(x);
    } else if (strcmp(fn, "ceilf") == 0) {
        answer = ceilf(x);
    } else if (strcmp(fn, "floorf") == 0) {
        answer = floorf(x);
    } else if (strcmp(fn, "truncf") == 0) {
        answer = truncf(x);
    } else if (strcmp(fn, "roundf") == 0) {
        answer = roundf(x);
    } else if (strcmp(fn, "nearbyintf") == 0) {
        answer = nearbyintf(x);
    } else {
        fprintf(stderr, "unknown float function %s\n", fn);
        return;
    }

    memcpy(&in, &x, sizeof in);
    memcpy(&out, &answer, sizeof out);
    /* The bits go where orbistoun reads them: a single-precision argument is the low half of
     * the float register, and the answer comes back zero-extended into the same width. */
    printf("REF|case|%s|%s|u:%lu\n", id, fn, (unsigned long)in);
    printf("REF|ret|%s|0x%lx\n", id, (unsigned long)out);
}

/* The exactly-specified half of libm, whose answers are bit patterns.
 *
 * # Why only half of it
 *
 * Around fifty math functions are implemented here and none had ever been compared. They do
 * not all belong in a differential, and the line between them is the finding rather than an
 * inconvenience:
 *
 * - `sqrt`, `fabs`, `ceil`, `floor`, `trunc`, `round` and friends are **exactly specified**.
 *   IEEE-754 and ISO C give each input one correct answer, so a bit-for-bit comparison is a
 *   statement about the contract.
 * - `sin`, `cos`, `exp`, `log`, `pow` and the rest of the transcendentals are **not**.
 *   Implementations are permitted to differ in the last place, and comparing them bit-for-bit
 *   would pin orbistoun to *glibc's* libm - the same trap as comparing `rand` against glibc's
 *   generator, or `strerror` against its wording (D532).
 *
 * # And why bit patterns
 *
 * A decimal rendering hides exactly the differences worth catching, which is what `strtod`
 * already says on this side. It also hides the **sign of zero**, and that is not a detail here:
 * `round(-0.5)` is `-0.0` by the round-half-away-from-zero rule, and an implementation that
 * answers `+0.0` passes every comparison that goes through a decimal string. */
static void math_double(const char *id, const char *fn, double x) {
    double answer = 0.0;
    uint64_t in = 0;
    uint64_t out = 0;

    if (strcmp(fn, "sqrt") == 0) {
        answer = sqrt(x);
    } else if (strcmp(fn, "fabs") == 0) {
        answer = fabs(x);
    } else if (strcmp(fn, "ceil") == 0) {
        answer = ceil(x);
    } else if (strcmp(fn, "floor") == 0) {
        answer = floor(x);
    } else if (strcmp(fn, "trunc") == 0) {
        answer = trunc(x);
    } else if (strcmp(fn, "round") == 0) {
        answer = round(x);
    } else {
        fprintf(stderr, "unknown math function %s\n", fn);
        return;
    }

    memcpy(&in, &x, sizeof in);
    memcpy(&out, &answer, sizeof out);
    printf("REF|case|%s|%s|u:%llu\n", id, fn, (unsigned long long)in);
    printf("REF|ret|%s|0x%llx\n", id, (unsigned long long)out);
}

static void format_time(const char *id, const char *format, size_t room, int sec, int min,
                        int hour, int mday, int mon, int year, int wday, int yday) {
    struct tm when;
    char buffer[64];
    int nine[9];
    memset(&when, 0, sizeof when);
    when.tm_sec = sec;
    when.tm_min = min;
    when.tm_hour = hour;
    when.tm_mday = mday;
    when.tm_mon = mon;
    when.tm_year = year;
    when.tm_wday = wday;
    when.tm_yday = yday;
    when.tm_isdst = 0;
    nine[0] = sec;
    nine[1] = min;
    nine[2] = hour;
    nine[3] = mday;
    nine[4] = mon;
    nine[5] = year;
    nine[6] = wday;
    nine[7] = yday;
    nine[8] = 0;

    memset(buffer, '@', sizeof buffer);
    size_t wrote = strftime(buffer, room, format, &when);

    printf("REF|case|%s|strftime|u:%zu|s:", id, room);
    emit_text(format);
    printf("|");
    emit_bytes(nine, sizeof nine);
    printf("\n");
    printf("REF|ret|%s|0x%zx\n", id, wrote);
    if (wrote > 0) {
        printf("REF|out|%s|buffer|", id);
        for (size_t i = 0; i < 32; i++) {
            printf("%02x", (unsigned char)buffer[i]);
        }
        printf("\n");
    }
}

static void tokenise_interleaved(const char *id, const char *first, const char *second,
                                 const char *delims, int rounds) {
    char a[24];
    char b[24];
    char *save_a = NULL;
    char *save_b = NULL;
    memset(a, (int)'@', sizeof a);
    memset(b, (int)'@', sizeof b);
    strcpy(a, first);
    strcpy(b, second);

    int step = 0;
    for (int round = 0; round < rounds; round++) {
        for (int stream = 0; stream < 2; stream++) {
            char *buffer = stream == 0 ? a : b;
            char **save = stream == 0 ? &save_a : &save_b;
            const char *subject = stream == 0 ? first : second;
            char *token = strtok_r(round == 0 ? buffer : NULL, delims, save);

            printf("REF|case|%s#%d|strtok_r|", id, step);
            if (round == 0) {
                printf("s:");
                emit_text(subject);
            } else {
                printf("n:");
            }
            printf("|s:");
            emit_text(delims);
            printf("|u:%d\n", stream);
            printf("REF|ret|%s#%d|0x%x\n", id, step, token ? 1 : 0);
            if (token) {
                printf("REF|out|%s#%d|offset|0x%zx\n", id, step, (size_t)(token - buffer));
            }
            step++;
        }
    }
    /* Both buffers, on the last step, so each stream's terminators are visible. */
    printf("REF|out|%s#%d|buffer0|", id, step - 1);
    for (size_t i = 0; i < 16; i++) {
        printf("%02x", (unsigned char)a[i]);
    }
    printf("\n");
    printf("REF|out|%s#%d|buffer1|", id, step - 1);
    for (size_t i = 0; i < 16; i++) {
        printf("%02x", (unsigned char)b[i]);
    }
    printf("\n");
}

static void tokenise_r(const char *id, const char *subject, const char *delims, int calls) {
    char buffer[24];
    char *save = NULL;
    memset(buffer, '@', sizeof buffer);
    strcpy(buffer, subject);

    for (int i = 0; i < calls; i++) {
        char *token = strtok_r(i == 0 ? buffer : NULL, delims, &save);
        printf("REF|case|%s#%d|strtok_r|", id, i);
        if (i == 0) {
            printf("s:");
            emit_text(subject);
        } else {
            printf("n:");
        }
        printf("|s:");
        emit_text(delims);
        printf("\n");
        printf("REF|ret|%s#%d|0x%x\n", id, i, token ? 1 : 0);
        if (token) {
            printf("REF|out|%s#%d|offset|0x%zx\n", id, i, (size_t)(token - buffer));
        }
    }
    printf("REF|out|%s#%d|buffer|", id, calls - 1);
    for (size_t i = 0; i < 16; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

static void tokenise(const char *id, const char *subject, const char *delims, int calls) {
    char buffer[24];
    memset(buffer, '@', sizeof buffer);
    strcpy(buffer, subject);

    for (int i = 0; i < calls; i++) {
        char *token = strtok(i == 0 ? buffer : NULL, delims);
        printf("REF|case|%s#%d|strtok|", id, i);
        if (i == 0) {
            printf("s:");
            emit_text(subject);
        } else {
            /* Null, meaning "continue where the last call stopped" - a distinct argument
             * type, because an empty string is a different thing and both occur here. */
            printf("n:");
        }
        printf("|s:");
        emit_text(delims);
        printf("\n");
        printf("REF|ret|%s#%d|0x%x\n", id, i, token ? 1 : 0);
        if (token) {
            printf("REF|out|%s#%d|offset|0x%zx\n", id, i, (size_t)(token - buffer));
        }
    }
    /* The subject as the sequence left it, on the last step. */
    printf("REF|out|%s#%d|buffer|", id, calls - 1);
    for (size_t i = 0; i < 16; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* A move within one buffer, where the source and destination overlap.
 *
 * **The case a naive copy gets wrong in exactly one direction.** Copying front-to-back is
 * correct when the destination is below the source and corrupts when it is above, because the
 * bytes it is about to read have already been overwritten. `memmove` must handle both; `memcpy`
 * is allowed not to, which is why only `memmove` is asked here.
 *
 * The whole window is recorded, since the answer is entirely in the bytes. */
static void move_within(const char *id, const char *initial, size_t dest, size_t from,
                        size_t n) {
    char buffer[16];
    memset(buffer, '@', sizeof buffer);
    strcpy(buffer, initial);
    memmove(buffer + dest, buffer + from, n);

    printf("REF|case|%s|memmove|s:", id);
    emit_text(initial);
    printf("|u:%zu|u:%zu|u:%zu\n", dest, from, n);
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 12; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* A fill, whose one interesting edge is a length of zero. */
static void fill(const char *id, const char *initial, int value, size_t n) {
    char buffer[16];
    memset(buffer, '@', sizeof buffer);
    strcpy(buffer, initial);
    memset(buffer, value, n);

    printf("REF|case|%s|memset|s:", id);
    emit_text(initial);
    printf("|i:%d|u:%zu\n", value, n);
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 12; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

/* An unbounded copy or append into a poisoned window.
 *
 * What is being watched is the terminator and what is left past it: `strcpy` writes one and
 * stops, `strcat` finds the existing one and writes from there. A shim that copies the right
 * characters and forgets the NUL passes every test that reads the result as a string. */
static void unbounded_copy(const char *id, const char *fn, const char *initial,
                           const char *src) {
    char buffer[16];
    memset(buffer, '@', sizeof buffer);
    strcpy(buffer, initial);
    if (strcmp(fn, "strcpy") == 0) {
        strcpy(buffer, src);
    } else if (strcmp(fn, "strcat") == 0) {
        strcat(buffer, src);
    } else {
        fprintf(stderr, "unknown copy %s\n", fn);
        return;
    }
    printf("REF|case|%s|%s|s:", id, fn);
    emit_text(initial);
    printf("|s:");
    emit_text(src);
    printf("\n");
    printf("REF|ret|%s|0x0\n", id);
    printf("REF|out|%s|buffer|", id);
    for (size_t i = 0; i < 12; i++) {
        printf("%02x", (unsigned char)buffer[i]);
    }
    printf("\n");
}

int main(void) {
    emit_provenance();

    /* Conversions. The interesting half is the end pointer and the unspecified corners:
     * a subject with no digits at all, a prefix the base does not allow, and a base of 0
     * where the prefix chooses the base. */
    convert("strtoul/plain-decimal", "strtoul", "42", 10);
    convert("strtoul/trailing-text", "strtoul", "42abc", 10);
    convert("strtoul/no-digits", "strtoul", "abc", 10);
    convert("strtoul/leading-space", "strtoul", "   42", 10);
    convert("strtoul/hex-prefix-base-16", "strtoul", "0x2a", 16);
    convert("strtoul/hex-prefix-base-0", "strtoul", "0x2a", 0);
    convert("strtoul/hex-prefix-base-10", "strtoul", "0x2a", 10);
    convert("strtoul/octal-base-0", "strtoul", "052", 0);
    convert("strtoul/negative", "strtoul", "-1", 10);
    convert("strtoul/overflow", "strtoul", "99999999999999999999999", 10);
    convert("strtoull/overflow", "strtoull", "99999999999999999999999", 10);
    convert("strtol/negative", "strtol", "-42", 10);
    convert("strtol/underflow", "strtol", "-99999999999999999999999", 10);
    convert("strtol/plus-sign", "strtol", "+42", 10);
    convert("strtol/base-36", "strtol", "zz", 36);
    /* **Base 0 is three rules in one**, and a shim usually implements the first: a leading
     * `0x` means sixteen, a leading `0` alone means eight, anything else means ten. */
    convert("strtol/base-0-octal", "strtol", "010", 0);
    convert("strtol/base-0-decimal", "strtol", "10", 0);
    convert("strtol/base-0-hex-upper", "strtol", "0X1f", 0);
    convert("strtol/base-0-just-zero", "strtol", "0", 0);
    /* An uppercase prefix at an explicit base 16, which is the same rule one layer down. */
    convert("strtol/base-16-upper-prefix", "strtol", "0XFF", 16);
    /* **`0x` with nothing after it.** ISO C 7.22.1.4: the subject sequence is the longest
     * initial one of the expected form, so it is `0` - the value is zero and `end` points at
     * the `x`, not past it. An implementation that consumed the prefix first reports two. */
    convert("strtol/hex-prefix-no-digits", "strtol", "0x", 16);
    convert("strtol/hex-prefix-no-digits-base-0", "strtol", "0x", 0);
    /* A sign with no digits is no conversion at all: zero, and `end` back at the start. */
    convert("strtol/sign-only", "strtol", "-", 10);
    convert("strtol/plus-only", "strtol", "+", 10);
    /* Bases at the edges of the permitted range. */
    convert("strtol/base-2", "strtol", "1011", 2);
    /* Digits above nine are case-insensitive, which base 36 is the widest test of. */
    convert("strtol/base-36-mixed-case", "strtol", "Zz", 36);
    /* Whitespace is skipped, and a tab counts as whitespace exactly as a space does. */
    convert("strtol/leading-tab", "strtol", "\t-7", 10);

    /* Searches, including the empty-set edges the standard specifies and shims forget. */
    search("strspn/all-matching", "strspn", "aaabbb", "ab");
    search("strspn/none-matching", "strspn", "aaabbb", "xyz");
    search("strspn/empty-set", "strspn", "abc", "");
    search("strcspn/stops-at-first", "strcspn", "abc123", "0123456789");
    search("strcspn/never-stops", "strcspn", "abcdef", "0123456789");
    search("strcspn/empty-set", "strcspn", "abc", "");

    /* Truncation, where the return value and the buffer disagree on purpose. */
    truncating_format("snprintf/fits", 8, 42);
    truncating_format("snprintf/truncates", 3, 123456);
    truncating_format("snprintf/zero-room", 0, 42);

    /* `sprintf`, across the conversions and - the part that actually breaks - the specifier
     * parser. Each case names the thing it tests. */
    format_int("sprintf/decimal", "%d", 42);
    format_int("sprintf/negative", "%d", -42);
    format_int("sprintf/zero", "%d", 0);
    format_int("sprintf/signed-alias-i", "%i", -7);
    format_int("sprintf/unsigned", "%u", 42);
    format_int("sprintf/hex-lower", "%x", 48879);
    format_int("sprintf/hex-upper", "%X", 48879);
    format_int("sprintf/octal", "%o", 64);
    format_int("sprintf/pointer", "%p", 42);
    format_int("sprintf/character", "%c", 65);
    /* Flags and width - a right-justified field, a left-justified one, and zero padding,
     * which interacts with left-justification in the way implementations get wrong. */
    format_int("sprintf/width", "%5d", 42);
    format_int("sprintf/width-left", "%-5d", 42);
    format_int("sprintf/width-zero", "%05d", 42);
    format_int("sprintf/width-zero-negative", "%05d", -42);
    format_int("sprintf/precision", "%.3d", 42);
    format_int("sprintf/width-narrower-than-value", "%2d", 4242);
    /* Literal text around a conversion, and a literal percent, which must consume no
     * argument at all. */
    format_int("sprintf/embedded", "a%db", 42);
    format_int("sprintf/literal-percent", "100%%", 0);
    format_str("sprintf/string", "%s", "hello");
    format_str("sprintf/string-empty", "%s", "");
    format_str("sprintf/string-width", "%8s", "hi");
    format_str("sprintf/string-width-left", "%-8s", "hi");
    format_str("sprintf/string-precision-truncates", "%.2s", "hello");

    /* `vsnprintf` through a real `va_list`. The sweep is over the *number* of arguments,
     * because the register save area holds six and the seventh comes from the stack - an
     * implementation that only reads the save area passes every case below seven. */
    format_va("vsnprintf/one-argument", 64, "%d", 1, 42);
    format_va("vsnprintf/two-arguments", 64, "%d,%d", 2, 1, 2);
    format_va("vsnprintf/six-arguments-fill-the-registers", 64, "%d%d%d%d%d%d", 6, 1, 2, 3, 4, 5,
              6);
    format_va("vsnprintf/seven-crosses-into-the-stack", 64, "%d%d%d%d%d%d%d", 7, 1, 2, 3, 4, 5, 6,
              7);
    format_va("vsnprintf/eight-arguments", 64, "%d%d%d%d%d%d%d%d", 8, 1, 2, 3, 4, 5, 6, 7, 8);
    /* No arguments at all: the list must not be consulted. */
    format_va("vsnprintf/no-arguments", 64, "literal", 0);
    /* Truncation, and the size-zero form callers use to ask how long the answer would be. */
    format_va("vsnprintf/truncates", 4, "%d%d%d", 3, 11, 22, 33);
    format_va("vsnprintf/room-for-nothing-but-the-nul", 1, "%d", 1, 42);
    /* Mixed conversions across the crossover, so a wrong argument is visible as the wrong
     * *kind* of rendering rather than only as a wrong number. */
    format_va("vsnprintf/mixed-across-the-crossover", 64, "%d %x %u %d %x %u %d", 7, 1, 2, 3, 4, 5,
              6, 7);
    /* **Room for the digits and nothing else.** `snprintf` always terminates, so three bytes
     * of room for `123` writes `12` and a NUL - the off-by-one everybody gets wrong once. */
    truncating_format("snprintf/room-equals-length", 3, 123);
    /* One more byte, and it fits exactly. The pair is the test; either alone proves little. */
    truncating_format("snprintf/room-is-length-plus-one", 4, 123);
    /* A minus sign counts toward the length, and toward the truncation. */
    truncating_format("snprintf/negative-truncated", 3, -123);
    /* The return is the length it *would* have needed, not what it wrote. */
    truncating_format("snprintf/return-is-would-be-length", 2, 999999);

    /* Comparisons. The empty-string and prefix edges are where an off-by-one lives, and the
     * high-byte case is where a signed `char` comparison answers backwards. */
    compare("strcmp/equal", "strcmp", "abc", "abc", 0);
    compare("strcmp/less", "strcmp", "abc", "abd", 0);
    compare("strcmp/greater", "strcmp", "abd", "abc", 0);
    compare("strcmp/prefix-is-less", "strcmp", "ab", "abc", 0);
    compare("strcmp/empty-against-empty", "strcmp", "", "", 0);
    compare("strcmp/empty-is-less", "strcmp", "", "a", 0);
    compare("strcmp/high-byte", "strcmp", "\x80", "a", 0);
    compare("strcasecmp/mixed", "strcasecmp", "AbC", "aBc", 0);
    /* **The trap that catches a naive fold.** `[` is 0x5b and `{` is 0x7b - they differ by
     * exactly the 0x20 that separates a letter from its own upper case, so an implementation
     * that ORs 0x20 rather than asking whether the byte is a letter reports these equal.
     * Same for `@` against the backtick, and `_` against DEL is the pair one bit further out. */
    compare("strcasecmp/bracket-and-brace", "strcasecmp", "[", "{", 0);
    compare("strcasecmp/at-and-backtick", "strcasecmp", "@", "`", 0);
    /* Digits have no case at all, so folding must be a no-op on them. */
    compare("strcasecmp/digits-are-not-letters", "strcasecmp", "1", "Q", 0);
    /* A prefix is less than what extends it, case folding or not. */
    compare("strcasecmp/prefix-is-less", "strcasecmp", "ABC", "abcd", 0);
    compare("strcasecmp/equal-empty", "strcasecmp", "", "", 0);
    /* Beyond ASCII the comparison is on `unsigned char`, so a high byte is *greater* than a
     * letter rather than less - which is what an implementation folding through a signed
     * `char` gets backwards. */
    compare("strcasecmp/high-byte-is-greater", "strcasecmp", "\x80", "a", 0);
    compare("strncmp/within-bound", "strncmp", "abcX", "abcY", 3);
    compare("strncmp/at-bound", "strncmp", "abcX", "abcY", 4);
    compare("strncmp/zero-bound", "strncmp", "a", "b", 0);
    compare("strncasecmp/within-bound", "strncasecmp", "ABc", "abd", 2);
    /* **A bound of zero compares nothing**, so it is equal however different the strings are.
     * The one edge where returning the byte difference would be wrong rather than merely
     * unspecified. */
    compare("strncasecmp/zero-bound", "strncasecmp", "ABC", "xyz", 0);
    /* Differing exactly at the bound, and one past it: the first sees the difference, the
     * second does not, and an off-by-one shows up as one of the two answering wrongly. */
    compare("strncasecmp/differs-at-bound", "strncasecmp", "ABc", "abd", 3);
    compare("strncasecmp/differs-past-bound", "strncasecmp", "ABcX", "abcY", 3);
    /* A bound longer than either string: the NUL stops it, and reading past it would be a
     * fault the sanitizer catches rather than a wrong answer. */
    compare("strncasecmp/bound-past-nul", "strncasecmp", "AB", "ab", 8);
    compare("strncasecmp/bracket-and-brace", "strncasecmp", "[", "{", 1);
    compare("memcmp/equal", "memcmp", "abc", "abc", 3);
    compare("memcmp/differs", "memcmp", "abc", "abd", 3);
    compare("memcmp/zero-length", "memcmp", "a", "b", 0);
    compare("memcmp/high-byte", "memcmp", "\x80", "a", 1);

    /* Searches. The terminator is findable by `strchr` and is not by `memchr` unless it is
     * inside the length - a distinction shims collapse. */
    find("strchr/present", "strchr", "hello", 'l', 0);
    find("strchr/absent", "strchr", "hello", 'z', 0);
    find("strchr/finds-terminator", "strchr", "abc", '\0', 0);
    find("strrchr/last-of-many", "strrchr", "hello", 'l', 0);
    find("strrchr/finds-terminator", "strrchr", "abc", '\0', 0);
    find("memchr/within-length", "memchr", "hello", 'l', 5);
    find("memchr/past-length", "memchr", "hello", 'o', 3);
    find("memchr/zero-length", "memchr", "hello", 'h', 0);
    find_substring("strstr/present", "hello world", "wor");
    find_substring("strstr/absent", "hello world", "xyz");
    find_substring("strstr/empty-needle", "hello", "");
    find_substring("strstr/needle-is-subject", "abc", "abc");
    /* **Repeated prefixes, where a naive search restarts in the wrong place.** After matching
     * `aa` of `aab` and failing on the third byte, an implementation that resumes from where
     * it stopped rather than from one past where it started misses the match at offset 1. */
    find_substring("strstr/repeated-prefix", "aaab", "aab");
    find_substring("strstr/all-same", "aaaa", "aaa");
    /* A false start three bytes long before the real match - the same bug, further in. */
    find_substring("strstr/false-start", "abcabcabd", "abcabd");
    /* Nearly matching to the very end and then running out of subject. */
    find_substring("strstr/truncated-at-end", "abcab", "abcabc");
    find_substring("strstr/needle-longer-than-subject", "ab", "abc");
    /* At the last possible offset, which an off-by-one on the search bound drops. */
    find_substring("strstr/match-at-final-offset", "xxxab", "ab");
    /* An empty subject answers only an empty needle. */
    find_substring("strstr/empty-subject", "", "a");
    find_substring("strstr/both-empty", "", "");

    /* Bounded copies, whose whole behaviour is in the bytes. */
    bounded_copy("strncpy/pads-with-nuls", "strncpy", "", "ab", 6);
    bounded_copy("strncpy/does-not-terminate", "strncpy", "", "abcdef", 4);
    bounded_copy("strncpy/exact-fit", "strncpy", "", "abcd", 4);
    bounded_copy("strncat/appends-and-terminates", "strncat", "ab", "cdef", 2);
    bounded_copy("strncat/bound-larger-than-source", "strncat", "ab", "cd", 6);

    /* `strlen` and `memcpy`, the two most-called functions in the corpus with no case. */
    length_of("strlen/plain", "hello");
    length_of("strlen/empty", "");
    /* A single byte above 0x7f, written as an escape rather than typed: as a character
     * it becomes two bytes of UTF-8 and the case quietly stops being about a high byte.
     *
     * **The literal is split on purpose.** A hex escape in C has no length limit, so
     * "a\x80b" is the single character 0x80b truncated, not a high byte followed by
     * a letter - it measured 2 rather than 3 until this was noticed. */
    length_of("strlen/embedded-high-byte", "a\x80" "b");
    length_of("strlen/one-character", "x");
    bounded_copy("memcpy/exact-length", "memcpy", "@@@@", "abc", 3);
    bounded_copy("memcpy/does-not-terminate", "memcpy", "@@@@", "abcd", 4);
    bounded_copy("memcpy/zero-length-writes-nothing", "memcpy", "keep", "abc", 0);
    bounded_copy("memcpy/copies-the-terminator-when-asked", "memcpy", "@@@@", "ab", 3);

    /* Absolute values. `INT_MIN` is absent on purpose - negating it is undefined. */
    magnitude("abs/positive", "abs", 42);
    magnitude("abs/negative", "abs", -42);
    magnitude("abs/zero", "abs", 0);
    magnitude("labs/negative", "labs", -2147483647);
    magnitude("llabs/negative", "llabs", -4294967296LL);
    magnitude("llabs/positive", "llabs", 4294967296LL);

    /* `atoi` and friends - `strtol` with the error reporting removed. */
    to_integer("atoi/plain", "atoi", "42");
    to_integer("atoi/negative", "atoi", "-42");
    to_integer("atoi/leading-space", "atoi", "   42");
    to_integer("atoi/trailing-text", "atoi", "42abc");
    to_integer("atoi/no-digits", "atoi", "abc");
    to_integer("atoi/empty", "atoi", "");
    to_integer("atoi/plus-sign", "atoi", "+42");
    to_integer("atoi/leading-zeros-are-decimal", "atoi", "010");
    to_integer("atol/plain", "atol", "2147483647");
    to_integer("atoll/wider-than-a-long", "atoll", "4294967296");

    /* `strpbrk`, beside the `strcspn` cases that ask the same question as a length. The
     * not-found end is where the two disagree in shape: a length of `strlen` against a null. */
    find_any("strpbrk/first-of-set", "hello world", "ow");
    find_any("strpbrk/absent", "hello", "xyz");
    find_any("strpbrk/at-offset-zero", "hello", "h");
    find_any("strpbrk/empty-set", "hello", "");
    find_any("strpbrk/empty-subject", "", "abc");
    find_any("strpbrk/last-character", "hello", "o");
    find_any("strpbrk/every-character-matches", "aaa", "a");

    /* `strnlen`. The unterminated case is the one that matters, and it is why the bound is
     * shorter than the window: an implementation that delegates to `strlen` reads past it. */
    bounded_length("strnlen/shorter-than-bound", "abc", 8);
    bounded_length("strnlen/exactly-the-bound", "abcd", 4);
    bounded_length("strnlen/longer-than-bound", "abcdef", 3);
    bounded_length("strnlen/zero-bound", "abc", 0);
    bounded_length("strnlen/empty-string", "", 4);

    /* `strdup` and `strndup` - the last genuinely differentiable pair from the census (D532).
     *
     * **The bound must be shorter than the source** or nothing is being tested: a `strncpy`
     * shaped implementation is correct whenever the source fits, so `truncates` is the case
     * that carries this and the others are there to show the list is not only its edge. */
    duplicate("strdup/plain", "strdup", "abcdef", 0);
    duplicate("strdup/empty", "strdup", "", 0);
    duplicate("strdup/embedded-high-byte", "strdup", "a\x80b", 0);

    duplicate("strndup/truncates", "strndup", "abcdef", 3);
    duplicate("strndup/exactly-the-length", "strndup", "abc", 3);
    duplicate("strndup/bound-longer-than-source", "strndup", "ab", 8);
    duplicate("strndup/zero-bound", "strndup", "abcdef", 0);
    duplicate("strndup/empty-source", "strndup", "", 4);


    /* The wide-character family. Four functions that have been implemented and unverified the
     * whole time, because a decision said the record format could not carry them and nobody
     * checked (D529).
     *
     * **The parameter nobody varied here is the element width itself.** A wide string of `A`
     * then `U+0100` is `41 00 00 00 | 00 01 00 00`, whose *second byte* is zero - so an
     * implementation that walks bytes instead of elements stops at one where the answer is two.
     * That case is `narrow-would-stop-early`, and it is the reason these are worth recording at
     * all rather than assumed correct by symmetry with the narrow family. */
    {
        static const wchar_t empty[] = {0};
        static const wchar_t ascii[] = {L'a', L'b', L'c', 0};
        /* Second element's low byte is zero - a byte-walking length stops at 1. */
        static const wchar_t trap[] = {0x41, 0x100, 0x42, 0};
        /* Above the ASCII range, and above what a signed char could hold. */
        static const wchar_t high[] = {0x00ff, 0x1f600, 0x7fffffff, 0};

        wide_length("wcslen/empty", empty, 1);
        wide_length("wcslen/ascii", ascii, 4);
        wide_length("wcslen/narrow-would-stop-early", trap, 4);
        wide_length("wcslen/above-ascii", high, 4);

        /* Equal, then differing only *above* the low byte - `0x41` against `0x141` share a low
         * byte, so a comparison that reads bytes calls them equal. */
        static const wchar_t same_low[] = {0x141, 0};
        static const wchar_t one[] = {0x41, 0};
        wide_compare("wcscmp/equal", ascii, 4, ascii, 4);
        wide_compare("wcscmp/differs-above-the-low-byte", one, 2, same_low, 2);
        wide_compare("wcscmp/differs-below", same_low, 2, one, 2);
        /* A high value against a small one: signed element arithmetic answers backwards. */
        static const wchar_t big[] = {0x7fffffff, 0};
        wide_compare("wcscmp/high-against-low", big, 2, one, 2);
        wide_compare("wcscmp/prefix-is-less", ascii, 4, empty, 1);

        /* `wcsrchr` - the *last* match, which an implementation searching forwards gets wrong
         * whenever there is more than one. Searching for the terminator is defined and answers
         * the terminator's own position. */
        static const wchar_t repeated[] = {L'a', L'b', L'a', L'b', 0};
        wide_find_last("wcsrchr/last-of-two", repeated, 5, L'a');
        wide_find_last("wcsrchr/at-the-end", repeated, 5, L'b');
        wide_find_last("wcsrchr/absent", repeated, 5, L'z');
        wide_find_last("wcsrchr/the-terminator", repeated, 5, 0);
        wide_find_last("wcsrchr/above-ascii", high, 4, 0x1f600);

        /* `wcsncpy` - pads with terminators when short, does not terminate when not. */
        wide_copy("wcsncpy/pads-with-terminators", ascii, 4, 6);
        wide_copy("wcsncpy/does-not-terminate", ascii, 4, 2);
        wide_copy("wcsncpy/exact-fit", ascii, 4, 3);
        wide_copy("wcsncpy/zero-count", ascii, 4, 0);
        wide_copy("wcsncpy/above-ascii", high, 4, 4);
    }

    /* `strlcpy`, where the return is the truncation test and the buffer is the other half. */
    bounded_string_copy("strlcpy/fits", "@@@@", "abc", 8);
    bounded_string_copy("strlcpy/truncates", "@@@@", "abcdefgh", 4);
    bounded_string_copy("strlcpy/exact-fit-plus-nul", "@@@@", "abc", 4);
    bounded_string_copy("strlcpy/zero-size-writes-nothing", "keep", "abc", 0);
    bounded_string_copy("strlcpy/empty-source", "keep", "", 8);

    /* The character classes, swept. Thirteen functions that had no case at all between them,
     * and the title in front of us calls into this family 415 times in a run that dies after
     * 2,077 calls. */
    {
        static const char *const classes[] = {
            "isalnum", "isalpha", "iscntrl", "isdigit", "isgraph", "islower",
            "isprint", "ispunct", "isspace", "isupper", "isxdigit",
        };
        for (size_t i = 0; i < sizeof classes / sizeof *classes; i++) {
            char id[64];
            snprintf(id, sizeof id, "%s/low-half", classes[i]);
            classify_half(id, classes[i], 0);
            snprintf(id, sizeof id, "%s/high-half", classes[i]);
            classify_half(id, classes[i], 1);
        }
        /* The boundaries, and the two alphabets' own ends. `@[` and backtick-`{` are the
         * characters either side of `A-Z` and `a-z`. */
        static const int edges[] = {'@', 'A', 'Z', '[', '`', 'a', 'z', '{',
                                    '0', '9', ' ', 0x7f, 0};
        for (size_t i = 0; i < sizeof edges / sizeof *edges; i++) {
            char id[64];
            snprintf(id, sizeof id, "tolower/0x%02x", (unsigned)edges[i]);
            map_case(id, "tolower", edges[i]);
            snprintf(id, sizeof id, "toupper/0x%02x", (unsigned)edges[i]);
            map_case(id, "toupper", edges[i]);
        }
    }

    /* Sorting and searching, which are the two that call back into the caller's own code.
     * The duplicate and the already-sorted input are where an unstable or a naive
     * implementation shows itself. */
    {
        static const int32_t jumbled[] = {5, 3, 9, 1, 7};
        static const int32_t already[] = {1, 2, 3, 4};
        static const int32_t repeated[] = {4, 2, 4, 2};
        static const int32_t backwards[] = {9, 7, 5, 3};
        static const int32_t single[] = {42};
        static const int32_t negatives[] = {3, -1, 0, -7};
        sort_int32("qsort/jumbled", jumbled, 5);
        sort_int32("qsort/already-sorted", already, 4);
        sort_int32("qsort/duplicates", repeated, 4);
        sort_int32("qsort/reversed", backwards, 4);
        sort_int32("qsort/single-element", single, 1);
        sort_int32("qsort/negatives", negatives, 4);

        static const int32_t haystack[] = {1, 3, 5, 7, 9};
        search_int32("bsearch/first", haystack, 5, 1);
        search_int32("bsearch/middle", haystack, 5, 5);
        search_int32("bsearch/last", haystack, 5, 9);
        search_int32("bsearch/absent-inside", haystack, 5, 4);
        search_int32("bsearch/absent-below", haystack, 5, 0);
        search_int32("bsearch/absent-above", haystack, 5, 99);
        search_int32("bsearch/empty-array", haystack, 0, 1);
    }

    /* Tokenising, which needs a whole sequence and mutates what it walks. The empty field
     * and the leading delimiter are where implementations differ from each other and from
     * what people expect: C skips runs of delimiters, so `a,,b` has two tokens and not
     * three. One call past the end confirms it keeps answering null. */
    tokenise("strtok/simple", "a,b,c", ",", 4);
    tokenise("strtok/empty-fields-are-skipped", "a,,b", ",", 3);
    tokenise("strtok/leading-delimiter", ",,abc", ",", 2);
    tokenise("strtok/trailing-delimiter", "ab,", ",", 3);
    tokenise("strtok/no-delimiter-present", "abc", ",", 2);
    tokenise("strtok/all-delimiters", ",,,", ",", 2);
    tokenise("strtok/multiple-delimiters", "a;b,c", ",;", 4);

    /* `strtok_r`, the same sequences through a caller-held place. Same inputs as `strtok`
     * above on purpose: the two must agree with each other as well as with the reference,
     * and a difference between them is a shim that kept the place in the wrong object. */
    tokenise_r("strtok_r/simple", "a,b,c", ",", 4);
    tokenise_r("strtok_r/empty-fields-are-skipped", "a,,b", ",", 3);

    /* The interleaved pair - the only shape that can tell `strtok_r` from a delegating
     * `strtok`, and the reason the "record carries one subject per case" blocker was worth
     * disproving (D529). Three rounds, so each stream is continued twice after both started. */
    tokenise_interleaved("strtok_r/interleaved", "a,b,c", "x,y,z", ",", 3);
    tokenise_interleaved("strtok_r/interleaved-uneven", "one", "p,q,r", ",", 3);

    /* `strftime`. Implemented, and uncovered until now - the same class as the wide family
     * (D530): a function nobody had ever compared against a reference.
     *
     * **The parameter nobody varied is the specifier**, which is what D511 established for
     * `sprintf` after two bugs hid in one. So the format is the case input and the date is
     * held still, except where a boundary is the point.
     *
     * Only the conversions orbistoun claims are used, and no zone or locale ones: `%Z` and
     * `%z` need fields past the nine it reads, and comparing them would test this emulator's
     * lack of a timezone rather than its formatting.
     *
     * 2026-09-04 is a Friday; day 246 of the year. `tm_year` is years since 1900 and `tm_mon`
     * is January = 0, both of which are the off-by-one every implementation gets wrong once. */
    format_time("strftime/date", "%Y-%m-%d", 64, 7, 6, 5, 4, 8, 126, 5, 246);
    format_time("strftime/time", "%H:%M:%S", 64, 7, 6, 5, 4, 8, 126, 5, 246);
    /* `%F` and `%T` are the same two, spelled once - a parser that expands them wrongly still
     * passes every case above. */
    format_time("strftime/F-is-the-date", "%F", 64, 7, 6, 5, 4, 8, 126, 5, 246);
    format_time("strftime/T-is-the-time", "%T", 64, 7, 6, 5, 4, 8, 126, 5, 246);
    format_time("strftime/D-and-R", "%D %R", 64, 7, 6, 5, 4, 8, 126, 5, 246);
    /* `%e` is space-padded where `%d` is zero-padded, on a day that shows the difference. */
    format_time("strftime/e-pads-with-space", "[%e][%d]", 64, 0, 0, 0, 4, 8, 126, 5, 246);
    /* `%y` at a century boundary: 1999 and 2000 differ in the digit that gets dropped. */
    format_time("strftime/y-at-1999", "%y", 64, 0, 0, 0, 1, 0, 99, 5, 0);
    format_time("strftime/y-at-2000", "%y", 64, 0, 0, 0, 1, 0, 100, 6, 0);
    /* `%p` at midnight and noon - twelve and zero are where AM/PM goes wrong. */
    format_time("strftime/p-at-midnight", "%p", 64, 0, 0, 0, 1, 0, 126, 4, 0);
    format_time("strftime/p-at-noon", "%p", 64, 0, 0, 12, 1, 0, 126, 4, 0);
    /* The names, which come from the fields rather than from the date. */
    format_time("strftime/names", "%a %A %b %B", 64, 0, 0, 0, 4, 8, 126, 5, 246);
    /* `%j` is three digits, zero-padded, and is `tm_yday + 1`. */
    format_time("strftime/day-of-year", "%j", 64, 0, 0, 0, 4, 8, 126, 5, 246);
    format_time("strftime/day-of-year-is-one-based", "%j", 64, 0, 0, 0, 1, 0, 126, 4, 0);
    /* The escapes, and a literal that is not a conversion at all. */
    format_time("strftime/escapes", "%%|%n|%t|", 64, 0, 0, 0, 1, 0, 126, 4, 0);
    format_time("strftime/no-conversions", "plain text", 64, 0, 0, 0, 1, 0, 126, 4, 0);
    format_time("strftime/empty-format", "", 64, 0, 0, 0, 1, 0, 126, 4, 0);
    /* Too small: answers zero, and the buffer contents are unspecified so they are not
     * recorded. An implementation that half-renders and reports a short count fails here. */
    format_time("strftime/does-not-fit", "%Y-%m-%d", 4, 7, 6, 5, 4, 8, 126, 5, 246);
    /* Exactly the room needed, including the terminator - the off-by-one either side. */
    format_time("strftime/exact-room", "%Y", 5, 0, 0, 0, 1, 0, 126, 4, 0);
    format_time("strftime/one-short", "%Y", 4, 0, 0, 0, 1, 0, 126, 4, 0);

    /* The exactly-specified math functions. Implemented, and never compared until now - the
     * same class the wide family and `strftime` came from (D530, D532).
     *
     * **The parameter nobody varied is the sign of zero and the halfway value.** Every
     * rounding function has one input where a plausible implementation is wrong and a decimal
     * comparison cannot see it: `round(-0.5)` is `-0.0`, not `+0.0`, and the two differ only in
     * the sign bit. So the halfway cases and the signed zeros are the point, and the ordinary
     * values are there to show the case list is not only edges. */
    {
        const double zero = 0.0;
        const double minus_zero = -0.0;
        const double infinity = 1.0 / 0.0;
        const double minus_infinity = -1.0 / 0.0;
        const double nan_value = 0.0 / 0.0;
        /* The smallest positive subnormal double, built rather than named. */
        const double tiny = 4.9406564584124654e-324;

        math_double("sqrt/four", "sqrt", 4.0);
        math_double("sqrt/two", "sqrt", 2.0);
        math_double("sqrt/zero", "sqrt", zero);
        math_double("sqrt/minus-zero", "sqrt", minus_zero);
        math_double("sqrt/infinity", "sqrt", infinity);
        math_double("sqrt/subnormal", "sqrt", tiny);

        math_double("fabs/negative", "fabs", -3.5);
        math_double("fabs/minus-zero", "fabs", minus_zero);
        math_double("fabs/minus-infinity", "fabs", minus_infinity);
        math_double("fabs/nan", "fabs", nan_value);

        /* Rounding, at the halfway values where the four rules disagree. */
        math_double("ceil/half", "ceil", 0.5);
        math_double("ceil/negative-half", "ceil", -0.5);
        math_double("ceil/already-whole", "ceil", 3.0);
        math_double("ceil/minus-zero", "ceil", minus_zero);

        math_double("floor/half", "floor", 0.5);
        math_double("floor/negative-half", "floor", -0.5);
        math_double("floor/already-whole", "floor", 3.0);
        math_double("floor/minus-zero", "floor", minus_zero);

        math_double("trunc/positive", "trunc", 1.75);
        math_double("trunc/negative", "trunc", -1.75);
        math_double("trunc/below-one", "trunc", 0.75);
        math_double("trunc/negative-below-one", "trunc", -0.75);

        /* `round` is half-away-from-zero, which is *not* what the hardware's default rounding
         * does - a shim built on a rint-style instruction answers 2 for 2.5 and 0 for -0.5,
         * and only these cases say so. */
        math_double("round/half-up", "round", 0.5);
        math_double("round/half-away-from-zero", "round", -0.5);
        math_double("round/two-and-a-half", "round", 2.5);
        math_double("round/one-and-a-half", "round", 1.5);
        math_double("round/minus-two-and-a-half", "round", -2.5);
        math_double("round/infinity", "round", infinity);
    }

    /* Single precision, and the pair that pins the rounding rule against each other. */
    {
        const float zerof = 0.0f;
        const float minus_zerof = -0.0f;
        const float infinityf = 1.0f / 0.0f;
        const float minus_infinityf = -1.0f / 0.0f;
        const float nanf_value = 0.0f / 0.0f;
        /* The smallest positive subnormal float, built rather than named. */
        const float tinyf = 1.40129846e-45f;

        math_float("sqrtf/four", "sqrtf", 4.0f);
        math_float("sqrtf/two", "sqrtf", 2.0f);
        math_float("sqrtf/minus-zero", "sqrtf", minus_zerof);
        math_float("sqrtf/infinity", "sqrtf", infinityf);
        math_float("sqrtf/subnormal", "sqrtf", tinyf);

        /* The only NaN case: `fabsf` clears the sign bit and the payload survives, so the
         * answer is specified where `sqrtf(NaN)` is not. */
        math_float("fabsf/negative", "fabsf", -3.5f);
        math_float("fabsf/minus-zero", "fabsf", minus_zerof);
        math_float("fabsf/minus-infinity", "fabsf", minus_infinityf);
        math_float("fabsf/nan", "fabsf", nanf_value);

        math_float("ceilf/half", "ceilf", 0.5f);
        math_float("ceilf/negative-half", "ceilf", -0.5f);
        math_float("ceilf/minus-zero", "ceilf", minus_zerof);
        math_float("floorf/half", "floorf", 0.5f);
        math_float("floorf/negative-half", "floorf", -0.5f);
        math_float("floorf/zero", "floorf", zerof);
        math_float("truncf/positive", "truncf", 1.75f);
        math_float("truncf/negative", "truncf", -1.75f);
        math_float("truncf/negative-below-one", "truncf", -0.75f);

        /* **The discriminating pair.** `roundf` is half-away-from-zero and `nearbyintf` is
         * half-to-even, so these four inputs give four different answers between them. An
         * implementation of either built on the other fails here and nowhere else. */
        math_float("roundf/half", "roundf", 0.5f);
        math_float("roundf/negative-half", "roundf", -0.5f);
        math_float("roundf/two-and-a-half", "roundf", 2.5f);
        math_float("roundf/one-and-a-half", "roundf", 1.5f);
        math_float("nearbyintf/half", "nearbyintf", 0.5f);
        math_float("nearbyintf/negative-half", "nearbyintf", -0.5f);
        math_float("nearbyintf/two-and-a-half", "nearbyintf", 2.5f);
        math_float("nearbyintf/one-and-a-half", "nearbyintf", 1.5f);
        math_float("nearbyintf/already-whole", "nearbyintf", 3.0f);
    }



    tokenise_r("strtok_r/leading-delimiter", ",,abc", ",", 2);
    tokenise_r("strtok_r/trailing-delimiter", "ab,", ",", 3);
    tokenise_r("strtok_r/no-delimiter-present", "abc", ",", 2);
    tokenise_r("strtok_r/all-delimiters", ",,,", ",", 2);

    /* Overlapping moves, both directions, plus the degenerate ones. */
    move_within("memmove/destination-below-source", "abcdef", 0, 2, 4);
    move_within("memmove/destination-above-source", "abcdef", 2, 0, 4);
    move_within("memmove/same-address", "abcdef", 1, 1, 4);
    move_within("memmove/zero-length", "abcdef", 0, 3, 0);
    move_within("memmove/adjacent-no-overlap", "abcdef", 4, 0, 2);

    /* Fills. */
    fill("memset/whole-prefix", "abcdef", 'X', 3);
    fill("memset/zero-length", "abcdef", 'X', 0);
    fill("memset/writes-nul", "abcdef", 0, 2);
    /* **The value is converted to `unsigned char`**, so 255 writes 0xff. An implementation
     * that carries it through a signed `char` and then sign-extends writes the same byte by
     * luck here, but one that range-checks the `int` first refuses a value it should accept. */
    fill("memset/high-value", "abcdef", 255, 3);
    /* Above 255, where the conversion is a truncation and not a clamp: 0x141 becomes 0x41. */
    fill("memset/value-above-a-byte", "abcdef", 0x141, 2);

    /* Unbounded copies, where the terminator is the half that gets forgotten. */
    unbounded_copy("strcpy/shorter-than-existing", "strcpy", "abcdef", "xy");
    unbounded_copy("strcpy/empty-source", "strcpy", "abcdef", "");
    unbounded_copy("strcat/appends", "strcat", "abc", "de");
    unbounded_copy("strcat/empty-source", "strcat", "abc", "");
    unbounded_copy("strcat/onto-empty", "strcat", "", "xyz");

    /* Float conversion, compared as bits so a last-place difference cannot hide. */
    convert_float("strtod/plain", "3.5");
    convert_float("strtod/exponent", "1e3");
    convert_float("strtod/trailing-text", "2.5xyz");
    convert_float("strtod/no-digits", "xyz");
    convert_float("strtod/negative", "-0.5");
    convert_float("strtod/one-third", "0.333333333333333333");

    /* `strtof`, at the width where a double-then-narrow implementation shows itself. The
     * first three are ordinary; the rest are where single precision runs out - a value that
     * needs more digits than a float holds, one below the smallest normal, and the overflow
     * and underflow that set errno. */
    convert_float32("strtof/plain", "1.5");
    convert_float32("strtof/negative", "-2.25");
    convert_float32("strtof/integral", "42");
    convert_float32("strtof/needs-rounding", "0.1");
    convert_float32("strtof/more-digits-than-a-float-holds", "16777217");
    convert_float32("strtof/subnormal", "1e-40");
    convert_float32("strtof/overflow", "1e39");
    convert_float32("strtof/underflow", "1e-46");
    convert_float32("strtof/no-digits", "abc");
    convert_float32("strtof/trailing-text", "1.5abc");
    convert_float32("strtof/leading-space", "   1.5");
    convert_float32("strtof/exponent", "1.5e3");

    return 0;
}
