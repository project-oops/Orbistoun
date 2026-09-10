# D611 - A parser that required a prefix the probe does not always write

**Status:** measured
**Date:** 2026-09-08

## Found by using it

Writing assertions against the measurements D609 brought in, one of them panicked on
`.expect("a code is a number")`. The measurement is `000-hw/sw-version:…:rc` and its value is
`0`.

```rust
fn parse_hex(text: &str) -> Option<u64> {
    let first = text.split_whitespace().next()?;
    u64::from_str_radix(first.strip_prefix("0x")?, 16).ok()
}
```

`Measurement::value` required the `0x`. Most of the probe's fields carry one and two of its
checks do not - `000-hw/sw-version` writes `0` and `000-hw/tsc-frequency` writes `1596300187` -
so every measurement from those checks answered `None`.

**The panic is the good outcome.** A caller writing `.expect(…)` finds this in one run; a caller
writing `.unwrap_or(0)` never finds it at all and quietly asserts against a zero nobody
measured. That second shape is what the parser is now written to make impossible: `0x10` is
sixteen, `10` is ten, and a field holding a word is still `None`.

The renaming from `parse_hex` to `parse_number` is not cosmetic. A function called `parse_hex`
that also reads decimal is the next person's bug.

## Three frequencies for one quantity

Reading decimal made `000-hw/tsc-frequency` visible, and it disagrees with the check already in
the table:

| source | value |
|---|--:|
| `000-hw/tsc-frequency`, twice | 1,596,300,187 |
| `120-measure/frequencies`, this morning | 1,596,300,179 |
| `120-measure/frequencies`, earlier | 1,596,300,174 |

Thirteen hertz apart in 1.6 GHz - eight parts per billion, which is a per-boot calibration and
exactly what `the_counter_frequency_matches_the_console` already says. `000-hw/tsc-frequency` is
marked constant only because its two runs happened to agree with each other.

## Six more measurements asserted, and one that resisted

`sceKernelSyncOnAddressWake` answering `0` where nothing was waiting is the condition worth
pinning, because the tempting implementation refuses it: a wake that found no waiter did
nothing, and a call that did nothing looks like a call that failed.

A guard nobody had needed before then fired - `two_checks_of_one_fact_do_not_disagree_about_which_list_it_is_in`.
Three conditions across two checks record the same fact, that this call answers `0`, and claiming
one while deferring the others files one fact two ways. All three are claimed, and the assertion
says explicitly that it covers the return code and **not** the release: whether a waiter was woken
is a second fact these measurements do not carry, and it is not smuggled in under a code that
would pass without it.

`getifaddrs` and `sceUserServiceGetInitialUser` agreed with the console first time.

**`sceKernelGetSystemSwVersion` did not, and is right not to.** The console answers `0` because a
console has a software version; orbistoun refuses with `0x80020002` when none is configured,
which is what D420 chose over answering a made-up one. So the call is correct and the claim needs
a machine presented before it - and `machine::present` is a process-wide `OnceLock`, so a test
that set it would decide what every other test in that binary sees according to which ran first.
It goes back on the outstanding list carrying that, which is more than it carried before somebody
tried.
