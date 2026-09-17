//! The character-classification tables, read from an obSCEne hardware capture.
//!
//! # Why these are measured rather than transcribed
//!
//! `_Getpctype` answers a pointer to the table `isalpha`, `isdigit` and the rest index. A
//! guest that calls it dereferences what it gets back, so a placeholder is not a stub
//! returning the wrong answer - it is an address the guest reads through (D459).
//!
//! The table's *shape* is documented: FreeBSD's `ctype.h` names the bits, and D448 recorded
//! that as the lawful oracle available at the time. But the bit values are a property of the
//! platform's own C library, not of the standard, and nothing lawful says what this one
//! chose. A capture reads them off the hardware, which is the difference between a table
//! that classifies characters correctly and one that merely classifies them plausibly.
//!
//! # What is read, and why it is within the boundary
//!
//! Bytes of a data table, from a probe this project wrote, running on a console somebody
//! owns. Not disassembly, not vendor source, not a derived constant: the same category as
//! any other `measured` fact, and recorded as one. The capture also carries its own
//! spot-checks - the classification of a dozen named characters, read through the *running*
//! library rather than out of the table - and [`run`] refuses to emit a table that disagrees
//! with them. A parse that silently shifted by one entry would otherwise produce a file that
//! looks entirely reasonable.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result};

/// The probe that captured these, as it names itself in the log.
const PROBE: &str = "035-libc/getpctype";

/// Bytes per entry. Each is a `u16`, little-endian.
const ENTRY_BYTES: usize = 2;

/// How far below the returned pointer the capture starts, **in bytes**.
///
/// C indexes these tables with a character *or* `EOF`, so a conforming table has entries
/// below zero, and the pointer the guest receives cannot be the start of the allocation.
///
/// **The record is named `table_raw_neg16` and that sixteen is bytes, not entries.** Read as
/// entries it puts index 0 half a table away, and every classification comes out wrong while
/// the file still looks entirely reasonable - which is precisely what the spot-checks caught
/// the first time this was run against real hardware. Eight entries, confirmed by solving the
/// alignment that satisfies all twelve of them at once.
const NEGATIVE_MARGIN_BYTES: usize = 16;

/// The same margin counted in entries, which is how the tables are indexed.
const NEGATIVE_MARGIN: usize = NEGATIVE_MARGIN_BYTES / ENTRY_BYTES;

/// One table, and what it answers.
#[derive(Debug, Clone, Copy)]
struct Table {
    /// The symbol the guest imports, exactly as it is spelled.
    symbol: &'static str,
    /// The section name in the rendered file.
    section: &'static str,
    /// One line saying what it holds.
    purpose: &'static str,
}

/// The three tables the probe captures.
const TABLES: &[Table] = &[
    Table {
        symbol: "_Getpctype",
        section: "pctype",
        purpose: "Classification bits - what `isalpha`, `isdigit` and the rest test.",
    },
    Table {
        symbol: "_Getptolower",
        section: "tolower",
        purpose: "The lower-case mapping `tolower` reads.",
    },
    Table {
        symbol: "_Getptoupper",
        section: "toupper",
        purpose: "The upper-case mapping `toupper` reads.",
    },
];

/// A spot-check the capture carries: one entry, read through the running library.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpotCheck {
    /// The index that was classified, which may be negative.
    index: i32,
    /// What the library answered.
    value: u16,
    /// The key as the log spells it, for the message when it disagrees.
    key: String,
}

/// Reassembles one table's bytes from the capture's fixed-width rows.
///
/// The rows carry their own offset, so a missing or duplicated one is caught here rather
/// than shifting every entry after it - the failure that would otherwise produce a plausible
/// table.
fn bytes_of(text: &str, symbol: &str) -> Result<Vec<u8>> {
    let mut rows: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        let [_, "bytes", probe, name, "table_raw_neg16", offset, hex] = fields.as_slice() else {
            continue;
        };
        if *probe != PROBE || *name != symbol {
            continue;
        }
        let offset: usize = offset
            .parse()
            .with_context(|| format!("{symbol}: offset {offset} is not a number"))?;
        let bytes = decode_hex(hex)
            .with_context(|| format!("{symbol}: row at offset {offset} is not hexadecimal"))?;
        anyhow::ensure!(
            rows.insert(offset, bytes).is_none(),
            "{symbol}: offset {offset} appears twice - the capture is not a clean run"
        );
    }
    anyhow::ensure!(!rows.is_empty(), "{symbol}: the capture has no table rows");

    let mut out = Vec::new();
    for (offset, bytes) in rows {
        anyhow::ensure!(
            offset == out.len(),
            concat!(
                "{}: rows jump from {} to {} - the capture is missing a row, and every ",
                "entry after the gap would be wrong"
            ),
            symbol,
            out.len(),
            offset
        );
        out.extend_from_slice(&bytes);
    }
    anyhow::ensure!(
        out.len() % ENTRY_BYTES == 0,
        "{symbol}: {} bytes is not a whole number of entries",
        out.len()
    );
    Ok(out)
}

