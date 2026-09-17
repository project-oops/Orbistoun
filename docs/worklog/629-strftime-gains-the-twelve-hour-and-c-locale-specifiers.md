# 629. strftime gains the twelve-hour clock, the weekday numbers, and the C-locale forms

**2026-09-16** - eight more conversion specifiers, each a pure function of `struct tm`, so a title
that stamps a time with `%I%p` or `%c` renders it instead of being refused

## The gap

`strftime` (`crates/orbistoun-libc/src/scan.rs`) rendered a solid set - `%Y %y %m %d %e %H %M %S %j
%p %a %A %b %B %F %T %R %D`, plus the escapes - and **refused everything else** with `_ => return 0`.
The refusal is the right default (a half-rendered timestamp is a wrong date, not a short one,
principle 3), but it caught specifiers that are ordinary, unambiguous, and derivable from the fields
`struct tm` already holds:

- `%I` - the hour on the twelve-hour clock.
- `%r` - the twelve-hour time in full, `%I:%M:%S %p`.
- `%w` / `%u` - the weekday as a number, Sunday-zero and ISO (Monday-one, Sunday-seven).
- `%C` - the century.
- `%c` / `%x` / `%X` - the C-locale date-and-time, date, and time.

A title formatting a time with any of them got `0` - the same "cannot render" answer a real gap
gives - even though every one is a fixed transform of `tm`.

## The fix

Added the eight specifiers. Each is a value the `tm` fields determine, with no timezone or locale
data required:

- `%I` maps hour to `12` at midnight and noon (`hour % 12 == 0`) and to `hour % 12` otherwise; `%r`
  composes it with `%p`'s AM/PM.
- `%w` is `tm_wday`; `%u` is the same but with Sunday moved from zero to seven.
- `%C` is `(year + 1900) / 100`.
- `%x` and `%X` are `%D` and `%T` outright - the C locale defines them as exactly those, so they
  fold into those arms rather than repeating the format (clippy's identical-arm lint caught the first
  draft that spelled them out, which was the honest signal that they are the same rendering).
- `%c` is `%a %b %e %H:%M:%S %Y`, and it refuses a broken `tm_wday`/`tm_mon` exactly as `%a`/`%b`
  do alone - a `%c` that rendered a wrong day name would hide the malformed struct.

**What stays refused, deliberately:** the timezone forms (`%z`, `%Z` - no timezone is modelled, and
inventing `+0000` would claim one) and the week-number forms (`%U`, `%W`, `%V`, `%G` - the ISO week
rules are subtle enough that a wrong week is worse than a refusal). The `_ => return 0` still guards
them, and the existing `%Z` test still passes.

## Provenance

ISO C 7.29.3.5 (`strftime`) and POSIX.1-2008 fix every one of these; the C-locale definitions of
`%c`/`%x`/`%X` are the ones a `setlocale(LC_TIME, "C")` program gets, which is the only locale
modelled. No vendor source - a rendered timestamp is POSIX, not console firmware.

## Made to fail

- `the_twelve_hour_and_c_locale_forms_render` - against 2026-08-29 21:47:05 (a Saturday afternoon):
  `%I`=`09`, `%r`=`09:47:05 PM`, `%w %u`=`6 6`, `%C`=`20`, `%x %X`=`08/29/26 21:47:05`,
  `%c`=`Sat Aug 29 21:47:05 2026`.
- `midnight_and_sunday_hit_the_clock_and_weekday_edges` - the two edges the arithmetic turns on:
  midnight's `%I` is `12` (not `0`) and its `%r` says `AM`, and Sunday is the one weekday where
  `%w`=`0` and `%u`=`7` disagree. Both would fail the naive `hour % 12` / bare-`wday` renderings.

## Gate state

`cargo test -p orbistoun-libc` 132 pass in the scan module (2 new tests), whole crate green;
`cargo clippy -p orbistoun-libc --all-targets` clean; fmt clean.