/// Decodes a row of hexadecimal into bytes.
fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(hex.len() % 2 == 0, "an odd number of hexadecimal digits");
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).map_err(anyhow::Error::from))
        .collect()
}

/// The entries of one table, in index order starting at `-NEGATIVE_MARGIN`.
fn entries_of(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(ENTRY_BYTES)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

/// The spot-checks the capture recorded for one table.
///
/// Keys are `<what>_<label>_<index>`, with `neg1` for `EOF`. The label is there for a human
/// reading the log and is deliberately ignored here: the index is what addresses the table.
fn spot_checks(text: &str, symbol: &str) -> Result<Vec<SpotCheck>> {
    let mut found = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        let [_, "measure", probe, name, key, value, _kind] = fields.as_slice() else {
            continue;
        };
        if *probe != PROBE || *name != symbol {
            continue;
        }
        let Some((_, index)) = key.rsplit_once('_') else {
            continue;
        };
        let index: i32 = match index.strip_prefix("neg") {
            Some(magnitude) => -magnitude.parse::<i32>()?,
            // The pointer is the table's address on the console, which says nothing about
            // what is in it and carries no trailing index.
            None => match index.parse() {
                Ok(index) => index,
                Err(_) => continue,
            },
        };
        let value = value.strip_prefix("0x").unwrap_or(value);
        let value = u32::from_str_radix(value, 16)
            .with_context(|| format!("{symbol}: {key} is not a hexadecimal value"))?;
        found.push(SpotCheck {
            index,
            // The log widens the `EOF` entry to 0xffff; the table holds it in sixteen bits.
            value: u16::try_from(value & 0xFFFF)?,
            key: (*key).to_owned(),
        });
    }
    Ok(found)
}

/// Checks a parsed table against the spot-checks the same capture recorded.
///
/// The point of the exercise. The rows are bytes at offsets; the spot-checks are answers the
/// *running* library gave for named characters. They were produced by different paths, so
/// agreeing means the parse landed on the right entries - and an off-by-one, a wrong
/// endianness or a missed negative margin makes them disagree loudly instead of producing a
/// table that reads perfectly and classifies wrongly.
fn verify(symbol: &str, entries: &[u16], checks: &[SpotCheck]) -> Result<()> {
    anyhow::ensure!(
        !checks.is_empty(),
        "{symbol}: the capture carries no spot-checks, so nothing confirms the parse"
    );
    for check in checks {
        let margin = i32::try_from(NEGATIVE_MARGIN)?;
        let at = usize::try_from(check.index + margin)
            .with_context(|| format!("{symbol}: {} is below the captured margin", check.key))?;
        let got = entries.get(at).copied().with_context(|| {
            format!(
                "{symbol}: {} is past the end of the captured table",
                check.key
            )
        })?;
        anyhow::ensure!(
            got == check.value,
            concat!(
                "{}: {} says {:#x}, but the table's entry {} is {:#x} - the parse is wrong, ",
                "and a table that disagrees with the library it came from must not be written"
            ),
            symbol,
            check.key,
            check.value,
            check.index,
            got
        );
    }
    Ok(())
}

/// The rendered file's preamble.
fn header(capture: &str) -> String {
    let first = -(NEGATIVE_MARGIN as i64);
    format!(
        concat!(
            "# Character-classification tables, measured on real hardware.\n",
            "#\n",
            "# GENERATED by `orbistoun-gen ctype` from an obSCEne capture. Do not hand-edit:\n",
            "# an entry typed in by a person is one nobody can trace back to a measurement.\n",
            "#\n",
            "# Source: obSCEne probe `{}`, capture `{}`.\n",
            "#\n",
            "# known_by = \"measured\". The table's *shape* is documented - FreeBSD's ctype.h\n",
            "# names the bits - but the values are the platform C library's own, and nothing\n",
            "# lawful says what it chose. These were read off a console, and each table is\n",
            "# checked against spot-checks the same capture took through the running library\n",
            "# before it is written here.\n",
            "#\n",
            "# Entries run from index {} upward, because C indexes these with a character\n",
            "# or EOF and a conforming table has entries below zero. `first_index` says so\n",
            "# rather than leaving a reader to infer it.\n"
        ),
        PROBE, capture, first
    )
}

/// Reads a capture and renders the tables.
pub(crate) fn run(source: &Path) -> Result<String> {
    use std::fmt::Write as _;

    let text = std::fs::read_to_string(source).with_context(|| {
        format!(
            "reading {} - point this at an obSCEne capture containing the `{PROBE}` probe",
            source.display()
        )
    })?;

    let capture = source.file_name().map_or_else(
        || "unknown".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut out = header(&capture);

    for table in TABLES {
        let bytes = bytes_of(&text, table.symbol)?;
        let entries = entries_of(&bytes);
        let checks = spot_checks(&text, table.symbol)?;
        verify(table.symbol, &entries, &checks)?;

        let _ = write!(out, "\n[{}]\n", table.section);
        let _ = writeln!(out, "# {}", table.purpose);
        let _ = writeln!(
            out,
            "# {} entries, confirmed against {} spot-check(s) from the same capture.",
            entries.len(),
            checks.len()
        );
        let _ = writeln!(out, "symbol = \"{}\"", table.symbol);
        let _ = writeln!(out, "known_by = \"measured\"");
        let _ = writeln!(out, "first_index = {}", -(NEGATIVE_MARGIN as i64));
        let _ = writeln!(out, "entries = [");
        for row in entries.chunks(16) {
            let rendered: Vec<String> = row.iter().map(|entry| format!("{entry:#06x}")).collect();
            let _ = writeln!(out, "    {},", rendered.join(", "));
        }
        let _ = writeln!(out, "]");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two entries: index -8 is 0x0001, index -7 is 0x0002.
    const ROWS: &str = "OBS|bytes|035-libc/getpctype|_Getpctype|table_raw_neg16|0|01000200\n";

    #[test]
    fn a_table_is_read_little_endian_from_its_negative_margin() {
        let entries = entries_of(&bytes_of(ROWS, "_Getpctype").unwrap());
        assert_eq!(entries, vec![0x0001, 0x0002]);
    }

    /// The guard, made to fail: a spot-check that disagrees with the bytes must stop the
    /// table being written. Without this the parse could shift by an entry and produce a
    /// file that reads perfectly and classifies wrongly.
    #[test]
    fn a_table_disagreeing_with_its_spot_check_is_refused() {
        let text =
            format!("{ROWS}OBS|measure|035-libc/getpctype|_Getpctype|mask_x_neg8|0x2|mask\n");
        let entries = entries_of(&bytes_of(&text, "_Getpctype").unwrap());
        let checks = spot_checks(&text, "_Getpctype").unwrap();
        let error = verify("_Getpctype", &entries, &checks)
            .expect_err("a table that disagrees with its own capture must be refused");
        assert!(
            error.to_string().contains("the parse is wrong"),
            "the refusal should say the parse is wrong: {error}"
        );
    }

    /// The agreeing case, so the guard above is known to be discriminating rather than
    /// always failing.
    #[test]
    fn a_table_agreeing_with_its_spot_check_is_accepted() {
        let text =
            format!("{ROWS}OBS|measure|035-libc/getpctype|_Getpctype|mask_x_neg8|0x1|mask\n");
        let entries = entries_of(&bytes_of(&text, "_Getpctype").unwrap());
        let checks = spot_checks(&text, "_Getpctype").unwrap();
        verify("_Getpctype", &entries, &checks).expect("an agreeing table must be accepted");
    }

    /// A capture with no spot-checks confirms nothing, so it is refused rather than trusted.
    #[test]
    fn a_table_with_no_spot_checks_is_refused() {
        let entries = entries_of(&bytes_of(ROWS, "_Getpctype").unwrap());
        let error = verify("_Getpctype", &entries, &[])
            .expect_err("a table nothing confirms must be refused");
        assert!(error.to_string().contains("nothing confirms"), "{error}");
    }

    /// A missing row would shift every entry after it, so the gap is an error rather than a
    /// silently shorter table.
    #[test]
    fn a_gap_in_the_rows_is_refused() {
        let text = concat!(
            "OBS|bytes|035-libc/getpctype|_Getpctype|table_raw_neg16|0|0100\n",
            "OBS|bytes|035-libc/getpctype|_Getpctype|table_raw_neg16|16|0200\n"
        );
        let error = bytes_of(text, "_Getpctype").expect_err("a gap must be refused");
        assert!(error.to_string().contains("missing a row"), "{error}");
    }
}
